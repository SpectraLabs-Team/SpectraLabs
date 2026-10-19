#!/bin/bash
# Postgres backup script (M20b step 12: Deployment)
# Usage: ./scripts/backup-postgres.sh [database] [output_dir]
# Runs nightly: 0 2 * * * /path/to/backup-postgres.sh spectra /backups

set -e

DB_NAME="${1:-spectra}"
DB_USER="${2:-spectra}"
DB_HOST="${3:-127.0.0.1}"
DB_PORT="${4:-5432}"
OUTPUT_DIR="${5:-.}"
BACKUP_FILE="$OUTPUT_DIR/postgres-$DB_NAME-$(date +%Y-%m-%d_%H-%M-%S).sql.gz"

# Create backup directory if needed
mkdir -p "$OUTPUT_DIR"

# Run backup
echo "[$(date)] Starting backup of $DB_NAME..."
PGPASSWORD="${POSTGRES_PASSWORD:-}" pg_dump \
  -h "$DB_HOST" \
  -U "$DB_USER" \
  -d "$DB_NAME" \
  -F plain \
  | gzip > "$BACKUP_FILE"

echo "[$(date)] Backup complete: $BACKUP_FILE"

# Keep only last 7 days of backups (optional)
find "$OUTPUT_DIR" -name "postgres-$DB_NAME-*.sql.gz" -mtime +7 -delete

# Test restore once (optional: uncomment to test)
# echo "[$(date)] Testing restore..."
# gunzip -c "$BACKUP_FILE" | head -100 > /dev/null && echo "Backup verified"
