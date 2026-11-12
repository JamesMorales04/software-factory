#!/usr/bin/env bash
# Compatibility entrypoint for the deterministic Compose acceptance journey.
#
# The implementation is the stage-addressed Python runner in scripts/e2e/.
# Existing callers keep working, and new callers can use:
#
#   scripts/compose-e2e.sh --from prepare --to gate --keep
#   cargo xtask e2e scenario --from gate --to summary --run-dir <run-dir>
#
# The runner never starts a paid provider session and never contacts GitHub.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/e2e/scenario.py" "$@"
