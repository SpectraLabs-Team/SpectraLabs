#!/usr/bin/env bash
# Copy the newest database dump and artifact snapshot to an off-host S3-compatible
# bucket (M28 Task 5), then verify the copy.
#
# Usage: scripts/copy-offhost.sh [OUTPUT_DIR]          (default OUTPUT_DIR: backups)
#
#   OFFHOST_TARGET                 s3://bucket/prefix for the second copy (required)
#   OFFHOST_AWS_ACCESS_KEY_ID      credentials for the off-host bucket only (required).
#   OFFHOST_AWS_SECRET_ACCESS_KEY  There is no fallback to the production keys.
#   OFFHOST_AWS_REGION             default us-east-1
#   OFFHOST_AWS_ENDPOINT           endpoint URL, for providers other than AWS (optional)
#   ARTIFACT_S3_BUCKET             the live artifact bucket. The copy refuses to write to it.
#
# Each run writes OFFHOST_TARGET/<stamp>/db/ (dump and .sha256) and
# OFFHOST_TARGET/<stamp>/artifacts/ (the snapshot with its MANIFEST.sha256).
# Verification: the dump is downloaded back and its SHA-256 compared with the local
# file, and the artifact object count is compared with the local file count.
#
# Retention on the off-host bucket is not done here. Set it with a bucket lifecycle
# rule (owner decision: M28 Decision 5). This script never deletes.
set -euo pipefail

out_dir="${1:-backups}"
target="${OFFHOST_TARGET:?set OFFHOST_TARGET to s3://bucket/prefix for the off-host copy}"
case "$target" in
  s3://?*) ;;
  *) echo "offhost: OFFHOST_TARGET must look like s3://bucket/prefix" >&2; exit 2 ;;
esac
: "${OFFHOST_AWS_ACCESS_KEY_ID:?set OFFHOST_AWS_ACCESS_KEY_ID (off-host credentials, not the production keys)}"
: "${OFFHOST_AWS_SECRET_ACCESS_KEY:?set OFFHOST_AWS_SECRET_ACCESS_KEY (off-host credentials)}"
command -v aws >/dev/null || { echo "offhost: the AWS CLI is required" >&2; exit 2; }

target_bucket="${target#s3://}"
target_bucket="${target_bucket%%/*}"
if [ -n "${ARTIFACT_S3_BUCKET:-}" ] && [ "$target_bucket" = "$ARTIFACT_S3_BUCKET" ]; then
  echo "offhost: refusing to copy into the live artifact bucket $ARTIFACT_S3_BUCKET" >&2
  exit 2
fi

# Use the off-host credentials for every aws call in this script only.
export AWS_ACCESS_KEY_ID="$OFFHOST_AWS_ACCESS_KEY_ID"
export AWS_SECRET_ACCESS_KEY="$OFFHOST_AWS_SECRET_ACCESS_KEY"
export AWS_DEFAULT_REGION="${OFFHOST_AWS_REGION:-us-east-1}"
endpoint=()
if [ -n "${OFFHOST_AWS_ENDPOINT:-}" ]; then endpoint=(--endpoint-url "$OFFHOST_AWS_ENDPOINT"); fi

# Newest local copies. Names carry a sortable UTC time, so the last one is the newest.
dump="$(ls -1 "$out_dir"/spectra-*.dump 2>/dev/null | sort | tail -n 1 || true)"
[ -n "$dump" ] || { echo "offhost: no database dump in $out_dir" >&2; exit 2; }
[ -f "$dump.sha256" ] || { echo "offhost: $dump has no .sha256 beside it" >&2; exit 2; }
snap="$(ls -1d "$out_dir"/artifacts/spectra-artifacts-*/ 2>/dev/null | sort | tail -n 1 || true)"
snap="${snap%/}"
[ -n "$snap" ] || { echo "offhost: no artifact snapshot in $out_dir/artifacts" >&2; exit 2; }
[ -f "$snap/MANIFEST.sha256" ] || { echo "offhost: $snap has no MANIFEST.sha256" >&2; exit 2; }

dump_name="$(basename "$dump")"
stamp="${dump_name#spectra-}"
stamp="${stamp%.dump}"
dest="${target%/}/$stamp"

echo "offhost: copying $dump_name and $(basename "$snap") to $dest"
aws "${endpoint[@]}" s3 cp "$dump" "$dest/db/$dump_name" --no-progress
aws "${endpoint[@]}" s3 cp "$dump.sha256" "$dest/db/$dump_name.sha256" --no-progress
aws "${endpoint[@]}" s3 sync "$snap" "$dest/artifacts" --no-progress

# 1. Database dump: download it back and compare checksums.
check_dir="$(mktemp -d)"
trap 'rm -rf "$check_dir"' EXIT
aws "${endpoint[@]}" s3 cp "$dest/db/$dump_name" "$check_dir/$dump_name" --no-progress >/dev/null
local_sum="$(sha256sum "$dump" | cut -d' ' -f1)"
remote_sum="$(sha256sum "$check_dir/$dump_name" | cut -d' ' -f1)"
if [ "$local_sum" != "$remote_sum" ]; then
  echo "offhost: FAILED: database dump checksum differs off-host" >&2
  exit 1
fi

# 2. Artifacts: the object count off-host must match the local file count.
local_count="$(find "$snap" -type f | wc -l)"
remote_count="$(aws "${endpoint[@]}" s3 ls "$dest/artifacts/" --recursive | grep -c . || true)"
if [ "$local_count" != "$remote_count" ]; then
  echo "offhost: FAILED: artifact objects off-host ($remote_count) do not match local files ($local_count)" >&2
  exit 1
fi

echo "offhost: verified $dest (database dump checksum ok; $remote_count artifact object(s))"
