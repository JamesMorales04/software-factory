#!/usr/bin/env bash
# Legacy F1-S4-C live entrypoint. Current authorization is F1-S4-C-07:
# one Vlog Project, at most two paid Implementer sessions, one repair cycle.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec "$ROOT/scripts/e2e-s4c-07-live.sh" "$@"
