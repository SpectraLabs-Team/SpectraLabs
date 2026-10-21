#!/usr/bin/env bash
# Use the repository's git hooks (.githooks): formatting on commit, the CI checks on push.
# Runs automatically on `npm ci` / `npm install` in web/frontend; run it by hand otherwise.
set -euo pipefail
cd "$(dirname "$0")/.."
git config core.hooksPath .githooks
chmod +x .githooks/* scripts/ci-local.sh 2>/dev/null || true
echo "git hooks installed (core.hooksPath=.githooks): pre-commit checks formatting, pre-push runs scripts/ci-local.sh"
