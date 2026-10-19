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
  echo "[BLOCKED] Chromium binary not found at $CHROMIUM_EXE (set CHROMIUM_EXE); browser journey NOT_TESTED"
  exit 0
fi

echo "== build core + worker + office (outputs under $TMP) =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)
if [[ ! -d "$ROOT/office/node_modules" ]]; then
  echo "office dependencies missing; installing from the lockfile"
  if ! (cd "$ROOT/office" && npm ci --no-audit --no-fund); then
    echo "[BLOCKED] npm ci failed (likely offline); browser journey NOT_TESTED"
    exit 0
  fi
fi
WEB_DIR="$TMP/s2b-dist"
# Typecheck first (same gate as CI), then bundle to a disposable directory:
# the repository working tree must not be modified by a test run.
(cd "$ROOT/office" && npm run typecheck >/dev/null)
(cd "$ROOT/office" && npx vite build --outDir "$WEB_DIR" >/dev/null)

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
  node "$ROOT/office/e2e-browser/s2-journey.mjs"
echo "[PASS] S2 browser journey completed (evidence in $TMP/s2-evidence)"
