#!/bin/bash
# Generate TypeScript API client from OpenAPI spec (M20b step 13)
# Prerequisites: npm install -g @openapitools/openapi-generator-cli
# Usage: ./scripts/generate-typescript-client.sh

set -e

SPEC_URL="${1:-http://localhost:8000/api/v1/openapi.json}"
OUTPUT_DIR="${2:-./web/frontend/src/lib/api-client}"

echo "[$(date)] Generating TypeScript client from $SPEC_URL..."

# Check if openapi-generator is installed
if ! command -v openapi-generator-cli &> /dev/null; then
  echo "Installing OpenAPI Generator..."
  npm install -g @openapitools/openapi-generator-cli
fi

# Generate client
openapi-generator-cli generate \
  -i "$SPEC_URL" \
  -g typescript-fetch \
  -o "$OUTPUT_DIR" \
  --package-name spectra-api-client \
  --additional-properties=npmVersion=8 \
  --skip-validate-spec

echo "[$(date)] Client generated to $OUTPUT_DIR"
echo "Next: npm install spectra-api-client in web/frontend"
