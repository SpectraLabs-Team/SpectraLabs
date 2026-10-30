#!/usr/bin/env bash
# Check that each store has a recent backup (M28 Task 4).
#
# Usage: scripts/check-backup-freshness.sh [OUTPUT_DIR] [MAX_AGE_HOURS]
#
#   OUTPUT_DIR         where backup-postgres.sh and backup-artifacts.sh write (default backups)
#   MAX_AGE_HOURS      a store whose newest backup is older than this is stale (default 26)
#   FRESHNESS_TEXTFILE optional path for Prometheus textfile metrics. The file is replaced
#                      atomically each run. Point a node_exporter textfile collector at its
#                      directory to feed the alert rules in monitoring/prometheus/.
#
# Checks the database dumps (OUTPUT_DIR/spectra-*.dump) and the artifact snapshots
# (OUTPUT_DIR/artifacts/spectra-artifacts-*). It reads the UTC time in each backup's
# name, so copies that lose file times are still judged correctly.
#
# Exit codes: 0 both fresh; 1 a store is stale or has no backup; 2 bad arguments.
# Alert delivery is not part of this script. The rule file makes the alert; M27 chooses
# where it is sent.
set -euo pipefail

out_dir="${1:-backups}"
max_hours="${2:-26}"
case "$max_hours" in
  '' | *[!0-9]*) echo "freshness: MAX_AGE_HOURS must be a whole number" >&2; exit 2 ;;
esac

now="$(date -u +%s)"
status=0
metrics=""

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

# check STORE LABEL STAMP: report how old the newest backup is, flag it if stale,
# and record metrics for the store.
check() {
  local store="$1" label="$2" stamp="$3" iso taken age_secs stale=0
  if [ -z "$stamp" ]; then
    echo "freshness: STALE $label: no backup found" >&2
    status=1
    metrics+="spectra_backup_stale{store=\"$store\"} 1"$'\n'
    return
  fi
  iso="${stamp:0:4}-${stamp:4:2}-${stamp:6:2} ${stamp:9:2}:${stamp:11:2}:${stamp:13:2}"
  taken="$(date -u -d "$iso" +%s)"
  age_secs=$(( now - taken ))
  if [ $(( age_secs / 3600 )) -gt "$max_hours" ]; then
    echo "freshness: STALE $label: newest backup is $(( age_secs / 3600 ))h old (limit ${max_hours}h)" >&2
    status=1
    stale=1
  else
    echo "freshness: ok $label: newest backup is $(( age_secs / 3600 ))h old"
  fi
  metrics+="spectra_backup_age_seconds{store=\"$store\"} $age_secs"$'\n'
  metrics+="spectra_backup_stale{store=\"$store\"} $stale"$'\n'
}

shopt -s nullglob
db_stamp="$(newest_stamp "$out_dir"/spectra-*.dump)"
artifact_stamp="$(newest_stamp "$out_dir"/artifacts/spectra-artifacts-*/)"
shopt -u nullglob

check database "database" "$db_stamp"
check artifacts "artifact store" "$artifact_stamp"

# Prometheus textfile output. Written to a temp file and renamed, so a scrape never
# reads a half-written file.
if [ -n "${FRESHNESS_TEXTFILE:-}" ]; then
  tmp="$FRESHNESS_TEXTFILE.$$.tmp"
  {
    echo "# HELP spectra_backup_age_seconds Age of the newest backup per store."
    echo "# TYPE spectra_backup_age_seconds gauge"
    echo "# HELP spectra_backup_stale 1 when a store has no recent backup, else 0."
    echo "# TYPE spectra_backup_stale gauge"
    printf '%s' "$metrics"
    echo "# HELP spectra_backup_freshness_check_timestamp_seconds When the freshness check last ran."
    echo "# TYPE spectra_backup_freshness_check_timestamp_seconds gauge"
    echo "spectra_backup_freshness_check_timestamp_seconds $now"
  } >"$tmp"
  mv "$tmp" "$FRESHNESS_TEXTFILE"
fi

exit "$status"
