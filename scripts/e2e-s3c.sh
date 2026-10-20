#!/usr/bin/env bash
# F1-S3-C end-to-end acceptance: real Factory Core + real Worker + real
# Chromium, with a deterministic stubbed `paseo` CLI (no daemon, no provider
# calls, no paid sessions).
#
# Proves the S3-C observability slice:
#   - empty Agents state, then a real durable session binding appears live;
#   - a pending/active binding is observable while the Run still executes;
#   - a failed Attempt and a successful retry keep separate generations;
#   - provider/model/mode/Worker/Attempt/generation/Run identity are rendered
#     from Core snapshots, never fabricated;
#   - RunDetail shows the session, bounded final evidence and an ordered
#     lifecycle ledger;
#   - browser reload and malformed-SSE reconnect preserve exactly the same
#     duplicate-free timeline;
#   - a cancelled Run with an unconfirmed external session is shown as
#     orphaned, never as a confirmed stop;
#   - the history survives a real Core restart.
# Exits nonzero on the first failing phase.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s3c-XXXXXX")"
TOKEN="s3ce2e-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s3ce2e-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S3C_PORT:-8476}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
BIN_WORKER="$TMP/factory-worker"
DB="$TMP/factory.db"
CORE_LOG="$TMP/core.log"
WORKER_LOG="$TMP/worker.log"
CORE_PID=""
WORKER_PID=""
STRICT="${FACTORY_S3C_STRICT:-0}"
STUBBIN="$TMP/stubbin"
STUB_CONF="$TMP/stub.conf"
TASK_TEXT="Append a pure function multiply(a, b) with a JSDoc comment to add.js and export it alongside add (keep add unchanged)."

