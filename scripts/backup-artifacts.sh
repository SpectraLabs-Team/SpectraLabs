#!/usr/bin/env bash
# Back up the artifact store (M28 Task 3): copy the S3 bucket into a dated
# snapshot directory, check it against a SHA-256 manifest, and prune old snapshots.
#
# Usage: scripts/backup-artifacts.sh [OUTPUT_DIR]          (default OUTPUT_DIR: backups)
#
#   ARTIFACT_S3_BUCKET       the artifact bucket (required)
#   ARTIFACT_S3_PREFIX       back up only keys under this prefix (optional)
#   AWS_ENDPOINT             S3 endpoint URL for RustFS, R2 or another provider (optional)
#   AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_REGION   AWS CLI credentials
#   BACKUP_KEEP_DAYS         delete snapshots older than this many days
#                            (default 14; 0 keeps everything)
#   BACKUP_KEEP_WEEKLY_DAYS  keep Sunday (UTC) snapshots for this many days
#                            (default 90; 0 keeps everything)
#
# Writes OUTPUT_DIR/artifacts/spectra-artifacts-<UTC time>/ with MANIFEST.sha256
# inside. Needs the AWS CLI. Every run copies the whole prefix, so disk use
# grows with the store. A snapshot only gets its final name once it verifies.
set -euo pipefail

out_dir="${1:-backups}"
: "${ARTIFACT_S3_BUCKET:?set ARTIFACT_S3_BUCKET to the artifact bucket}"
prefix="${ARTIFACT_S3_PREFIX:-}"
keep_days="${BACKUP_KEEP_DAYS:-14}"
keep_weekly_days="${BACKUP_KEEP_WEEKLY_DAYS:-90}"
for v in "$keep_days" "$keep_weekly_days"; do
  case "$v" in
    '' | *[!0-9]*) echo "backup: keep periods must be whole numbers of days" >&2; exit 2 ;;
  esac
done
command -v aws >/dev/null || { echo "backup: the AWS CLI is required" >&2; exit 2; }

endpoint=()
if [ -n "${AWS_ENDPOINT:-}" ]; then endpoint=(--endpoint-url "$AWS_ENDPOINT"); fi

snap_root="$out_dir/artifacts"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
name="spectra-artifacts-$stamp"
tmp="$snap_root/.$name.partial"
mkdir -p "$snap_root"
trap 'rm -rf "$tmp"' EXIT

aws "${endpoint[@]}" s3 sync "s3://$ARTIFACT_S3_BUCKET/$prefix" "$tmp" --no-progress
(cd "$tmp" && find . -type f ! -name MANIFEST.sha256 -print0 | sort -z | xargs -0 -r sha256sum >MANIFEST.sha256)
(cd "$tmp" && sha256sum --check --quiet MANIFEST.sha256)
files="$(find "$tmp" -type f ! -name MANIFEST.sha256 | wc -l)"
mv "$tmp" "$snap_root/$name"
echo "backup: wrote $snap_root/$name ($files file(s), $(du -sh "$snap_root/$name" | cut -f1))"

# Prune by the UTC time in each snapshot's name. Sunday snapshots follow the
# weekly period; the others follow the daily period.
now="$(date -u +%s)"
for dir in "$snap_root"/spectra-artifacts-*/; do
  [ -d "$dir" ] || continue
  base="${dir%/}"
  base="${base##*/}"
  t="${base#spectra-artifacts-}"
  iso="${t:0:4}-${t:4:2}-${t:6:2} ${t:9:2}:${t:11:2}:${t:13:2}"
  if date -u -d "$iso" +%s >/dev/null 2>&1; then
    taken="$(date -u -d "$iso" +%s)"
    dow="$(date -u -d "$iso" +%u)"
  else
    taken="$(date -u -j -f "%Y-%m-%d %H:%M:%S" "$iso" +%s)"
    dow="$(date -u -j -f "%Y-%m-%d %H:%M:%S" "$iso" +%u)"
  fi
  age_days=$(( (now - taken) / 86400 ))
  if [ "$dow" = 7 ]; then limit="$keep_weekly_days"; else limit="$keep_days"; fi
  if [ "$limit" -gt 0 ] && [ "$age_days" -gt "$limit" ]; then
    rm -rf "$dir"
    echo "backup: removed old snapshot $base"
  fi
done
