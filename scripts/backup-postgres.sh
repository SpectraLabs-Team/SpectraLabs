#!/usr/bin/env bash
# Back up the Spectra Postgres database (M20 step 12).
#
# Usage: scripts/backup-postgres.sh [OUTPUT_DIR]          (default OUTPUT_DIR: backups)
#
#   DATABASE_URL      the database to back up (required)
#   BACKUP_KEEP_DAYS  delete this script's backups older than this many days
#                     (default 7; 0 keeps everything)
#
# Writes OUTPUT_DIR/spectra-<UTC time>.dump (pg_dump custom format, compressed)
# and a .sha256 beside it. The dump only gets its final name once pg_restore
# can read it back, so a half-written file never looks like a backup.
# Restore-test backups regularly with scripts/restore-postgres.sh.
#
# Nightly from cron:
#   0 2 * * * cd /srv/spectralabs && DATABASE_URL=postgres://... scripts/backup-postgres.sh /var/backups/spectra
set -euo pipefail

out_dir="${1:-backups}"
: "${DATABASE_URL:?set DATABASE_URL to the database to back up}"
keep_days="${BACKUP_KEEP_DAYS:-7}"
case "$keep_days" in
  '' | *[!0-9]*) echo "backup: BACKUP_KEEP_DAYS must be a whole number" >&2; exit 2 ;;
esac

mkdir -p "$out_dir"
name="spectra-$(date -u +%Y%m%dT%H%M%SZ).dump"
tmp="$out_dir/.$name.partial"
trap 'rm -f "$tmp"' EXIT

pg_dump --format=custom --compress=6 --no-owner --no-privileges --file="$tmp" "$DATABASE_URL"
pg_restore --list "$tmp" >/dev/null # a readable archive, not a truncated file
mv "$tmp" "$out_dir/$name"
(cd "$out_dir" && sha256sum "$name" >"$name.sha256")
echo "backup: wrote $out_dir/$name ($(du -h "$out_dir/$name" | cut -f1))"

if [ "$keep_days" -gt 0 ]; then
  find "$out_dir" -maxdepth 1 -type f -name 'spectra-*.dump*' -mtime +"$keep_days" -print -delete |
    sed 's/^/backup: removed old /'
fi
