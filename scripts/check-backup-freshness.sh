#!/usr/bin/env bash
# Check that each store has a recent backup (M28 Task 4).
#
# Usage: scripts/check-backup-freshness.sh [OUTPUT_DIR] [MAX_AGE_HOURS]
#
#   OUTPUT_DIR     where backup-postgres.sh and backup-artifacts.sh write (default backups)
#   MAX_AGE_HOURS  a store whose newest backup is older than this is stale (default 26)
#
# Checks the database dumps (OUTPUT_DIR/spectra-*.dump) and the artifact snapshots
# (OUTPUT_DIR/artifacts/spectra-artifacts-*). It reads the UTC time in each backup's
# name, so copies that lose file times are still judged correctly.
#
# Exit codes: 0 both fresh; 1 a store is stale or has no backup; 2 bad arguments.
# Alert delivery is not part of this script. M27 wires the exit code to a
# notification; until then, run it from cron and check its output.
set -euo pipefail

out_dir="${1:-backups}"
max_hours="${2:-26}"
case "$max_hours" in
  '' | *[!0-9]*) echo "freshness: MAX_AGE_HOURS must be a whole number" >&2; exit 2 ;;
esac

now="$(date -u +%s)"
status=0

# newest_stamp PATHS...: print the newest UTC stamp (YYYYMMDDTHHMMSSZ) found in the
# given paths' names, or nothing if none match.
newest_stamp() {
  local path stamp newest=""
  for path in "$@"; do
    [ -e "$path" ] || continue
    stamp="$(basename "$path" | grep -oE '[0-9]{8}T[0-9]{6}Z' | tail -n 1 || true)"
    [ -n "$stamp" ] || continue
    if [ -z "$newest" ] || [[ "$stamp" > "$newest" ]]; then newest="$stamp"; fi
  done
  printf '%s' "$newest"
}

# check LABEL STAMP: report how old the newest backup is, and flag it if stale.
check() {
  local label="$1" stamp="$2" iso taken age_hours
  if [ -z "$stamp" ]; then
    echo "freshness: STALE $label: no backup found" >&2
    status=1
    return
  fi
  iso="${stamp:0:4}-${stamp:4:2}-${stamp:6:2} ${stamp:9:2}:${stamp:11:2}:${stamp:13:2}"
  taken="$(date -u -d "$iso" +%s)"
  age_hours=$(( (now - taken) / 3600 ))
  if [ "$age_hours" -gt "$max_hours" ]; then
    echo "freshness: STALE $label: newest backup is ${age_hours}h old (limit ${max_hours}h)" >&2
    status=1
  else
    echo "freshness: ok $label: newest backup is ${age_hours}h old"
  fi
}

shopt -s nullglob
db_stamp="$(newest_stamp "$out_dir"/spectra-*.dump)"
artifact_stamp="$(newest_stamp "$out_dir"/artifacts/spectra-artifacts-*/)"
shopt -u nullglob

check "database" "$db_stamp"
check "artifact store" "$artifact_stamp"
exit "$status"
