#!/usr/bin/env bash
# PR-scoped mutation sample. A missing tool is BLOCKED; an empty diff is a
# no-op, not a mutation pass.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
BASE="${1:-origin/main}"
if ! command -v cargo-mutants >/dev/null 2>&1; then
  printf 'BLOCKED: cargo-mutants is not installed\n' >&2
  exit 2
fi
DIFF="$(mktemp)"
trap 'rm -f "$DIFF"' EXIT
git diff --no-color --find-renames "$BASE"...HEAD >"$DIFF"
if [[ ! -s "$DIFF" ]]; then
  printf 'no diff against %s; mutation sample skipped\n' "$BASE"
  exit 0
fi
cargo mutants --in-diff "$DIFF" --package factory-native
