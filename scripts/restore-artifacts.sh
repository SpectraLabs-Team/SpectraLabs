#!/usr/bin/env bash
# Restore an artifact-store snapshot into scratch storage and check a sample of
# files (M28 Task 7). Pair with scripts/backup-artifacts.sh.
#
# Usage: scripts/restore-artifacts.sh SNAPSHOT_DIR RESTORE_TARGET [SAMPLE_SIZE]
#
#   SNAPSHOT_DIR    a dated snapshot from backup-artifacts.sh (has MANIFEST.sha256)
#   RESTORE_TARGET  an empty scratch directory, or s3://bucket/prefix on a scratch bucket
#   SAMPLE_SIZE     files to re-check after the restore (default 20)
#
#   ARTIFACT_S3_BUCKET, ARTIFACT_S3_PREFIX   the live bucket. The restore refuses to write there.
#   ARTIFACT_DIR                             the live local store. The restore refuses to write there.
#   AWS_ENDPOINT                             S3 endpoint for RustFS, R2 or another provider (optional)
#
# Steps: check the whole snapshot against its manifest, refuse live targets, copy the
# snapshot to the target, then re-check a random sample of the restored files against
# the manifest. Exits non-zero on any mismatch.
set -euo pipefail

usage="usage: restore-artifacts.sh SNAPSHOT_DIR RESTORE_TARGET [SAMPLE_SIZE]"
snap="${1:?$usage}"
target="${2:?$usage}"
sample_size="${3:-20}"
case "$sample_size" in
  '' | *[!0-9]*) echo "restore: SAMPLE_SIZE must be a whole number" >&2; exit 2 ;;
esac
[ "$sample_size" -gt 0 ] || { echo "restore: SAMPLE_SIZE must be at least 1" >&2; exit 2; }
[ -f "$snap/MANIFEST.sha256" ] || { echo "restore: no MANIFEST.sha256 in $snap" >&2; exit 2; }

endpoint=()
if [ -n "${AWS_ENDPOINT:-}" ]; then endpoint=(--endpoint-url "$AWS_ENDPOINT"); fi

# 1. The snapshot itself must verify before anything is copied from it.
(cd "$snap" && sha256sum --check --quiet MANIFEST.sha256)
echo "restore: snapshot verifies against its manifest"

# 2. Refuse to write into the live store.
if [ -n "${ARTIFACT_S3_BUCKET:-}" ]; then
  live_s3="s3://$ARTIFACT_S3_BUCKET/${ARTIFACT_S3_PREFIX:-}"
  live_s3="${live_s3%/}"
  case "$target" in
    s3://*) norm_target="${target%/}"
      if [ "$norm_target" = "$live_s3" ] || [ "$norm_target" = "$live_s3/" ]; then
        echo "restore: refusing to restore into the live bucket $live_s3" >&2; exit 2
      fi ;;
  esac
fi
if [ -n "${ARTIFACT_DIR:-}" ]; then
  live_dir="$(realpath -m "$ARTIFACT_DIR")"
  case "$target" in
    s3://*) ;;
    *) t="$(realpath -m "$target")"
       case "$t/" in
         "$live_dir/"*) echo "restore: refusing to restore into the live store $live_dir" >&2; exit 2 ;;
       esac ;;
  esac
fi

# 3. The target must be empty, so nothing is overwritten by accident.
case "$target" in
  s3://*)
    if [ -n "$(aws "${endpoint[@]}" s3 ls "${target%/}/" 2>/dev/null | head -n 1)" ]; then
      echo "restore: target $target is not empty" >&2; exit 2
    fi
    aws "${endpoint[@]}" s3 sync "$snap" "$target" --no-progress ;;
  *)
    if [ -d "$target" ] && [ -n "$(ls -A "$target")" ]; then
      echo "restore: target $target is not empty" >&2; exit 2
    fi
    mkdir -p "$target"
    cp -a "$snap"/. "$target"/ ;;
esac
echo "restore: copied snapshot to $target"

# 4. Re-check a random sample of the restored files against the manifest.
sample="$(mktemp)"
trap 'rm -f "$sample"' EXIT
total="$(wc -l <"$snap/MANIFEST.sha256")"
if [ "$total" -eq 0 ]; then echo "restore: manifest lists no files" >&2; exit 1; fi
shuf -n "$sample_size" "$snap/MANIFEST.sha256" >"$sample"
checked="$(wc -l <"$sample")"

case "$target" in
  s3://*)
    base="${target%/}"
    while read -r hash path; do
      rel="${path#./}"
      tmp_file="$(mktemp)"
      aws "${endpoint[@]}" s3 cp "$base/$rel" "$tmp_file" --no-progress >/dev/null
      got="$(sha256sum "$tmp_file" | cut -d' ' -f1)"
      rm -f "$tmp_file"
      if [ "$got" != "$hash" ]; then echo "restore: sample mismatch for $rel" >&2; exit 1; fi
    done <"$sample" ;;
  *)
    (cd "$target" && sha256sum --check --quiet "$sample") ;;
esac

echo "restore: sample of $checked of $total file(s) matches the manifest"
