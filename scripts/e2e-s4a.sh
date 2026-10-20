#!/usr/bin/env bash
# F1-S4-A deterministic acceptance: real Core + real Worker + deterministic
# Paseo CLI simulator + independent verifier + independent reviewer + real
# Chromium (candidate UI checks and Office projections). No daemon, no
# provider calls, no paid sessions.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s4a-XXXXXX")"
TOKEN="s4a-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s4a-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
VTOKEN="s4a-verifier-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
RTOKEN="s4a-reviewer-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S4A_PORT:-8480}"
BASE="http://127.0.0.1:${PORT}"
STRICT="${FACTORY_S4A_STRICT:-1}"
CHROMIUM_EXE="${CHROMIUM_EXE:-$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome}"
PLAYWRIGHT_CORE="${FACTORY_PLAYWRIGHT_CORE:-$ROOT/office/e2e-browser/node_modules/playwright-core}"

blocked() {
  echo "[BLOCKED] $1"
  if [[ "$STRICT" == "1" ]]; then
    echo "[FAIL] strict mode: blocked gate is a failure"
    exit 1
  fi
  exit 0
}

echo "== build Core, Worker, Verifier, Reviewer and Office =="
(cd "$ROOT/core" && go build -o "$TMP/factory-core" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$TMP/factory-worker" ./cmd/factory-worker)
(cd "$ROOT/core" && go build -o "$TMP/factory-verifier" ./cmd/factory-verifier)
(cd "$ROOT/core" && go build -o "$TMP/factory-reviewer" ./cmd/factory-reviewer)
(cd "$ROOT/office" && npm run build >/dev/null)

if [[ ! -x "$CHROMIUM_EXE" ]]; then
  blocked "Chromium not found at $CHROMIUM_EXE (set CHROMIUM_EXE); browser checks NOT_TESTED"
fi
if [[ ! -d "$PLAYWRIGHT_CORE" ]]; then
  blocked "playwright-core not found at $PLAYWRIGHT_CORE; browser checks NOT_TESTED"
fi
if [[ ! -x "$ROOT/office/e2e-browser/node_modules/.bin/playwright-cli" ]]; then
  blocked "pinned @playwright/cli is not installed (npm ci in office/e2e-browser)"
fi

echo "== F1-S4-A deterministic oracle =="
python3 "$ROOT/scripts/e2e_s4a_oracle.py" \
  "$BASE" "$TOKEN" "$WTOKEN" "$VTOKEN" "$RTOKEN" \
  "$TMP/factory-core" "$TMP/factory-worker" \
  "$TMP/factory-verifier" "$TMP/factory-reviewer" \
  "$TMP" "$CHROMIUM_EXE" "$PLAYWRIGHT_CORE" "$PORT" \
  "$ROOT/office/dist" "all"

echo
echo "F1-S4-A deterministic acceptance: ALL PHASES DONE"
echo "evidence: $TMP/evidence"
echo "state:    $TMP/state.json"
echo "TRUSTED_LOCAL only; no isolated/certified claim and no paid sessions."
