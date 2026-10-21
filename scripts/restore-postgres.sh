#!/usr/bin/env bash
# Restore a backup from scripts/backup-postgres.sh, and check it (M20 step 12).
#
# Usage: scripts/restore-postgres.sh BACKUP_FILE
#
#   RESTORE_DATABASE_URL  the database to restore INTO (required). Everything the
#                         backup contains is dropped and recreated there.
#   DATABASE_URL          if set, must differ from RESTORE_DATABASE_URL: this
#                         script refuses to restore over the live database.
#
# Verifies the .sha256 beside the backup (when present), restores, then checks
# that the schema and migrations are there and every workspace's registry hash
# chain is unbroken. Run it on a scratch database regularly: a backup that has
# never been restored is not a backup.
set -euo pipefail

file="${1:?usage: scripts/restore-postgres.sh BACKUP_FILE}"
: "${RESTORE_DATABASE_URL:?set RESTORE_DATABASE_URL to a scratch database to restore into}"
if [ -n "${DATABASE_URL:-}" ] && [ "$DATABASE_URL" = "$RESTORE_DATABASE_URL" ]; then
  echo "restore: RESTORE_DATABASE_URL is DATABASE_URL; refusing to overwrite the live database" >&2
  exit 2
fi
[ -f "$file" ] || { echo "restore: no such file: $file" >&2; exit 2; }

if [ -f "$file.sha256" ]; then
  (cd "$(dirname "$file")" && sha256sum --check --quiet "$(basename "$file").sha256")
  echo "restore: checksum ok"
fi

pg_restore --clean --if-exists --no-owner --no-privileges --exit-on-error \
  --dbname="$RESTORE_DATABASE_URL" "$file"
echo "restore: restored $file"

q() { psql "$RESTORE_DATABASE_URL" -X -A -t -v ON_ERROR_STOP=1 -c "$1"; }

for table in schema_migrations _sqlx_migrations users workspaces jobs runs run_artifacts registry_events workers; do
  if [ "$(q "SELECT to_regclass('public.$table') IS NOT NULL")" != "t" ]; then
    echo "restore: FAILED: table $table is missing" >&2
    exit 1
  fi
done
echo "restore: migrations applied: $(q "SELECT count(*) FROM _sqlx_migrations WHERE success")"
echo "restore: rows: users=$(q 'SELECT count(*) FROM users') workspaces=$(q 'SELECT count(*) FROM workspaces') jobs=$(q 'SELECT count(*) FROM jobs') runs=$(q 'SELECT count(*) FROM runs')"

broken="$(q "SELECT count(*) FROM registry_events e
             JOIN registry_events p ON p.workspace_id = e.workspace_id AND p.seq = e.seq - 1
             WHERE e.prev_hash <> p.hash")"
gaps="$(q "SELECT count(*) FROM (SELECT workspace_id, max(seq) - min(seq) + 1 - count(*) AS missing
                                  FROM registry_events GROUP BY workspace_id) g WHERE missing <> 0")"
if [ "$broken" != "0" ] || [ "$gaps" != "0" ]; then
  echo "restore: FAILED: registry chain broken ($broken bad links, $gaps workspaces with gaps)" >&2
  exit 1
fi
echo "restore: registry chains intact ($(q 'SELECT count(*) FROM registry_events') events)"
echo "restore: ok"
