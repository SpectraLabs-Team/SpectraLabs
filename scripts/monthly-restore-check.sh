#!/usr/bin/env bash
# Monthly restore test (M28 Task 6): restore the newest database backup into a
# scratch database with scripts/restore-postgres.sh, and keep a dated log.
# Schedule it monthly from cron or the ops runner; this script runs one check.
#
# Usage: scripts/monthly-restore-check.sh [OUTPUT_DIR]     (default OUTPUT_DIR: backups)
#
#   RESTORE_DATABASE_URL  the scratch database to restore into (required)
#   DATABASE_URL          the live database, if set: must differ from RESTORE_DATABASE_URL
#
# Writes OUTPUT_DIR/restore-checks/restore-<UTC time>.log and prints one result
# line: "monthly-restore: PASS <backup>" or "monthly-restore: FAIL <backup>".
# Two passes in a row is the M28 bar for the restore test. Exits with the
# restore script's exit code.
set -euo pipefail

out_dir="${1:-backups}"
: "${RESTORE_DATABASE_URL:?set RESTORE_DATABASE_URL to a scratch database to restore into}"
if [ -n "${DATABASE_URL:-}" ] && [ "$DATABASE_URL" = "$RESTORE_DATABASE_URL" ]; then
  echo "monthly-restore: RESTORE_DATABASE_URL is DATABASE_URL; refusing to restore over the live database" >&2
  exit 2
fi

# Backup names carry a sortable UTC time, so the last one is the newest.
# Only top-level dumps: weekly/ copies are not picked.
newest="$(ls -1 "$out_dir"/spectra-*.dump 2>/dev/null | sort | tail -n 1 || true)"
if [ -z "$newest" ]; then
  echo "monthly-restore: no database backup found in $out_dir" >&2
  exit 2
fi

log_dir="$out_dir/restore-checks"
mkdir -p "$log_dir"
log="$log_dir/restore-$(date -u +%Y%m%dT%H%M%SZ).log"

here="$(cd "$(dirname "$0")" && pwd)"
set +e
"$here/restore-postgres.sh" "$newest" >"$log" 2>&1
rc=$?
set -e

if [ "$rc" -eq 0 ]; then
  echo "monthly-restore: PASS $newest (log: $log)"
else
  echo "monthly-restore: FAIL $newest (exit $rc, log: $log)"
fi
exit "$rc"