blocked() {
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
  blocked "Chromium binary not found at $CHROMIUM_EXE (set CHROMIUM_EXE); S3-C browser journey NOT_TESTED"
fi
command -v node >/dev/null || blocked "node is required for the S3-C browser journey"
command -v npm >/dev/null || blocked "npm is required for the S3-C browser journey"

echo "== build core + worker =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)

echo "== build Office in a disposable copy =="
WEB_SRC="$TMP/office-src"
mkdir -p "$WEB_SRC"
cp -r "$ROOT/office/." "$WEB_SRC/"
rm -rf "$WEB_SRC/node_modules" "$WEB_SRC/e2e-browser/node_modules" \
  "$WEB_SRC/dist" "$WEB_SRC/e2e-browser/.s2-evidence" \
  "$WEB_SRC/e2e-browser/.journey-evidence" "$WEB_SRC/e2e-browser/.soak-evidence" \
  "$WEB_SRC/e2e-browser/.s3c-evidence" "$WEB_SRC/e2e-browser/.sse.cjs"
if ! (cd "$WEB_SRC" && npm ci --no-audit --no-fund >/dev/null 2>&1); then
  blocked "office npm ci failed (likely offline); S3-C browser journey NOT_TESTED"
fi
if ! (cd "$WEB_SRC/e2e-browser" && npm ci --no-audit --no-fund >/dev/null 2>&1); then
  blocked "e2e-browser npm ci failed (likely offline); S3-C browser journey NOT_TESTED"
fi
WEB_DIR="$TMP/s3c-dist"
(cd "$WEB_SRC" && npm run typecheck >/dev/null)
(cd "$WEB_SRC" && npx vite build --outDir "$WEB_DIR" >/dev/null)

start_core() {
  env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_CORE" --addr "127.0.0.1:${PORT}" --db "$DB" --web-dir "$WEB_DIR" >"$CORE_LOG" 2>&1 &
  CORE_PID=$!
  for _ in $(seq 1 60); do
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

start_worker() {
  env PATH="$STUBBIN:$PATH" FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_WORKER" --core "$BASE" --worker-id local-1 --exec-mode local >"$WORKER_LOG" 2>&1 &
  WORKER_PID=$!
  sleep 2
  if ! kill -0 "$WORKER_PID" 2>/dev/null; then
    echo "worker failed to start; log:"; tail -40 "$WORKER_LOG"
    return 1
  fi
}

stop_worker() {
  if [[ -n "${WORKER_PID:-}" ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill "$WORKER_PID" 2>/dev/null || true
    wait "$WORKER_PID" 2>/dev/null || true
  fi
  WORKER_PID=""
}

# write_stub <ok|hang-logs>: installs a self-contained `paseo` stub. The
# success mode performs a real edit in the recorded workspace; hang-logs
# lets the Worker bind a real session identity and then hangs the timeline
# observation, so an operator cancel produces an orphaned bound session.
write_stub() {
  mkdir -p "$STUBBIN"
  printf '%s' "$1" > "$STUB_CONF"
  cat > "$STUBBIN/paseo" <<STUB_EOF
#!/usr/bin/env bash
CONF="$STUB_CONF"
MODE="\$(cat "\$CONF" 2>/dev/null || echo ok)"
STATE="\$CONF.state"
next_id() {
  n=\$(cat "\$CONF.count" 2>/dev/null || echo 0); n=\$((n+1)); echo "\$n" > "\$CONF.count"
  echo "\$1_stub_\$n"
}
record() { echo "\$@" >> "\$STATE"; }
lookup() { grep -h "^\$1 " "\$STATE" 2>/dev/null | head -1 | cut -d' ' -f2- || true; }
case "\$1" in
  --version) echo "paseo-stub 0.0.0-s3c" ;;
  workspace)
    case "\$2" in
      create)
        path=""; while (( \$# > 0 )); do case "\$1" in --path) path="\$2"; shift 2;; *) shift;; esac; done
        id="\$(next_id ws)"; record "\$id \$path"; echo "{\"workspaceId\":\"\$id\"}" ;;
      ls) echo '[]' ;;
      archive) exit 0 ;;
    esac ;;
  run)
    if [[ "\$MODE" == "fail-once" && ! -f "\$CONF.failedonce" ]]; then
      touch "\$CONF.failedonce"
      echo "stub: transient provider failure" >&2
      exit 1
    fi
    ws=""; while (( \$# > 0 )); do case "\$1" in --workspace) ws="\$2"; shift 2;; *) shift;; esac; done
    sid="\$(next_id STUBAGENT)"; record "sess \$sid \$ws"
    if [[ "\$MODE" == "ok" || "\$MODE" == "fail-once" ]]; then
      proj="\$(lookup "\$ws")"
      if [[ -f "\$proj/add.js" ]] && ! grep -q multiply "\$proj/add.js"; then
        cat >> "\$proj/add.js" <<'JS_EOF'
/** Multiply two numbers. */
function multiply(a, b) {
  return a * b;
}
module.exports.multiply = multiply;
JS_EOF
      fi
    fi
    echo "{\"Id\":\"\$sid\"}" ;;
  inspect)
    ws="\$(grep -h "^sess \$2 " "\$STATE" 2>/dev/null | head -1 | cut -d' ' -f3- || true)"
    proj="\$(lookup "\$ws")"
    echo "{\"Id\":\"\$2\",\"Provider\":\"codex\",\"Model\":\"gpt-6-luna\",\"Mode\":\"full-access\",\"Cwd\":\"\$proj\",\"Status\":\"idle\"}" ;;
  logs)
    if [[ "\$MODE" == "hang-logs" ]]; then exec sleep 120; fi
    printf '[User] task\n[Read] add.js\n[Edit] add.js\n[Shell] npm test\n[Text] done\n' ;;
  send|wait|stop|archive) exit 0 ;;
  *) exit 3 ;;
esac
STUB_EOF
  chmod +x "$STUBBIN/paseo"
}

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }
worker() { curl -sf -H "Authorization: Bearer $WTOKEN" "$@"; }

queue_paseo() {
  local timeout="$1" attempts="${2:-1}" resp
  resp="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
    -d "{\"workloadKind\":\"paseo-task\",\"workloadInput\":{\"task\":\"$TASK_TEXT\"},\"executionMode\":\"trusted-local\",\"provider\":\"codex\",\"model\":\"gpt-6-luna\",\"paseoMode\":\"full-access\",\"timeoutSecs\":$timeout,\"maxAttempts\":$attempts}")"
  python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$resp"
}

