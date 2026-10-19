#!/usr/bin/env bash
# F1-S2 real-browser journey: unlock -> Workers -> Runs -> queue a synthetic
# Run through the Office UI -> succeeded -> Run detail (Attempts + event
# lineage). Real Core binary, real Worker binary, real Chromium.
# Exits nonzero on the first failing step.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s2b-XXXXXX")"
TOKEN="s2b-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s2b-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S2B_PORT:-8474}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
BIN_WORKER="$TMP/factory-worker"
DB="$TMP/factory.db"
CORE_PID=""
WORKER_PID=""
# Strict mode (CI) turns a skipped browser gate into a failure: a mandatory
# browser check that was not executed must never read as green.
STRICT="${FACTORY_S2B_STRICT:-0}"

blocked() { # $1 = reason
  echo "[BLOCKED] $1"
  if [[ "$STRICT" == "1" ]]; then
    echo "[FAIL] strict mode: blocked gate is a failure"
    exit 1
  fi
  exit 0
}

cleanup() {
  if [[ -n "${WORKER_PID:-}" ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill "$WORKER_PID" 2>/dev/null || true
    wait "$WORKER_PID" 2>/dev/null || true
  fi
  if [[ -n "${CORE_PID:-}" ]] && kill -0 "$CORE_PID" 2>/dev/null; then
    kill "$CORE_PID" 2>/dev/null || true
    wait "$CORE_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

CHROMIUM_EXE="${CHROMIUM_EXE:-$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome}"
if [[ ! -x "$CHROMIUM_EXE" ]]; then
  blocked "Chromium binary not found at $CHROMIUM_EXE (set CHROMIUM_EXE); browser journey NOT_TESTED"
fi

echo "== build core + worker + office (all outputs under $TMP) =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)
# The Office tree is copied to a disposable directory first: dependency
# installs and bundling must never modify the repository working tree,
# so a test run leaves `git status` clean by construction.
WEB_SRC="$TMP/office-src"
mkdir -p "$WEB_SRC"
cp -r "$ROOT/office/." "$WEB_SRC/"
rm -rf "$WEB_SRC/node_modules" "$WEB_SRC/e2e-browser/node_modules" \
  "$WEB_SRC/dist" "$WEB_SRC/e2e-browser/.s2-evidence" \
  "$WEB_SRC/e2e-browser/.journey-evidence" "$WEB_SRC/e2e-browser/.soak-evidence" \
  "$WEB_SRC/e2e-browser/.sse.cjs"
if ! (cd "$WEB_SRC" && npm ci --no-audit --no-fund >/dev/null 2>&1); then
  blocked "office npm ci failed (likely offline); browser journey NOT_TESTED"
fi
if ! (cd "$WEB_SRC/e2e-browser" && npm ci --no-audit --no-fund >/dev/null 2>&1); then
  blocked "e2e-browser npm ci failed (likely offline); browser journey NOT_TESTED"
fi
WEB_DIR="$TMP/s2b-dist"
# Typecheck first (same gate as CI), then bundle to the disposable directory.
(cd "$WEB_SRC" && npm run typecheck >/dev/null)
(cd "$WEB_SRC" && npx vite build --outDir "$WEB_DIR" >/dev/null)

echo "== start core (port $PORT) + worker =="
env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
  "$BIN_CORE" --addr "127.0.0.1:${PORT}" --db "$DB" --web-dir "$WEB_DIR" >"$TMP/core.log" 2>&1 &
CORE_PID=$!
for _ in $(seq 1 50); do
  if curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health"; then
    break
  fi
  sleep 0.2
done
env FACTORY_WORKER_TOKEN="$WTOKEN" \
  "$BIN_WORKER" --core "$BASE" --worker-id local-1 >"$TMP/worker.log" 2>&1 &
WORKER_PID=$!
sleep 3

echo "== real-browser S2 journey =="
BASE_URL="$BASE" FACTORY_API_TOKEN="$TOKEN" CHROMIUM_EXE="$CHROMIUM_EXE" \
  S2_EVIDENCE_DIR="$TMP/s2-evidence" \
  node "$WEB_SRC/e2e-browser/s2-journey.mjs"
echo "[PASS] S2 browser journey completed (evidence in $TMP/s2-evidence)"
