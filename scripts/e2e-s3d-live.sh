#!/usr/bin/env bash
# F1-S3-D bounded real acceptance. Exactly ONE paid Paseo session is
# authorized per initial acceptance; this script never retries one.
#
# Route: opencode/opencode-go/deepseek-v4.1-flash, mode build (TRUSTED_LOCAL,
# broad local access, not isolated). The Worker is killed only after the
# real session is durably bound; a recovery-only Worker then stops/archives
# the owned session/workspace by exact title + labels. No unrelated Paseo
# session is touched and the shared daemon is never restarted.
set -euo pipefail

LIVE="${FACTORY_S3D_LIVE:-0}"
if [[ "$LIVE" != "0" && "$LIVE" != "1" ]]; then
  echo "[BLOCKED] FACTORY_S3D_LIVE must be 0 or 1"; exit 2
fi
if [[ "$LIVE" != "1" ]]; then
  echo "[SKIP] S3-D live lane disabled; no provider session requested"
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s3d-live-XXXXXX")"
TOKEN="s3dlive-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s3dlive-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S3D_LIVE_PORT:-8496}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
BIN_WORKER="$TMP/factory-worker"
DB="$TMP/factory.db"
PROVIDER="${S3D_LIVE_PROVIDER:-opencode}"
MODEL="${S3D_LIVE_MODEL:-opencode-go/deepseek-v4.1-flash}"
PASEO_MODE="build"
python3 "$ROOT/scripts/live-e2e-authorization.py" \
  --campaign F1-S3-D --project default --max-sessions 1 \
  --route opencode/opencode-go/deepseek-v4.1-flash \
  --requested-route "$PROVIDER/$MODEL"
CORE_PID=""
WORKER_PID=""
RECOVERY_PID=""

cleanup() {
  for var in CORE_PID WORKER_PID RECOVERY_PID; do
    pid="${!var:-}"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }

wait_run() { # id want budget
  local status=""
  for _ in $(seq 1 "$3"); do
    status="$(auth "$BASE/api/v1/runs/$1" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])")"
    [[ "$status" == "$2" ]] && { echo "$status"; return 0; }
    case "$status" in succeeded|failed|cancelled) [[ "$2" != "$status" ]] && { echo "$status"; return 1; } ;; esac
    sleep 1
  done
  echo "TIMEOUT($status)"; return 1
}

session_json() {
  auth "$BASE/api/v1/runs/$1/sessions" | python3 -c "import json,sys; s=json.load(sys.stdin)['sessions']; print(json.dumps(s[0] if s else {}))"
}
session_field() { session_json "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('$2',''))"; }
recovery_status() { session_json "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('recovery',{}).get('status',''))"; }
recovery_field() { session_json "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('recovery',{}).get('$2',''))"; }

echo "== build core + worker =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)

echo "== start Core =="
env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
  "$BIN_CORE" --addr "127.0.0.1:${PORT}" --db "$DB" >"$TMP/core.log" 2>&1 &
CORE_PID=$!
for _ in $(seq 1 80); do
  curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health" && break
  sleep 0.2
done

echo "== start real Worker (one session only: $PROVIDER/$MODEL $PASEO_MODE) =="
env FACTORY_WORKER_TOKEN="$WTOKEN" \
  "$BIN_WORKER" --core "$BASE" --worker-id live-s3d --exec-mode local \
  --recovery-interval 0 >"$TMP/worker.log" 2>&1 &
WORKER_PID=$!
sleep 2
kill -0 "$WORKER_PID" 2>/dev/null || { echo "worker failed"; tail -40 "$TMP/worker.log"; exit 1; }

RUN="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d "{\"workloadKind\":\"paseo-task\",\"workloadInput\":{\"task\":\"Append a JSDoc multiply(a, b) function to add.js and export it (keep add unchanged).\"},\"executionMode\":\"trusted-local\",\"provider\":\"$PROVIDER\",\"model\":\"$MODEL\",\"paseoMode\":\"$PASEO_MODE\",\"timeoutSecs\":600,\"maxAttempts\":1}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])")"
echo "run: $RUN"

