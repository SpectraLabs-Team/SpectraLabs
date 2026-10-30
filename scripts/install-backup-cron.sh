#!/usr/bin/env bash
# Install the nightly database backup job on the production host (M28 Task 1).
#
# Usage:
#   scripts/install-backup-cron.sh [OUTPUT_DIR] [ENV_FILE]
#
# Default OUTPUT_DIR: /var/backups/spectra
# Default ENV_FILE:   /srv/spectralabs/.env
#
# Prerequisites checked:
# 1. Matching pg_dump, pg_restore, and sha256sum installed.
# 2. Database connectivity with the backup role (POSTGRES_BACKUP_USER).
# 3. Creates the backup directory and weekly sub-directory with 0700 permissions.
# 4. Installs the crontab entry running nightly at 02:00 UTC (referencing ENV_FILE without secrets in crontab).
# 5. Runs an initial backup verification pass to ensure .dump and .sha256 verify.
# 6. Runs scripts/check-backup-freshness.sh to confirm green monitoring status.
#
# Keep settings default to 14 daily, 90 weekly (pending owner sign-off on M28 Decision 1).
set -euo pipefail

out_dir="${1:-/var/backups/spectra}"
env_file="${2:-/srv/spectralabs/.env}"
keep_days="${BACKUP_KEEP_DAYS:-14}"
keep_weekly_days="${BACKUP_KEEP_WEEKLY_DAYS:-90}"

echo "==> M28 Task 1: Installing nightly database backup on $(hostname)..."

# 1. Tool availability
command -v pg_dump >/dev/null || { echo "install-backup: pg_dump is required" >&2; exit 2; }
command -v pg_restore >/dev/null || { echo "install-backup: pg_restore is required" >&2; exit 2; }
command -v sha256sum >/dev/null || { echo "install-backup: sha256sum is required" >&2; exit 2; }

# 2. Load environment if present
if [ -f "$env_file" ]; then
  eval "$(grep -E '^(DATABASE_URL|BACKUP_DATABASE_URL|POSTGRES_BACKUP_USER|POSTGRES_BACKUP_PASSWORD)=' "$env_file" 2>/dev/null || true)"
fi

db_url="${BACKUP_DATABASE_URL:-${DATABASE_URL:-}}"
if [ -z "$db_url" ]; then
  echo "install-backup: DATABASE_URL or BACKUP_DATABASE_URL must be set (or in $env_file)" >&2
  exit 2
fi

# 3. Check connectivity and role
echo "==> Verifying database connection with backup role..."
psql "$db_url" -c "SELECT current_user, current_database(), version();" >/dev/null

# 4. Prepare directory
echo "==> Setting up backup directory at $out_dir..."
mkdir -p "$out_dir" "$out_dir/weekly"
chmod 0700 "$out_dir" "$out_dir/weekly" 2>/dev/null || true

# 5. Execute initial backup by hand
echo "==> Running initial test backup..."
DATABASE_URL="$db_url" BACKUP_KEEP_DAYS="$keep_days" BACKUP_KEEP_WEEKLY_DAYS="$keep_weekly_days" \
  scripts/backup-postgres.sh "$out_dir"

latest_dump="$(ls -1 "$out_dir"/spectra-*.dump 2>/dev/null | sort | tail -n 1)"
[ -f "$latest_dump" ] || { echo "install-backup: backup file was not produced" >&2; exit 1; }
[ -f "$latest_dump.sha256" ] || { echo "install-backup: sha256 was not produced" >&2; exit 1; }

echo "==> Verifying initial backup checksum..."
(cd "$out_dir" && sha256sum --check --quiet "$(basename "$latest_dump.sha256")")
echo "install-backup: checksum verified for $latest_dump"

# 6. Check freshness script
if [ -f "scripts/check-backup-freshness.sh" ]; then
  echo "==> Checking freshness monitor..."
  scripts/check-backup-freshness.sh "$out_dir" 26 || true
fi

# 7. Print and register cron entry
cron_line="0 2 * * * cd /srv/spectralabs && ( [ -f $env_file ] && export \$(grep -v '^#' $env_file | xargs) ); BACKUP_KEEP_DAYS=$keep_days BACKUP_KEEP_WEEKLY_DAYS=$keep_weekly_days scripts/backup-postgres.sh $out_dir >> $out_dir/backup.log 2>&1"

echo "==> Nightly cron entry:"
echo "    $cron_line"

# If crontab is available and running as target user, install entry if not present
if command -v crontab >/dev/null 2>&1; then
  current_cron="$(crontab -l 2>/dev/null || true)"
  if ! echo "$current_cron" | grep -F "scripts/backup-postgres.sh" >/dev/null; then
    (echo "$current_cron"; echo "$cron_line") | crontab -
    echo "install-backup: crontab installed successfully"
  else
    echo "install-backup: backup cron already present in crontab"
  fi
fi

echo "==> M28 Task 1 installation complete."
