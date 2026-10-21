#!/usr/bin/env bash
# F1-S4-C-07 bounded real-Paseo Vlog certification entrypoint.
# One Project, at most two paid Implementer sessions and one corrective cycle.
# Product Owner opt-in only; never merges or delivers.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/e2e_s4c_07_live.py" "$@"