ACTIVE=""
for _ in $(seq 1 300); do
  status="$(session_field "$RUN" status)"
  sid="$(session_field "$RUN" sessionId)"
  if [[ "$status" == "active" && -n "$sid" ]]; then ACTIVE="$sid"; break; fi
  case "$status" in succeeded|failed|cancelled) echo "binding ended before interruption: $status"; break ;; esac
  sleep 1
done
[[ -n "$ACTIVE" ]] || { echo "[FAIL] real session was never durably bound"; tail -60 "$TMP/worker.log"; exit 1; }
WS="$(session_field "$RUN" workspaceId)"
echo "durable binding: session=$ACTIVE workspace=$WS"

echo "== owned Worker interruption (SIGKILL after binding) =="
kill -9 "$WORKER_PID" 2>/dev/null || true
wait "$WORKER_PID" 2>/dev/null || true
WORKER_PID=""
[[ "$(wait_run "$RUN" failed 60)" == "failed" ]] || { echo "[FAIL] run did not fail closed after Worker death"; exit 1; }
[[ "$(session_field "$RUN" status)" == "orphaned" ]] || { echo "[FAIL] binding not orphaned"; exit 1; }

echo "== start recovery-only Worker =="
env FACTORY_WORKER_TOKEN="$WTOKEN" \
  "$BIN_WORKER" --core "$BASE" --worker-id live-s3d-recovery --exec-mode local \
  --recovery-only --recovery-interval 1s >"$TMP/recovery.log" 2>&1 &
RECOVERY_PID=$!
sleep 2
kill -0 "$RECOVERY_PID" 2>/dev/null || { echo "recovery worker failed"; tail -40 "$TMP/recovery.log"; exit 1; }

CONFIRMED=""
for _ in $(seq 1 360); do
  if [[ "$(recovery_status "$RUN")" == "confirmed" ]]; then CONFIRMED=1; break; fi
  sleep 1
done
if [[ -z "$CONFIRMED" ]]; then
  echo "[FAIL] real cleanup was not confirmed"
  python3 -m json.tool <<<"$(session_json "$RUN")"
  tail -80 "$TMP/recovery.log"
  echo "The owned session was NOT declared cleaned. Inspect session $ACTIVE manually; do not run a second paid session."
  exit 1
fi

echo "== external verification with the real Paseo CLI =="
paseo inspect "$ACTIVE" --json >"$TMP/final-inspect.json"
python3 - "$TMP/final-inspect.json" "$ACTIVE" "$WS" <<'PY'
import json, sys
inspect, sid, ws = sys.argv[1:4]
data = json.load(open(inspect, encoding="utf-8"))
assert data.get("Id") == sid, data
assert data.get("Archived") is True, data
print("[PASS] real session archived:", sid)
PY
if paseo workspace ls --json | python3 -c "import json,sys; ids=[w.get('workspaceId') for w in json.load(sys.stdin)]; sys.exit(0 if '$WS' in ids else 1)"; then
  echo "[FAIL] workspace $WS still active"
  exit 1
fi
echo "[PASS] real workspace absent from active listing: $WS"

EVENTS="$(auth "$BASE/api/v1/runs/$RUN/events" | python3 -c "import json,sys; print(' '.join(e['eventType'] for e in json.load(sys.stdin)['events']))")"
grep -q PaseoSessionOrphaned <<<"$EVENTS" || { echo "[FAIL] missing orphan event"; exit 1; }
grep -q PaseoSessionRecoveryRecorded <<<"$EVENTS" || { echo "[FAIL] missing recovery receipt"; exit 1; }
grep -q RunCompleted <<<"$EVENTS" && { echo "[FAIL] stale authoritative success"; exit 1; }
[[ "$(session_field "$RUN" status)" == "orphaned" ]] || { echo "[FAIL] binding mutated"; exit 1; }
[[ "$(recovery_field "$RUN" sessionId)" == "$ACTIVE" ]] || { echo "[FAIL] recovery identity mismatch"; exit 1; }

echo "[PASS] S3-D live: one real session, durable binding, fenced Worker death, verified cleanup, no stale success"
echo "evidence: $TMP"
echo "run=$RUN session=$ACTIVE workspace=$WS"
echo "NO second paid session was started."
