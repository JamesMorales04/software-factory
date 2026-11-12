#!/usr/bin/env bash
# Local/CI quality lane for the stage-addressed harness and the Office typed
# lint/browser checks. It does not start Compose, a model or GitHub.
#
# Environment:
#   QUALITY_HARNESS_SKIP_BROWSER=1  skip the real-Chromium component lane
#   CHROMIUM_EXE=/path/to/chrome     browser executable for the component lane
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

log() { printf '\n== %s ==\n' "$*"; }

log "Python scenario-runner unit tests"
python3 -m unittest discover -s scripts/e2e/tests -v

log "Scenario runner interface smoke"
python3 scripts/e2e/scenario.py --list >/dev/null
python3 scripts/e2e/scenario.py --help >/dev/null
python3 scripts/e2e/cassette.py scripts/fixtures/provider-cassette.example.json >/dev/null

if [[ ! -d office/node_modules ]]; then
  printf 'office/node_modules is missing; run npm ci in office first\n' >&2
  exit 2
fi

cd office
log "Office typecheck and production build"
npm run typecheck
npm run build

log "Office Biome and typed lint"
npm run lint
npm run lint:typed

log "Office dead-code/dependency check"
npm run knip

log "Office jsdom tests"
npm test -- --run

if [[ "${QUALITY_HARNESS_SKIP_BROWSER:-0}" != "1" ]]; then
  log "Office real-browser component tests"
  npm run test:browser
fi

log "Quality harness lane PASS"
