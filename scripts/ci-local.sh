#!/usr/bin/env bash
# Run the checks CI runs, locally, before you push. The git hooks call this (see
# scripts/install-hooks.sh); run it by hand with `just ci` or `scripts/ci-local.sh`.
#
#   scripts/ci-local.sh            everything below that applies to what changed
#   scripts/ci-local.sh --quick    format + clippy only (the pre-commit hook)
#   scripts/ci-local.sh --all      every check, whatever changed
#
# Optional, when set:
#   DATABASE_URL       a Postgres you own: migrations, SQL checks, Postgres registry tests
#   TEST_DATABASE_URL  a Postgres for the dashboard's tests (npm test)
#
# The Rust toolchain comes from rust-toolchain.toml, as in CI. Exit code 1 on the first failure.
# CLIPPY_EXTRA is appended to every clippy call (e.g. "-A unknown-lints" on an older toolchain).
set -euo pipefail
cd "$(dirname "$0")/.."

mode="changed"
case "${1:-}" in
  --quick) mode="quick" ;;
  --all) mode="all" ;;
  "") ;;
  *) echo "usage: scripts/ci-local.sh [--quick|--all]" >&2; exit 2 ;;
esac

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
fail() { printf '\n\033[31mci-local: %s\033[0m\n' "$*" >&2; exit 1; }

# What changed relative to the branch's merge base with main (or everything, if unknown).
base=$(git merge-base HEAD origin/main 2>/dev/null || git merge-base HEAD main 2>/dev/null || echo "")
changed() {
  [ "$mode" = all ] && return 0
  [ -z "$base" ] && return 0
  { git diff --name-only "$base" -- "$@"; git diff --name-only --cached -- "$@"; git diff --name-only -- "$@"; } | grep -q .
}

rust_paths=(Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml crates examples parity scripts web/frontend/db/migrations)
web_paths=(web/frontend)

if changed "${rust_paths[@]}"; then
  step "rustfmt"
  cargo fmt --all --check || fail "formatting differs: run 'cargo fmt --all' and commit the result"

  step "clippy (default features)"
  cargo clippy --workspace --all-targets -- -D warnings ${CLIPPY_EXTRA:-}
  step "clippy (postgres, CLI, s3 features)"
  cargo clippy -p spectra-registry --features postgres --all-targets -- -D warnings ${CLIPPY_EXTRA:-}
  cargo clippy -p spectra-cli --features postgres --all-targets -- -D warnings ${CLIPPY_EXTRA:-}
  cargo clippy -p spectra-db -p spectra-api --features spectra-api/s3 --all-targets -- -D warnings ${CLIPPY_EXTRA:-}

  if [ "$mode" != quick ]; then
    step "standard-library-only engine crates"
    cargo metadata --format-version 1 --no-deps | python3 scripts/check_std_only.py

    step "tests"
    cargo test --workspace
    cargo test -p spectra-db --features s3 --lib

    step "generated API description is current"
    spec=$(mktemp)
    cargo run --quiet -p spectra-api --bin openapi-spec > "$spec"
    diff -q "$spec" web/frontend/lib/generated/engine-api.json >/dev/null \
      || fail "web/frontend/lib/generated is stale: run 'just api-client' (scripts/generate-typescript-client.sh) and commit it"

    if [ -n "${DATABASE_URL:-}" ]; then
      step "Postgres: migrations, registry tests, SQL checks"
      cargo run --quiet -p spectra-db -- migrate
      cargo test -p spectra-registry --features postgres --test pg_registry
      psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -f crates/spectra-db/tests/schema_checks.sql
      { node crates/spectra-db/scripts/prepare-queries.mjs; cat crates/spectra-db/tests/queue_checks.sql; } \
        | psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1
    else
      echo "(DATABASE_URL not set: skipping the Postgres checks; CI runs them)"
    fi
  fi
fi

if changed "${web_paths[@]}"; then
  if [ ! -d web/frontend/node_modules ]; then
    echo "(web/frontend/node_modules missing: run 'npm ci' there to include the dashboard checks)"
  else
    step "dashboard: typecheck and lint"
    (cd web/frontend && npm run --silent typecheck && npm run --silent lint)
    if [ "$mode" != quick ]; then
      if [ -n "${TEST_DATABASE_URL:-${DATABASE_URL:-}}" ]; then
        step "dashboard: tests"
        (cd web/frontend && TEST_DATABASE_URL="${TEST_DATABASE_URL:-$DATABASE_URL}" npm test)
      else
        echo "(TEST_DATABASE_URL not set: skipping the dashboard tests; CI runs them)"
      fi
    fi
  fi
fi

printf '\n\033[32mci-local: all checks passed (%s)\033[0m\n' "$mode"
