#!/usr/bin/env bash
# Provision separate Postgres roles for migration owner vs. runtime service (M20 hardening).
# Usage:
#   DATABASE_URL=postgres://spectra:secret@localhost:5432/spectra \
#   POSTGRES_APP_USER=spectra_app \
#   POSTGRES_APP_PASSWORD=runtime_secret \
#   ./scripts/setup-db-roles.sh

set -euo pipefail

: "${DATABASE_URL:?DATABASE_URL must be set}"
POSTGRES_APP_USER="${POSTGRES_APP_USER:-spectra_app}"

echo "==> Provisioning application role '$POSTGRES_APP_USER'..."
cargo run -p spectra-db -- provision-roles

echo "==> Application service role successfully configured."
