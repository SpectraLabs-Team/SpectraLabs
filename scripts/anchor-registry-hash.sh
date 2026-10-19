#!/bin/bash
# Anchor registry head hash (M20b step 12: Deployment)
# Writes the current registry head hash to a trusted location (e.g., GitHub releases)
# Usage: ./scripts/anchor-registry-hash.sh
# Runs daily: 0 3 * * * /path/to/anchor-registry-hash.sh

set -e

# Database connection
DB_HOST="${DB_HOST:-127.0.0.1}"
DB_PORT="${DB_PORT:-5432}"
DB_NAME="${DB_NAME:-spectra}"
DB_USER="${DB_USER:-spectra}"

# Output file
ANCHOR_FILE="${ANCHOR_FILE:-.registry-head-hash.txt}"

# Get the current registry head hash
REGISTRY_HASH=$(PGPASSWORD="${POSTGRES_PASSWORD:-}" psql \
  -h "$DB_HOST" \
  -U "$DB_USER" \
  -d "$DB_NAME" \
  -t -c "SELECT hash FROM registry_events ORDER BY seq DESC LIMIT 1;" 2>/dev/null || echo "")

if [ -z "$REGISTRY_HASH" ]; then
  echo "[$(date)] Warning: Could not retrieve registry hash from database"
  exit 1
fi

echo "[$(date)] Registry head hash: $REGISTRY_HASH"

# Write to local file
echo "$REGISTRY_HASH" > "$ANCHOR_FILE"
echo "[$(date)] Anchored to $ANCHOR_FILE"

# Optional: Write to GitHub release or other trusted location
# Example: upload to GitHub:
# gh release create "registry-anchor-$(date +%Y-%m-%d)" "$ANCHOR_FILE" \
#   --title "Registry Head Hash Anchor - $(date +%Y-%m-%d)" \
#   --notes "Registry head hash: $REGISTRY_HASH"

# Optional: Upload to S3
# aws s3 cp "$ANCHOR_FILE" "s3://my-bucket/registry-anchors/$(date +%Y-%m-%d).txt" --sse AES256
