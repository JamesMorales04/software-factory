#!/usr/bin/env bash
# F1-S3-D deterministic recovery acceptance: real Core + real Worker +
# stubbed Paseo CLI. No daemon, no provider calls, no paid sessions.
#
# D1 Worker SIGKILL, D2 Core restart, D3 daemon disconnect/reconnect
# simulator, D4 unbound session discovery, D5 concurrent recovery, D6
# repeated recovery after restart, D7 identity mismatch quarantine, plus a
# real Chromium check of the recovery projection. Real-daemon restart is
# NOT part of this suite (BLOCKED; no isolated daemon is started).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s3d-XXXXXX")"
TOKEN="s3de2e-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s3de2e-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S3D_PORT:-8478}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
BIN_WORKER="$TMP/factory-worker"
DB="$TMP/factory.db"
CORE_LOG="$TMP/core.log"
STUB_DIR="$TMP/stub"
STUBBIN="$TMP/stubbin"
CALLS="$STUB_DIR/calls.log"
CORE_PID=""
EXEC_PID=""
REC_A_PID=""
REC_B_PID=""
STRICT="${FACTORY_S3D_STRICT:-1}"

blocked() {
  echo "[BLOCKED] $1"
  if [[ "$STRICT" == "1" ]]; then
    echo "[FAIL] strict mode: blocked gate is a failure"
    exit 1
  fi
  exit 0
}

cleanup() {
  for pid_var in CORE_PID EXEC_PID REC_A_PID REC_B_PID; do
    pid="${!pid_var:-}"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT

echo "== build core + worker =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)

mkdir -p "$STUB_DIR" "$STUBBIN"
cp "$ROOT/scripts/s3d_paseo_stub.py" "$STUBBIN/paseo.py"
cat > "$STUBBIN/paseo" <<STUB_WRAPPER
#!/bin/sh
export S3D_STUB_DIR="$STUB_DIR"
exec python3 "$STUBBIN/paseo.py" "\$@"
STUB_WRAPPER
chmod +x "$STUBBIN/paseo" "$STUBBIN/paseo.py"
printf 'ok' > "$STUB_DIR/mode"
printf 'up' > "$STUB_DIR/daemon"
export S3D_STUB_DIR="$STUB_DIR"

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }
wapi() { curl -sf -H "Authorization: Bearer $WTOKEN" "$@"; }

start_core() {
  local web="${1:-}"
  local args=(--addr "127.0.0.1:${PORT}" --db "$DB")
  if [[ -n "$web" ]]; then args+=(--web-dir "$web"); fi
  env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_CORE" "${args[@]}" >"$CORE_LOG" 2>&1 &
  CORE_PID=$!
  for _ in $(seq 1 80); do
    if curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health"; then
      return 0
    fi
    sleep 0.2
  done
  echo "core failed to start; log:"; tail -40 "$CORE_LOG"
  return 1
}

stop_core() {
  if [[ -n "${CORE_PID:-}" ]] && kill -0 "$CORE_PID" 2>/dev/null; then
    kill "$CORE_PID" 2>/dev/null || true
    wait "$CORE_PID" 2>/dev/null || true
  fi
  CORE_PID=""
}

start_exec_worker() {
  env S3D_STUB_DIR="$STUB_DIR" PASEO_BIN="$STUBBIN/paseo" \
    FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_WORKER" --core "$BASE" --worker-id "$1" --exec-mode local \
    --recovery-interval 0 >"$TMP/exec-$1.log" 2>&1 &
  EXEC_PID=$!
  sleep 1
  kill -0 "$EXEC_PID" 2>/dev/null || { echo "exec worker failed"; tail -30 "$TMP/exec-$1.log"; return 1; }
}

kill_exec_worker() {
  if [[ -n "${EXEC_PID:-}" ]] && kill -0 "$EXEC_PID" 2>/dev/null; then
    kill -9 "$EXEC_PID" 2>/dev/null || true
    wait "$EXEC_PID" 2>/dev/null || true
  fi
  EXEC_PID=""
}

stop_exec_worker() {
  if [[ -n "${EXEC_PID:-}" ]] && kill -0 "$EXEC_PID" 2>/dev/null; then
    kill "$EXEC_PID" 2>/dev/null || true
    wait "$EXEC_PID" 2>/dev/null || true
  fi
  EXEC_PID=""
}

