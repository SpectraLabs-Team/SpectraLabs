#!/usr/bin/env bash
# Regenerate the dashboard's TypeScript types for the engine API (M20 step 13).
#
# Usage: scripts/generate-typescript-client.sh          (or: just api-client)
#
# 1. Prints the OpenAPI description from the Rust code (no server needed) to
#    web/frontend/lib/generated/engine-api.json.
# 2. Generates web/frontend/lib/generated/engine-api.ts from it with
#    openapi-typescript (fetched by npx; nothing is added to package.json).
#
# Commit both files. web/frontend/lib/jobs.ts is the typed client the dashboard
# calls; its types mirror these and the generated file is the reference when
# the API changes. CI regenerates them and fails if the committed copies are stale.
set -euo pipefail
cd "$(dirname "$0")/.."
out=web/frontend/lib/generated
mkdir -p "$out"
cargo run --quiet -p spectra-api --bin openapi-spec >"$out/engine-api.json"
(cd web/frontend && npx --yes openapi-typescript@7.4.4 lib/generated/engine-api.json -o lib/generated/engine-api.ts)
echo "api-client: wrote $out/engine-api.json and $out/engine-api.ts"
