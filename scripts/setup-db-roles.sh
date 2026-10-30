#!/usr/bin/env bash
# Provision separate Postgres roles for migration owner, runtime service and backup runner (M20 hardening).
# Usage:
#   DATABASE_URL=postgres://spectra:secret@localhost:5432/spectra \
#   POSTGRES_APP_USER=spectra_app \
#   POSTGRES_APP_PASSWORD=runtime_secret \
#   POSTGRES_BACKUP_USER=spectra_backup \
#   POSTGRES_BACKUP_PASSWORD=backup_secret \
#   ./scripts/setup-db-roles.sh

set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL must be set}"
POSTGRES_APP_USER="${POSTGRES_APP_USER:-spectra_app}"
POSTGRES_BACKUP_USER="${POSTGRES_BACKUP_USER:-spectra_backup}"

echo "==> Provisioning application role '$POSTGRES_APP_USER' and backup role '$POSTGRES_BACKUP_USER'..."
cargo run -p spectra-db -- provision-roles

echo "==> Application service and backup roles successfully configured."