start_recovery_worker() {
  local var="$1" id="$2"
  env S3D_STUB_DIR="$STUB_DIR" PASEO_BIN="$STUBBIN/paseo" \
    FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_WORKER" --core "$BASE" --worker-id "$id" --exec-mode local \
    --recovery-only --recovery-interval 1s >"$TMP/recovery-$id.log" 2>&1 &
  printf -v "$var" '%s' "$!"
  sleep 1
  local pid="${!var}"
  kill -0 "$pid" 2>/dev/null || { echo "recovery worker failed"; tail -30 "$TMP/recovery-$id.log"; return 1; }
}

stop_recovery_workers() {
  for var in REC_A_PID REC_B_PID; do
    local pid="${!var:-}"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
    printf -v "$var" '%s' ""
  done
}

set_mode() { printf '%s' "$1" > "$STUB_DIR/mode"; }
set_daemon() { printf '%s' "$1" > "$STUB_DIR/daemon"; }

queue_run() { # timeout maxAttempts -> run id
  local resp
  resp="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
    -d "{\"workloadKind\":\"paseo-task\",\"workloadInput\":{\"task\":\"Append multiply to add.js\"},\"executionMode\":\"trusted-local\",\"provider\":\"codex\",\"model\":\"gpt-6-luna\",\"paseoMode\":\"full-access\",\"timeoutSecs\":$1,\"maxAttempts\":$2}")"
  python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$resp"
}

run_status() {
  auth "$BASE/api/v1/runs/$1" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])"
}

session_json() {
  auth "$BASE/api/v1/runs/$1/sessions" | python3 -c "import json,sys; s=json.load(sys.stdin)['sessions']; print(json.dumps(s[0] if s else {}))"
}

session_field() {
  session_json "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('$2',''))"
}

recovery_status() {
  session_json "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('recovery',{}).get('status',''))"
}

recovery_field() {
  session_json "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('recovery',{}).get('$2',''))"
}

wait_terminal() {
  local id="$1" budget="$2" status=""
  for _ in $(seq 1 "$budget"); do
    status="$(run_status "$id")"
    case "$status" in
      succeeded|failed|cancelled) echo "$status"; return 0 ;;
    esac
    sleep 1
  done
  echo "TIMEOUT($status)"; return 1
}

wait_binding() {
  local id="$1" want="$2" budget="$3" status=""
  for _ in $(seq 1 "$budget"); do
    status="$(session_field "$id" status)"
    [[ "$status" == "$want" ]] && { echo "$status"; return 0; }
    sleep 1
  done
  echo "TIMEOUT($status)"; return 1
}

wait_active() {
  local id="$1" budget="$2" status sid=""
  for _ in $(seq 1 "$budget"); do
    status="$(session_field "$id" status)"
    sid="$(session_field "$id" sessionId)"
    if [[ "$status" == "active" && -n "$sid" ]]; then echo "$sid"; return 0; fi
    sleep 1
  done
  echo "TIMEOUT($status/$sid)"; return 1
}

wait_recovery() {
  local id="$1" want="$2" budget="$3" status=""
  for _ in $(seq 1 "$budget"); do
    status="$(recovery_status "$id")"
    [[ "$status" == "$want" ]] && { echo "$status"; return 0; }
    sleep 1
  done
  echo "TIMEOUT($status)"; return 1
}

event_types() {
  auth "$BASE/api/v1/runs/$1/events" | python3 -c "import json,sys; print(' '.join(e['eventType'] for e in json.load(sys.stdin)['events']))"
}

count_events() { # runid type [outcome]
  auth "$BASE/api/v1/runs/$1/events" | python3 -c "
import json,sys
run_type, outcome = sys.argv[1], (sys.argv[2] if len(sys.argv)>2 else '')
events=json.load(sys.stdin)['events']
n=0
for e in events:
    if e['eventType']==run_type and (not outcome or e.get('payload',{}).get('outcome')==outcome):
        n+=1
print(n)" "$2" "${3:-}"
}

call_seen() { grep -Fxq "$1" "$CALLS" 2>/dev/null; }

echo "== start Core =="
start_core

