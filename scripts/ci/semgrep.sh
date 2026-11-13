#!/usr/bin/env bash
# Narrow security rules for first-party code. The host/CI image must provide
# semgrep; absence is BLOCKED, never a silent pass.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
if ! command -v semgrep >/dev/null 2>&1; then
  printf 'BLOCKED: semgrep is not installed\n' >&2
  exit 2
fi
semgrep --config .semgrep.yml --error --quiet