wait_terminal() {
  local id="$1" budget="$2" waited=0 status=""
  while (( waited < budget )); do
    status="$(auth "$BASE/api/v1/runs/$id" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])")"
    case "$status" in
      succeeded|failed|cancelled) echo "$status"; return 0 ;;
    esac
    sleep 2
    waited=$((waited + 2))
  done
  echo "TIMEOUT(waited ${budget}s, last: $status)"
  return 1
}

wait_bound_active() {
  local id="$1" budget="$2" waited=0 status=""
  while (( waited < budget )); do
    status="$(auth "$BASE/api/v1/runs/$id/sessions" | python3 -c "
import json,sys
ss=json.load(sys.stdin)['sessions']
print((ss[0].get('status','')+' '+ss[0].get('sessionId','')) if ss else '')")"
    if [[ "$status" == active\ * && "${status#active }" != "" ]]; then
      echo "$status"; return 0
    fi
    sleep 2
    waited=$((waited + 2))
  done
  echo "TIMEOUT(waited ${budget}s, last: $status)"
  return 1
}

echo "== start Core + Worker (deterministic stub lane) =="
start_core
write_stub ok
start_worker

echo "-- empty projection and scoped auth --"
auth "$BASE/api/v1/sessions" | grep -q '"sessions":\[\]' || { echo "[FAIL] fresh Core must expose an empty session list"; exit 1; }
[[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $WTOKEN" "$BASE/api/v1/sessions")" == "401" ]] || { echo "[FAIL] worker token must not read the operator session projection"; exit 1; }
[[ "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/api/v1/sessions")" == "401" ]] || { echo "[FAIL] unauthenticated session projection must be rejected"; exit 1; }
[[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/sessions?status=bogus")" == "400" ]] || { echo "[FAIL] invalid status filter must be 400"; exit 1; }
LONG_PROVIDER="$(python3 -c 'print("x"*65)')"
[[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/sessions?provider=$LONG_PROVIDER")" == "400" ]] || { echo "[FAIL] over-long provider filter must be 400"; exit 1; }
echo "[PASS] empty state and scoped read-only auth"

echo "-- browser: live success, evidence, reload, reconnect --"
SUCCESS_JSON="$TMP/s3c-success.json"
BASE_URL="$BASE" FACTORY_API_TOKEN="$TOKEN" CHROMIUM_EXE="$CHROMIUM_EXE" \
  S3C_PHASE=success S3C_EVIDENCE_DIR="$TMP/s3c-evidence" \
  node "$WEB_SRC/e2e-browser/s3c-journey.mjs" | tee "$SUCCESS_JSON"
RUN_SUCCESS="$(python3 -c "import json,sys; print(json.load(sys.stdin)['runId'])" < "$SUCCESS_JSON")"
SESSION_SUCCESS="$(python3 -c "import json,sys; print(json.load(sys.stdin)['sessionId'])" < "$SUCCESS_JSON")"
EVENTS_SUCCESS="$(python3 -c "import json,sys; print(json.load(sys.stdin)['eventCount'])" < "$SUCCESS_JSON")"
[[ -n "$RUN_SUCCESS" && -n "$SESSION_SUCCESS" ]] || { echo "[FAIL] browser journey did not return real identities"; exit 1; }

python3 - "$BASE" "$TOKEN" "$RUN_SUCCESS" "$SESSION_SUCCESS" <<'PY'
import json, sys, urllib.request
base, token, run_id, session_id = sys.argv[1:5]
def get(path):
    req = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=20) as response:
        return json.load(response)