echo
echo "== D1: Worker SIGKILL -> lease expiry, fencing, recovery =="
set_mode hang-logs; set_daemon up
start_exec_worker exec-d1
RUN1="$(queue_run 600 1)"
SID1="$(wait_active "$RUN1" 60)" || { echo "[FAIL] D1 binding never active"; exit 1; }
WS1="$(session_field "$RUN1" workspaceId)"
kill_exec_worker
[[ "$(wait_terminal "$RUN1" 40)" == "failed" ]] || { echo "[FAIL] D1 run did not fail closed"; exit 1; }
[[ "$(wait_binding "$RUN1" orphaned 20)" == "orphaned" ]] || { echo "[FAIL] D1 binding not orphaned"; exit 1; }
start_recovery_worker REC_A_PID recover-d1
[[ "$(wait_recovery "$RUN1" confirmed 90)" == "confirmed" ]] || { echo "[FAIL] D1 recovery not confirmed"; tail -40 "$TMP/recovery-recover-d1.log"; exit 1; }
EV1="$(event_types "$RUN1")"
grep -q RunExpired <<<"$EV1" || { echo "[FAIL] D1 missing RunExpired"; exit 1; }
grep -q PaseoSessionOrphaned <<<"$EV1" || { echo "[FAIL] D1 missing PaseoSessionOrphaned"; exit 1; }
grep -q PaseoSessionRecoveryRecorded <<<"$EV1" || { echo "[FAIL] D1 missing recovery event"; exit 1; }
grep -q RunCompleted <<<"$EV1" && { echo "[FAIL] D1 stale success recorded"; exit 1; }
call_seen "stop $SID1" || { echo "[FAIL] D1 owned session was not stopped"; exit 1; }
call_seen "archive $SID1" || { echo "[FAIL] D1 owned session was not archived"; exit 1; }
call_seen "workspace-archive $WS1" || { echo "[FAIL] D1 owned workspace was not archived"; exit 1; }
[[ "$(recovery_field "$RUN1" lastObservedAt)" != "" ]] || { echo "[FAIL] D1 no observation timestamp"; exit 1; }
echo "[PASS] D1: crash fenced, orphan recorded, external resources confirmed cleaned, no stale success"
stop_recovery_workers

echo
echo "== D2: Core restart -> durable state, no adoption, cleanup continues =="
set_mode hang-logs; start_exec_worker exec-d2
RUN2="$(queue_run 600 1)"
SID2="$(wait_active "$RUN2" 60)" || { echo "[FAIL] D2 binding never active"; exit 1; }
stop_core
start_core
[[ "$(wait_binding "$RUN2" orphaned 30)" == "orphaned" ]] || { echo "[FAIL] D2 binding not orphaned after restart"; exit 1; }
start_recovery_worker REC_A_PID recover-d2
[[ "$(wait_recovery "$RUN2" confirmed 90)" == "confirmed" ]] || { echo "[FAIL] D2 recovery not confirmed"; exit 1; }
EV2="$(event_types "$RUN2")"
grep -q PaseoSessionActive <<<"$EV2" || { echo "[FAIL] D2 history lost"; exit 1; }
grep -q PaseoSessionOrphaned <<<"$EV2" || { echo "[FAIL] D2 restart fencing missing"; exit 1; }
grep -q PaseoSessionRecoveryRecorded <<<"$EV2" || { echo "[FAIL] D2 cleanup receipt missing"; exit 1; }
grep -q RunCompleted <<<"$EV2" && { echo "[FAIL] D2 adopted superseded generation"; exit 1; }
[[ "$(run_status "$RUN2")" == "failed" ]] || { echo "[FAIL] D2 run not failed closed"; exit 1; }
echo "[PASS] D2: restart retained history, superseded generation never adopted, cleanup continued ($SID2)"
stop_exec_worker
stop_recovery_workers

echo
echo "== D3: daemon disconnect/reconnect simulator -> unconfirmed then confirmed =="
set_mode hang-logs; set_daemon up
start_exec_worker exec-d3
RUN3="$(queue_run 600 1)"
SID3="$(wait_active "$RUN3" 60)" || { echo "[FAIL] D3 binding never active"; exit 1; }
set_daemon down
auth -X POST "$BASE/api/v1/runs/$RUN3/cancel" -H 'Content-Type: application/json' -d '{}' >/dev/null
[[ "$(wait_terminal "$RUN3" 20)" == "cancelled" ]] || { echo "[FAIL] D3 cancellation not recorded"; exit 1; }
[[ "$(wait_binding "$RUN3" orphaned 20)" == "orphaned" ]] || { echo "[FAIL] D3 binding not orphaned"; exit 1; }
start_recovery_worker REC_A_PID recover-d3
[[ "$(wait_recovery "$RUN3" unconfirmed 60)" == "unconfirmed" ]] || { echo "[FAIL] D3 did not record unconfirmed"; exit 1; }
set_daemon up
[[ "$(wait_recovery "$RUN3" confirmed 90)" == "confirmed" ]] || { echo "[FAIL] D3 did not recover after reconnect"; exit 1; }
echo "[PASS] D3: disconnect stayed unconfirmed, reconnect produced verified cleanup ($SID3)"
stop_exec_worker
stop_recovery_workers

