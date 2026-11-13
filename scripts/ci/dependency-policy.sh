#!/usr/bin/env bash
# Dependency policy: licenses/bans/sources only. Advisories remain owned by
# the unsuppressed cargo-audit gate; RUSTSEC-2026-0459 is resolved by the
# vendored sqlx-sqlite manifest patch selecting libsqlite3-sys 0.39.1.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if ! command -v cargo-deny >/dev/null 2>&1; then
  printf 'BLOCKED: cargo-deny is not installed\n' >&2
  exit 2
fi
cargo deny check licenses bans sources