run = get(f"/api/v1/runs/{run_id}")["run"]
assert run["status"] == "succeeded", run["status"]
result = run["result"]
assert result["testExit"] == 0, result
assert result["formatExit"] == 0, result
assert result["diffBytes"] > 0, result
assert result["provider"] == "codex" and result["model"] == "gpt-6-luna"
assert result["paseoMode"] == "full-access"
assert "diffExcerpt" not in result, "source excerpt must not be persisted"
sessions = get(f"/api/v1/runs/{run_id}/sessions")["sessions"]
assert len(sessions) == 1, sessions
binding = sessions[0]
assert binding["status"] == "succeeded" and binding["sessionId"] == session_id, binding
types = [event["eventType"] for event in get(f"/api/v1/runs/{run_id}/events")["events"]]
for want in ("RunQueued", "RunLeased", "RunStarted", "PaseoSessionIntended",
             "PaseoSessionActive", "PaseoSessionSucceeded", "RunCompleted"):
    assert want in types, types
print("[PASS] success Run: real binding, verified evidence, complete lifecycle, no source excerpt")
PY

echo "-- multi-attempt: failed generation and successful retry stay distinct --"
write_stub fail-once
RUN_MULTI="$(queue_paseo 600 2)"
ST_MULTI="$(wait_terminal "$RUN_MULTI" 240)"
[[ "$ST_MULTI" == "succeeded" ]] || { echo "[FAIL] multi-attempt run: want succeeded, got $ST_MULTI"; tail -60 "$WORKER_LOG"; exit 1; }
python3 - "$BASE" "$TOKEN" "$RUN_MULTI" <<'PY'
import json, sys, urllib.request
base, token, run_id = sys.argv[1:4]
def get(path):
    req = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=20) as response:
        return json.load(response)
run = get(f"/api/v1/runs/{run_id}")["run"]
assert run["status"] == "succeeded", run["status"]
assert run["attemptCount"] == 2, run["attemptCount"]
sessions = get(f"/api/v1/runs/{run_id}/sessions")["sessions"]
assert [s["seq"] for s in sessions] == [1, 2], sessions
assert [s["status"] for s in sessions] == ["failed", "succeeded"], sessions
assert [s["leaseGeneration"] for s in sessions] == [1, 2], sessions
assert sessions[0]["attemptId"] != sessions[1]["attemptId"], sessions
assert sessions[0].get("sessionId", "") == "", sessions[0]
assert sessions[1].get("sessionId", ""), sessions[1]
types = [event["eventType"] for event in get(f"/api/v1/runs/{run_id}/events")["events"]]
for want in ("RunFailed", "PaseoSessionFailed", "PaseoSessionSucceeded", "RunCompleted"):
    assert want in types, types
print("[PASS] two Attempts keep separate generations and binding outcomes")
PY
MULTI_JSON="$TMP/s3c-multi.json"
BASE_URL="$BASE" FACTORY_API_TOKEN="$TOKEN" CHROMIUM_EXE="$CHROMIUM_EXE" \
  S3C_PHASE=multi S3C_EVIDENCE_DIR="$TMP/s3c-evidence" \
  S3C_MULTI_RUN_ID="$RUN_MULTI" \
  node "$WEB_SRC/e2e-browser/s3c-journey.mjs" | tee "$MULTI_JSON"

echo "-- cancellation of a bound session becomes an honest orphan --"
write_stub hang-logs
RUN_ORPHAN="$(queue_paseo 600)"
BOUND="$(wait_bound_active "$RUN_ORPHAN" 90)"
[[ "$BOUND" == active\ * ]] || { echo "[FAIL] session never bound: $BOUND"; tail -40 "$WORKER_LOG"; exit 1; }
SESSION_ORPHAN="${BOUND#active }"
echo "-- browser: live active session while the run is still executing --"
LIVE_JSON="$TMP/s3c-live.json"
BASE_URL="$BASE" FACTORY_API_TOKEN="$TOKEN" CHROMIUM_EXE="$CHROMIUM_EXE" \
  S3C_PHASE=live S3C_EVIDENCE_DIR="$TMP/s3c-evidence" \
  S3C_LIVE_RUN_ID="$RUN_ORPHAN" S3C_LIVE_SESSION_ID="$SESSION_ORPHAN" \
  node "$WEB_SRC/e2e-browser/s3c-journey.mjs" | tee "$LIVE_JSON"
