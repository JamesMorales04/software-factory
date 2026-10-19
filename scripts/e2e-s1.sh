#!/usr/bin/env bash
# F1-S1 end-to-end acceptance: real Core binary, real SQLite file, real HTTP.
# Positive journey + negatives + restart retention + offline-OTLP proof +
# secret-redaction proof. Exits nonzero on the first failing phase.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s1-XXXXXX")"
TOKEN="s1-e2e-token-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
CANARY="CANARY_$(head -c12 /dev/urandom | od -An -tx1 | tr -d ' \n')"
export FACTORY_API_TOKEN="$TOKEN"
PORT="${FACTORY_E2E_PORT:-8471}"
BASE="http://127.0.0.1:${PORT}"
BIN="$TMP/factory-core"
DB="$TMP/factory.db"
LOG1="$TMP/core1.log"
LOG2="$TMP/core2.log"
LOG3="$TMP/core3.log"
CORE_PID=""

cleanup() {
  if [[ -n "$CORE_PID" ]] && kill -0 "$CORE_PID" 2>/dev/null; then
    kill "$CORE_PID" 2>/dev/null || true
    wait "$CORE_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "== build core =="
(cd "$ROOT/core" && go build -o "$BIN" ./cmd/factory-core)

start_core() { # $1=logfile, extra env follows as $2...
  local log="$1"; shift
  env "$@" FACTORY_API_TOKEN="$TOKEN" "$BIN" --addr "127.0.0.1:${PORT}" --db "$DB" >"$log" 2>&1 &
  CORE_PID=$!
  for _ in $(seq 1 50); do
    if curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health"; then
      return 0
    fi
    sleep 0.2
  done
  echo "core failed to start; log:"; cat "$log"
  return 1
}

stop_core() {
  kill "$CORE_PID" 2>/dev/null || true
  wait "$CORE_PID" 2>/dev/null || true
  CORE_PID=""
  sleep 0.5 # let WAL checkpoint on clean shutdown
}

echo "== phase 1: boot + full oracle =="
start_core "$LOG1"
python3 "$ROOT/scripts/e2e_oracle.py" "$BASE" "$TOKEN"
stop_core

echo "== phase 2: restart, same DB, oracle expects retained state =="
start_core "$LOG2"
N_ACTIONS=$(python3 -c "import json,urllib.request; r=urllib.request.Request('$BASE/api/v1/actions', headers={'Authorization':'Bearer $TOKEN'}); print(len(json.load(urllib.request.urlopen(r))['actions']))")
# Phase-2 journey creates exactly one new action (SSE skipped), so the
# retained total must be previous count + 1 with history intact.
python3 "$ROOT/scripts/e2e_oracle.py" "$BASE" "$TOKEN" --expect-actions "$((N_ACTIONS+1))" --skip-sse
stop_core

echo "== phase 3: unreachable OTLP exporter must not block writes =="
start_core "$LOG3" "OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:1"
python3 - "$BASE" "$TOKEN" <<'EOF'
import json, sys, urllib.request
base, token = sys.argv[1], sys.argv[2]
req = urllib.request.Request(base + "/api/v1/actions",
    data=json.dumps({"title": "otel-outage-proof"}).encode(),
    method="POST",
    headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
with urllib.request.urlopen(req, timeout=15) as res:
    body = json.load(res)
assert res.status == 201 and body["action"]["id"], body
print("[PASS] create with dead OTLP exporter still persists")
EOF
stop_core

echo "== phase 4: canary redaction =="
if grep -q "$CANARY" "$LOG1" "$LOG2" "$LOG3"; then
  echo "[FAIL] canary leaked into core logs"; exit 1
fi
# The canary travels inside an action description created in phase 1? No:
# create it now against a fresh boot and re-grep.
start_core "$LOG1"
python3 - "$BASE" "$TOKEN" "$CANARY" <<'EOF'
import json, sys, urllib.request
base, token, canary = sys.argv[1], sys.argv[2], sys.argv[3]
req = urllib.request.Request(base + "/api/v1/actions",
    data=json.dumps({"title": "redaction probe", "description": f"holds {canary} inside"}).encode(),
    method="POST",
    headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json",
             "X-Correlation-ID": f"token={canary}"})
with urllib.request.urlopen(req, timeout=15) as res:
    assert res.status == 201, res.status
print("[PASS] canary-carrying action stored")
EOF
stop_core
if grep -q "$CANARY" "$LOG1" "$LOG2" "$LOG3"; then
  echo "[FAIL] canary leaked into core logs"; grep -n "$CANARY" "$LOG1" | head -3 || true; exit 1
fi
echo "[PASS] canary absent from all core logs"

echo
echo "S1 E2E: ALL PHASES PASSED (tmp=$TMP)"
