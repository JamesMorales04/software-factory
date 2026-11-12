#!/usr/bin/env bash
# Local/CI quality lane for the stage-addressed harness and the Office typed
# lint/browser checks. It does not start Compose, a model or GitHub.
#
# Environment:
#   QUALITY_HARNESS_SKIP_BROWSER=1  skip the real-Chromium component lane
#   QUALITY_HARNESS_SKIP_A11Y=1     skip the axe floor
#   QUALITY_HARNESS_BROWSER_REPEAT  browser-component repeats (default 1; CI uses 20)
#   CHROMIUM_EXE=/path/to/chrome     browser executable for the component lane
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

log() { printf '\n== %s ==\n' "$*"; }

for tool in python3 npm curl; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'BLOCKED: required tool is missing: %s\n' "$tool" >&2
    exit 2
  }
done

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
  repeat="${QUALITY_HARNESS_BROWSER_REPEAT:-1}"
  for ((run = 1; run <= repeat; run += 1)); do
    log "Office real-browser component tests (${run}/${repeat})"
    npm run test:browser
  done
fi

if [[ "${QUALITY_HARNESS_SKIP_A11Y:-0}" != "1" ]]; then
  log "Office automated accessibility floor"
  a11y_port="${FACTORY_A11Y_PORT:-5199}"
  a11y_log="${TMPDIR:-/tmp}/factory-quality-a11y-vite.log"
  npm run dev -- --port "$a11y_port" --strictPort >"$a11y_log" 2>&1 &
  a11y_pid=$!
  cleanup_a11y() {
    kill "$a11y_pid" 2>/dev/null || true
    wait "$a11y_pid" 2>/dev/null || true
  }
  trap cleanup_a11y EXIT
  ready=0
  for _ in $(seq 1 100); do
    if curl -fsS -o /dev/null "http://localhost:${a11y_port}/" 2>/dev/null; then
      ready=1
      break
    fi
    sleep 0.1
  done
  if ((ready != 1)); then
    printf 'BLOCKED: Office Vite server did not become ready; see %s\n' "$a11y_log" >&2
    exit 2
  fi
  FACTORY_OFFICE_URL="http://localhost:${a11y_port}/" npm run test:a11y
  cleanup_a11y
  trap - EXIT
fi

log "Quality harness lane PASS"