auth -X POST "$BASE/api/v1/runs/$RUN_ORPHAN/cancel" -H 'Content-Type: application/json' -d '{}' >/dev/null
ST_ORPHAN="$(wait_terminal "$RUN_ORPHAN" 90)"
[[ "$ST_ORPHAN" == "cancelled" ]] || { echo "[FAIL] want cancelled, got $ST_ORPHAN"; exit 1; }

python3 - "$BASE" "$TOKEN" "$RUN_ORPHAN" "$SESSION_ORPHAN" <<'PY'
import json, sys, urllib.request
base, token, run_id, session_id = sys.argv[1:5]
def get(path):
    req = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=20) as response:
        return json.load(response)
run = get(f"/api/v1/runs/{run_id}")["run"]
assert run["status"] == "cancelled", run["status"]
binding = get(f"/api/v1/runs/{run_id}/sessions")["sessions"][0]
assert binding["status"] == "orphaned", binding
assert binding["sessionId"] == session_id and binding["sessionId"], binding
types = [event["eventType"] for event in get(f"/api/v1/runs/{run_id}/events")["events"]]
assert "PaseoSessionIntended" in types and "PaseoSessionActive" in types, types
assert "PaseoSessionOrphaned" in types, types
assert "PaseoSessionCancelled" not in types, "Core cancellation must not claim a stop"
assert "RunCancelled" in types, types
print("[PASS] cancelled bound session -> orphaned, no fabricated stop")
PY

echo "-- restart Core: history survives --"
stop_worker
stop_core
start_core
start_worker
sleep 2
python3 - "$BASE" "$TOKEN" "$RUN_SUCCESS" "$RUN_ORPHAN" "$RUN_MULTI" "$EVENTS_SUCCESS" <<'PY'
import json, sys, urllib.request
base, token, success_run, orphan_run, multi_run, event_count = sys.argv[1:7]
def get(path):
    req = urllib.request.Request(base + path, headers={"Authorization": "Bearer " + token})
    with urllib.request.urlopen(req, timeout=20) as response:
        return json.load(response)
sessions = get("/api/v1/sessions")["sessions"]
assert len(sessions) == 4, sessions
by_run = {}
for session in sessions:
    by_run.setdefault(session["runId"], []).append(session)
assert by_run[success_run][0]["status"] == "succeeded", by_run[success_run]
assert by_run[orphan_run][0]["status"] == "orphaned", by_run[orphan_run]
assert [s["status"] for s in sorted(by_run[multi_run], key=lambda s: s["seq"])] == ["failed", "succeeded"], by_run[multi_run]
assert len(get(f"/api/v1/runs/{success_run}/events")["events"]) == int(event_count)
events = get("/api/v1/events?after=0&limit=500")["events"]
seqs = [event["globalSeq"] for event in events]
assert seqs == list(range(1, len(seqs) + 1)), "replay must be contiguous"
print("[PASS] Core restart retains all bindings, history and a contiguous ledger")
PY

echo "-- browser: historical orphan state after Core restart --"
ORPHAN_JSON="$TMP/s3c-orphan.json"
BASE_URL="$BASE" FACTORY_API_TOKEN="$TOKEN" CHROMIUM_EXE="$CHROMIUM_EXE" \
  S3C_PHASE=orphan S3C_EVIDENCE_DIR="$TMP/s3c-evidence" \
  S3C_ORPHAN_RUN_ID="$RUN_ORPHAN" S3C_ORPHAN_SESSION_ID="$SESSION_ORPHAN" \
  node "$WEB_SRC/e2e-browser/s3c-journey.mjs" | tee "$ORPHAN_JSON"

echo
echo "S3-C end-to-end acceptance: ALL PHASES DONE (stubbed Paseo; no paid sessions)"
echo "evidence: $TMP/s3c-evidence"
echo "success run: $RUN_SUCCESS session: $SESSION_SUCCESS"
echo "orphan run: $RUN_ORPHAN session: $SESSION_ORPHAN"