echo
echo "== D4: crash after session create before bind -> exact-label discovery =="
set_mode kill-parent-after-create; set_daemon up
start_exec_worker exec-d4
RUN4="$(queue_run 600 1)"
[[ "$(wait_terminal "$RUN4" 40)" == "failed" ]] || { echo "[FAIL] D4 run did not fail closed"; exit 1; }
[[ "$(wait_binding "$RUN4" orphaned 20)" == "orphaned" ]] || { echo "[FAIL] D4 unbound binding not orphaned"; exit 1; }
[[ -z "$(session_field "$RUN4" sessionId)" ]] || { echo "[FAIL] D4 expected an unbound session"; exit 1; }
start_recovery_worker REC_A_PID recover-d4
[[ "$(wait_recovery "$RUN4" confirmed 90)" == "confirmed" ]] || { echo "[FAIL] D4 discovery/cleanup failed"; tail -40 "$TMP/recovery-recover-d4.log"; exit 1; }
SID4="$(recovery_field "$RUN4" sessionId)"
WS4="$(recovery_field "$RUN4" workspaceId)"
[[ -n "$SID4" && -n "$WS4" ]] || { echo "[FAIL] D4 discovered identities not recorded"; exit 1; }
call_seen "stop $SID4" || { echo "[FAIL] D4 discovered session not stopped"; exit 1; }
call_seen "workspace-archive $WS4" || { echo "[FAIL] D4 discovered workspace not archived"; exit 1; }
grep -q RunCompleted <<<"$(event_types "$RUN4")" && { echo "[FAIL] D4 stale success"; exit 1; }
echo "[PASS] D4: unbound session identified by durable intent + exact labels, never guessed ($SID4)"
stop_recovery_workers

echo
echo "== D5: concurrent recovery -> one receipt, no cross cleanup =="
set_mode hang-logs; start_exec_worker exec-d5
RUN5="$(queue_run 600 1)"
SID5="$(wait_active "$RUN5" 60)" || { echo "[FAIL] D5 binding never active"; exit 1; }
set_daemon down
auth -X POST "$BASE/api/v1/runs/$RUN5/cancel" -H 'Content-Type: application/json' -d '{}' >/dev/null
[[ "$(wait_terminal "$RUN5" 20)" == "cancelled" ]] || { echo "[FAIL] D5 cancel"; exit 1; }
# Kill the execution Worker so only the two recovery Workers can act.
kill_exec_worker
set_daemon up
set_mode slow-cleanup
start_recovery_worker REC_A_PID recover-d5a
start_recovery_worker REC_B_PID recover-d5b
[[ "$(wait_recovery "$RUN5" confirmed 90)" == "confirmed" ]] || { echo "[FAIL] D5 no confirmed receipt"; exit 1; }
sleep 3
CONFIRMED5="$(count_events "$RUN5" PaseoSessionRecoveryRecorded confirmed)"
[[ "$CONFIRMED5" == "1" ]] || { echo "[FAIL] D5 duplicate authoritative receipt: $CONFIRMED5"; exit 1; }
STOPS5="$(grep -Fc "stop $SID5" "$CALLS" || true)"
[[ "$STOPS5" == "1" ]] || { echo "[FAIL] D5 stop calls = $STOPS5, want exactly 1"; exit 1; }
[[ "$(session_field "$RUN5" status)" == "orphaned" ]] || { echo "[FAIL] D5 binding mutated"; exit 1; }
echo "[PASS] D5: overlapping recovery Workers produced exactly one terminal receipt and one stop"
stop_recovery_workers
stop_exec_worker
set_mode ok

