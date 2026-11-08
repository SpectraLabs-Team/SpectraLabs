#!/usr/bin/env bash
# Anchor the registry's hash chains outside the database (M20 step 12, M5).
#
# Usage:
#   scripts/anchor-registry-hash.sh [ANCHOR_DIR]                       (one-shot anchor)
#   scripts/anchor-registry-hash.sh --schedule [INTERVAL] [ANCHOR_DIR] (scheduled daemon mode)
#
#   DATABASE_URL  the database (required)
#
# Each workspace's registry is a hash chain (registry_events.prev_hash -> hash),
# so its latest hash commits to its whole history. This script records every
# workspace's head (workspace, seq, hash) in ANCHOR_DIR/registry-heads-<UTC time>.txt
# and appends "<UTC time> <sha256 of that file> <workspace count>" to
# ANCHOR_DIR/anchors.log.
#
# The anchor only helps if it is kept where whoever controls the database cannot
# rewrite it: commit ANCHOR_DIR to a separate git repository, upload it to a
# bucket with object lock, or attach it to a release. Set ANCHOR_UPLOAD to a
# command to run with the heads file as its argument, for example
#   ANCHOR_UPLOAD='aws s3 cp --sse AES256 "$0" s3://spectra-anchors/'
# Daily from cron:
#   0 3 * * * cd /srv/spectralabs && DATABASE_URL=postgres://... scripts/anchor-registry-hash.sh /var/lib/spectra/anchors
set -euo pipefail

compute_digest() {
  local target="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$target" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$target" | cut -d' ' -f1
  else
    openssl dgst -sha256 "$target" | awk '{print $NF}'
  fi
}

anchor_once() {
  local dir="$1"
  : "${DATABASE_URL:?set DATABASE_URL}"
  mkdir -p "$dir"
  local stamp
  stamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  local heads="$dir/registry-heads-$(date -u +%Y%m%dT%H%M%SZ).txt"

  psql "$DATABASE_URL" -X -A -t -F ' ' -v ON_ERROR_STOP=1 -c \
    "SELECT DISTINCT ON (workspace_id) workspace_id, seq, hash
       FROM registry_events
      ORDER BY workspace_id, seq DESC" >"$heads"

  local count
  count="$(grep -c . "$heads" || true)"
  local digest
  digest="$(compute_digest "$heads")"
  echo "$stamp $digest $count" >>"$dir/anchors.log"
  echo "anchor: $count workspace head(s) in $heads, digest $digest"

  if [ -n "${ANCHOR_UPLOAD:-}" ]; then
    sh -c "$ANCHOR_UPLOAD" "$heads"
    echo "anchor: uploaded"
  fi
}

if [ "${1:-}" = "--schedule" ] || [ "${1:-}" = "--daemon" ]; then
  interval="${2:-3600}" # default 1 hour
  dir="${3:-anchors}"
  echo "anchor-daemon: starting scheduled registry anchoring every ${interval}s into $dir"
  while true; do
    anchor_once "$dir" || echo "anchor-daemon: warning: anchoring attempt failed at $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    sleep "$interval"
  done
else
  dir="${1:-anchors}"
  anchor_once "$dir"
fi
