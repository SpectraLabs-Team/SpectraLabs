#!/usr/bin/env bash
# Regenerate every C++ golden file from the frozen reference engines.
# Usage: parity/regen.sh <quantforge C++ v0.6.0 checkout> [<risk-engine source dir> <risk-engine build dir>]
# The research engine must already be built (cmake --preset release && cmake --build --preset release).
# The optional arguments point at the C++ risk engine (PCPROJECT) and its Release build; without
# them the risk golden is left as it is.
set -euo pipefail
REF="${1:?path to the C++ reference checkout}"
RISK_SRC="${2:-}"
RISK_BUILD="${3:-}"
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
LIB="$REF/build/release/libqf_core.a"
build() { g++ -std=c++20 -O2 -I"$REF/include" "$HERE/cpp/$1.cpp" "$LIB" -o "$TMP/$1"; }

python3 "$HERE/gen_ops.py" "$HERE/ops"
python3 "$HERE/gen_data.py" "$HERE/data"
for g in ledger_golden data_golden backtest_golden research_golden registry_golden; do build "$g"; done

for f in "$HERE"/ops/*.ops; do
  "$TMP/ledger_golden" "$f" > "$HERE/golden/$(basename "$f" .ops).golden"
done
mkdir -p "$HERE/golden/data"
"$TMP/data_golden" "$HERE/data" "$HERE/golden/data"
"$TMP/backtest_golden" > "$HERE/golden/backtest.golden"
"$TMP/research_golden" > "$HERE/golden/research.golden"
"$TMP/registry_golden" "$TMP/registry_work" > "$HERE/golden/registry.golden"
cp "$TMP/registry_work/registry/registry.jsonl" "$HERE/golden/registry.jsonl"

if [[ -n "$RISK_SRC" ]]; then
  "$HERE/regen_risk.sh" "$RISK_SRC" "${RISK_BUILD:?risk-engine build dir}"
fi
echo "regenerated golden files in $HERE/golden"