echo
echo "== D6: repeated recovery after Core restart -> idempotent =="
BEFORE6="$(count_events "$RUN5" PaseoSessionRecoveryRecorded)"
stop_core
start_core
start_recovery_worker REC_A_PID recover-d6
sleep 4
AFTER6="$(count_events "$RUN5" PaseoSessionRecoveryRecorded)"
[[ "$BEFORE6" == "$AFTER6" ]] || { echo "[FAIL] D6 recovery replayed: $BEFORE6 -> $AFTER6"; exit 1; }
[[ "$(recovery_status "$RUN5")" == "confirmed" ]] || { echo "[FAIL] D6 receipt lost"; exit 1; }
echo "[PASS] D6: terminal recovery is durable and is not replayed"
stop_recovery_workers

echo
echo "== D7: identity mismatch -> blocked without destructive action =="
set_mode hang-logs; start_exec_worker exec-d7
RUN7="$(queue_run 600 1)"
SID7="$(wait_active "$RUN7" 60)" || { echo "[FAIL] D7 binding never active"; exit 1; }
set_mode mismatch
kill_exec_worker
[[ "$(wait_terminal "$RUN7" 40)" == "failed" ]] || { echo "[FAIL] D7 run"; exit 1; }
[[ "$(wait_binding "$RUN7" orphaned 20)" == "orphaned" ]] || { echo "[FAIL] D7 orphan"; exit 1; }
start_recovery_worker REC_A_PID recover-d7
[[ "$(wait_recovery "$RUN7" blocked 60)" == "blocked" ]] || { echo "[FAIL] D7 mismatch not blocked"; exit 1; }
REASON7="$(recovery_field "$RUN7" reason)"
grep -qi "identity" <<<"$REASON7" || { echo "[FAIL] D7 blocked reason not actionable: $REASON7"; exit 1; }
call_seen "stop $SID7" && { echo "[FAIL] D7 destructive call on mismatched session"; exit 1; }
call_seen "archive $SID7" && { echo "[FAIL] D7 destructive call on mismatched session"; exit 1; }
echo "[PASS] D7: mismatch quarantined as blocked; no stop/archive issued"
stop_recovery_workers

# Browser recovery visibility. Build Office in place (dist/ is ignored).
echo
echo "== Pending recovery projection (no recovery Worker) =="
set_mode hang-logs; set_daemon up
start_exec_worker exec-pending
RUN8="$(queue_run 600 1)"
SID8="$(wait_active "$RUN8" 60)" || { echo "[FAIL] pending: binding never active"; exit 1; }
kill_exec_worker
[[ "$(wait_terminal "$RUN8" 40)" == "failed" ]] || { echo "[FAIL] pending: run"; exit 1; }
[[ "$(wait_binding "$RUN8" orphaned 20)" == "orphaned" ]] || { echo "[FAIL] pending: orphan"; exit 1; }
[[ "$(recovery_status "$RUN8")" == "pending" ]] || { echo "[FAIL] pending: no pending receipt"; exit 1; }
echo "[PASS] pending: orphan has a durable pending cleanup target ($SID8)"

echo
echo "== Office: pending, confirmed and blocked recovery visibility =="
CHROMIUM_EXE="${CHROMIUM_EXE:-$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome}"
if [[ ! -x "$CHROMIUM_EXE" ]]; then
  blocked "Chromium not found at $CHROMIUM_EXE (set CHROMIUM_EXE); S3-D browser check NOT_TESTED"
fi
(cd "$ROOT/office" && npm run build >/dev/null)
stop_core
start_core "$ROOT/office/dist"
BASE_URL="$BASE" FACTORY_API_TOKEN="$TOKEN" CHROMIUM_EXE="$CHROMIUM_EXE" \
  S3D_EVIDENCE_DIR="$TMP/s3d-evidence" \
  S3D_CONFIRMED_RUN="$RUN5" S3D_CONFIRMED_SESSION="$SID5" \
  S3D_BLOCKED_RUN="$RUN7" S3D_BLOCKED_SESSION="$SID7" \
  S3D_PENDING_RUN="$RUN8" S3D_PENDING_SESSION="$SID8" \
  node "$ROOT/office/e2e-browser/s3d-recovery.mjs"

echo
echo "S3-D deterministic recovery acceptance: ALL PHASES DONE (stubbed Paseo; no paid sessions)"
echo "confirmed run: $RUN5 session: $SID5"
echo "blocked run:   $RUN7 session: $SID7"
echo "pending run:   $RUN8 session: $SID8"
echo "real daemon restart: BLOCKED (not attempted; simulator only)"
echo "evidence: $TMP/s3d-evidence"
