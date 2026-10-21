#!/usr/bin/env bash
# F1-S4-C deterministic acceptance: real Core, real Worker, real
# Core-spawned verifier/reviewer, real Chromium, two external Project
# fixtures and a deterministic Paseo CLI simulator. No provider calls.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s4c-XXXXXX")"
PORT="${FACTORY_S4C_PORT:-8492}"
CHROMIUM_EXE="${CHROMIUM_EXE:-$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome}"
PLAYWRIGHT_CORE="${FACTORY_PLAYWRIGHT_CORE:-$ROOT/office/e2e-browser/node_modules/playwright-core}"
BROWSER_CLI="${FACTORY_BROWSER_CLI:-$ROOT/office/e2e-browser/node_modules/.bin/playwright-cli}"
VLOG="${S4C_VLOG_REPO:-$HOME/Escritorio/Development/factory-fixtures/factory-fixture-vlog}"
CALC="${S4C_CALC_REPO:-$HOME/Escritorio/Development/factory-fixtures/factory-fixture-calculator}"

materialize_fixture() {
  local source="$1" dest="$2" label="$3"
  mkdir -p "$dest"
  cp -R "$source"/. "$dest"/
  rm -rf "$dest/.git"
  git -C "$dest" init -q -b main
  git -C "$dest" add .
  git -C "$dest" -c user.name=fixture -c user.email=fixture@factory.local \
    commit -qm "disposable $label baseline"
}

if [[ ! -d "$VLOG/.git" ]]; then
  materialize_fixture "$ROOT/scripts/fixtures/s4c-vlog" "$TMP/fixtures/vlog" vlog
  VLOG="$TMP/fixtures/vlog"
fi
if [[ ! -d "$CALC/.git" ]]; then
  materialize_fixture "$ROOT/scripts/fixtures/s4c-calculator" "$TMP/fixtures/calculator" calculator
  CALC="$TMP/fixtures/calculator"
fi

blocked() {
  echo "[BLOCKED] $1"
  exit 2
}

echo "== build Core, Worker, Verifier, Reviewer and Office =="
(cd "$ROOT/core" && go build -o "$TMP/factory-core" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$TMP/factory-worker" ./cmd/factory-worker)
(cd "$ROOT/core" && go build -o "$TMP/factory-verifier" ./cmd/factory-verifier)
(cd "$ROOT/core" && go build -o "$TMP/factory-reviewer" ./cmd/factory-reviewer)
(cd "$ROOT/office" && npm run build >/dev/null)

[[ -x "$CHROMIUM_EXE" ]] || blocked "Chromium not found at $CHROMIUM_EXE"
[[ -d "$PLAYWRIGHT_CORE" ]] || blocked "playwright-core not found at $PLAYWRIGHT_CORE"
[[ -x "$BROWSER_CLI" ]] || blocked "pinned Playwright CLI not found at $BROWSER_CLI"
[[ -d "$VLOG/.git" ]] || blocked "Project A fixture not found at $VLOG"
[[ -d "$CALC/.git" ]] || blocked "Project B fixture not found at $CALC"
command -v node >/dev/null || blocked "node is unavailable"

echo "== F1-S4-C deterministic oracle =="
python3 "$ROOT/scripts/e2e_s4c_oracle.py" \
  "$VLOG" "$CALC" \
  "$TMP/factory-core" "$TMP/factory-worker" \
  "$TMP/factory-verifier" "$TMP/factory-reviewer" \
  "$TMP" "$CHROMIUM_EXE" "$PLAYWRIGHT_CORE" "$PORT" "$ROOT/office/dist" "$BROWSER_CLI"

echo
echo "F1-S4-C deterministic acceptance: ALL PHASES DONE"
echo "evidence: $TMP/evidence"
echo "TRUSTED_LOCAL only; no isolated/certified claim and no paid model sessions."
