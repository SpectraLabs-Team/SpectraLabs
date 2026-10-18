#!/usr/bin/env bash
# Regenerate the risk-engine golden file from the C++ risk engine (PCPROJECT).
# Usage: parity/regen_risk.sh <risk-engine source dir> <its CMake Release build dir>
# The build dir must contain libquantforge_core.a and generated/ (cmake -DCMAKE_BUILD_TYPE=Release).
set -euo pipefail
SRC="${1:?risk-engine source dir}"
BUILD="${2:?risk-engine build dir}"
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
g++ -std=gnu++20 -O3 -I"$SRC/include" -I"$SRC/src" -I"$SRC" -I"$BUILD/generated" "$HERE/cpp/risk_golden.cpp" \
  -Wl,--whole-archive "$BUILD/libquantforge_core.a" -Wl,--no-whole-archive -o "$TMP/risk_golden"
mkdir -p "$HERE/golden/risk"
"$TMP/risk_golden" "$TMP/work" > "$HERE/golden/risk/risk.golden"
echo "regenerated $HERE/golden/risk/risk.golden"
