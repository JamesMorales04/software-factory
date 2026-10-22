#!/usr/bin/env bash
# F1-S3-B end-to-end acceptance: real Factory Core + real Worker binary
# executing real Paseo sessions (live lane) plus deterministic worker-level
# failure probes through a stubbed `paseo` CLI (stub lane, CI-safe).
#
# Stub lane (always runs; no daemon, no provider calls, no paid sessions):
#   s1 startup failure -> failed run, failed binding, no success recorded
#   s2 identity mismatch -> failed run, orphaned binding, orphan queue lists it
#   s3 operator cancel mid-session -> cancelled run, orphaned binding
#   s4 lease-expiry hang -> failed run, orphaned binding
#   s5 stub-success full lifecycle -> succeeded run, succeeded binding,
#      verified evidence (routing, diff, tests), cleanup verified
#   s6 synthetic-echo regression (restricted default, no binding)
# Live lane (disabled by default; explicit authorization required):
#   one real Paseo session per allowlisted route through the Factory Worker,
#   with effective provider/model/mode/Cwd asserted from session evidence,
#   a real git diff, passing format/tests, truthful terminal settlement and
#   owned-resource cleanup. Exactly one paid session per route.
# Exits nonzero on the first failing phase.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s3b-XXXXXX")"
TOKEN="s3be2e-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s3be2e-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S3B_PORT:-8474}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
BIN_WORKER="$TMP/factory-worker"
DB="$TMP/factory.db"
CORE_LOG="$TMP/core.log"
WORKER_LOG="$TMP/worker.log"
CORE_PID=""
WORKER_PID=""
LIVE="${FACTORY_S3B_LIVE:-0}"
if [[ "$LIVE" != "0" && "$LIVE" != "1" ]]; then
  echo "[BLOCKED] FACTORY_S3B_LIVE must be 0 or 1"; exit 2
fi
STUBBIN="$TMP/stubbin"
STUB_CONF="$TMP/stub.conf"
TASK_TEXT="Append a pure function multiply(a, b) with a JSDoc comment to add.js and export it alongside add (keep add unchanged)."

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

echo "== build core + worker =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)

start_core() {
  env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_CORE" --addr "127.0.0.1:${PORT}" --db "$DB" >"$CORE_LOG" 2>&1 &
  CORE_PID=$!
  for _ in $(seq 1 50); do
    if curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health"; then
      return 0
    fi
    sleep 0.2
  done
  echo "core failed to start; log:"; tail -30 "$CORE_LOG"
  return 1
}

# start_worker [extra-path]: extra-path, when given, is prepended to PATH
# for the worker only (stub lane). The script keeps the real PATH.
start_worker() {
  local extra="${1:-}"
  if [[ -n "$extra" ]]; then
    env PATH="$extra:$PATH" FACTORY_WORKER_TOKEN="$WTOKEN" \
      "$BIN_WORKER" --core "$BASE" --worker-id local-1 --exec-mode local >"$WORKER_LOG" 2>&1 &
  else
    env FACTORY_WORKER_TOKEN="$WTOKEN" \
      "$BIN_WORKER" --core "$BASE" --worker-id local-1 --exec-mode local >"$WORKER_LOG" 2>&1 &
  fi
  WORKER_PID=$!
  sleep 2
  if ! kill -0 "$WORKER_PID" 2>/dev/null; then
    echo "worker failed to start; log:"; tail -30 "$WORKER_LOG"
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

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }
worker() { curl -sf -H "Authorization: Bearer $WTOKEN" "$@"; }

queue_paseo() { # provider model mode timeout maxAttempts -> run id
  local resp
  resp="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
    -d "{\"workloadKind\":\"paseo-task\",\"workloadInput\":{\"task\":\"$TASK_TEXT\"},\"executionMode\":\"trusted-local\",\"provider\":\"$1\",\"model\":\"$2\",\"paseoMode\":\"$3\",\"timeoutSecs\":$4,\"maxAttempts\":$5}")"
  python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$resp"
}

run_status() { # runid -> status
  auth "$BASE/api/v1/runs/$1" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])"
}

wait_terminal() { # runid timeout_secs -> final status
  local id="$1" budget="$2" waited=0 status=""
  while (( waited < budget )); do
    status="$(run_status "$id")"
    case "$status" in
      succeeded|failed|cancelled) echo "$status"; return 0 ;;
    esac
    sleep 5
    waited=$((waited + 5))
  done
  echo "TIMEOUT(waited ${budget}s, last: $status)"
  return 1
}

binding_status() { # runid -> "status workspaceId sessionId"
  auth "$BASE/api/v1/runs/$1/sessions" | python3 -c "
import json,sys
ss=json.load(sys.stdin)['sessions']
b=ss[0] if ss else {}
print(b.get('status',''), b.get('workspaceId',''), b.get('sessionId',''))"
}

# write_stub <mode>: installs a self-contained stub `paseo` whose behavior
# is driven by the baked-in CONF path (modes: fail-run, mismatch, hang, ok).
# ok mode performs a real file edit in the recorded workspace path so the
# full success lifecycle is deterministic without paid sessions.
write_stub() {
  mkdir -p "$STUBBIN"
  printf '%s' "$1" > "$STUB_CONF"
  cat > "$STUBBIN/paseo" <<STUB_EOF
#!/usr/bin/env bash
CONF="$STUB_CONF"
MODE="\$(cat "\$CONF" 2>/dev/null || echo fail-run)"
STATE="\$CONF.state"
next_id() { # prefix -> id
  n=\$(cat "\$CONF.count" 2>/dev/null || echo 0); n=\$((n+1)); echo "\$n" > "\$CONF.count"
  echo "\$1_stub_\$n"
}
record() { echo "\$@" >> "\$STATE"; }
lookup() { # workspace-id -> path
  grep -h "^\$1 " "\$STATE" 2>/dev/null | head -1 | cut -d' ' -f2- || true
}
case "\$1" in
  --version) echo "paseo-stub 0.0.0-s3b" ;;
  workspace)
    case "\$2" in
      create)
        path=""; while (( \$# > 0 )); do case "\$1" in --path) path="\$2"; shift 2;; *) shift;; esac; done
        id="\$(next_id ws)"; record "\$id \$path"; echo "{\"workspaceId\":\"\$id\"}" ;;
      ls) echo '[]' ;;
      archive) exit 0 ;;
    esac ;;
  run)
    if [[ "\$MODE" == "fail-run" ]]; then echo "stub: daemon unreachable" >&2; exit 1; fi
    # Hang mode simulates a session that never settles on its own. exec
    # replaces the shell so cancellation leaves no orphaned descendant
    # holding the captured pipes (a plain sleep would survive the
    # parent's death, keep the pipes open, and stall Wait on EOF).
    if [[ "\$MODE" == "hang" ]]; then exec sleep 120; fi
    ws=""; while (( \$# > 0 )); do case "\$1" in --workspace) ws="\$2"; shift 2;; *) shift;; esac; done
    sid="\$(next_id STUBAGENT)"; record "sess \$sid \$ws"
    if [[ "\$MODE" == "ok" ]]; then
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
    if [[ "\$MODE" == "mismatch" ]]; then
      echo "{\"Id\":\"\$2\",\"Provider\":\"opencode\",\"Model\":\"opencode-go/deepseek-v4.1-flash\",\"Mode\":\"build\",\"Cwd\":\"\$proj\",\"Status\":\"idle\"}"
    else
      echo "{\"Id\":\"\$2\",\"Provider\":\"codex\",\"Model\":\"gpt-6-luna\",\"Mode\":\"full-access\",\"Cwd\":\"\$proj\",\"Status\":\"idle\"}"
    fi ;;
  logs) printf '[User] task\n[Read] add.js\n[Edit] add.js\n[Shell] npm test\n[Text] done\n' ;;
  send|wait|stop|archive) exit 0 ;;
  *) exit 3 ;;
esac
STUB_EOF
  chmod +x "$STUBBIN/paseo"
}

echo "== stub lane: deterministic worker failure/success probes (no paid sessions) =="
start_core
write_stub fail-run
start_worker "$STUBBIN"

echo "-- s1: failed session startup is reported honestly --"
RUN_S1="$(queue_paseo codex gpt-6-luna full-access 360 1)"
ST_S1="$(wait_terminal "$RUN_S1" 150)"
[[ "$ST_S1" == "failed" ]] || { echo "[FAIL] s1: want failed, got $ST_S1"; exit 1; }
B_S1="$(binding_status "$RUN_S1")"
[[ "$B_S1" == failed* ]] || { echo "[FAIL] s1: binding must be failed: $B_S1"; exit 1; }
EV_S1="$(auth "$BASE/api/v1/runs/$RUN_S1/events" | python3 -c "import json,sys; print(' '.join(e['eventType'] for e in json.load(sys.stdin)['events']))")"
grep -q PaseoSessionIntended <<<"$EV_S1" || { echo "[FAIL] s1: missing Intended"; exit 1; }
grep -q PaseoSessionFailed <<<"$EV_S1" || { echo "[FAIL] s1: missing Failed"; exit 1; }
grep -q PaseoSessionSucceeded <<<"$EV_S1" && { echo "[FAIL] s1: must never record success"; exit 1; }
echo "[PASS] s1: startup failure -> failed run, failed binding, no success"

echo "-- s2: wrong identity is orphaned, never substituted --"
write_stub mismatch
RUN_S2="$(queue_paseo codex gpt-6-luna full-access 360 1)"
ST_S2="$(wait_terminal "$RUN_S2" 150)"
[[ "$ST_S2" == "failed" ]] || { echo "[FAIL] s2: want failed, got $ST_S2"; exit 1; }
B_S2="$(binding_status "$RUN_S2")"
[[ "$B_S2" == orphaned* ]] || { echo "[FAIL] s2: binding must be orphaned: $B_S2"; exit 1; }
SESS_S2="$(echo "$B_S2" | cut -d' ' -f3)"
ORPHANS="$(auth "$BASE/api/v1/sessions/orphaned?limit=50")"
grep -q "$SESS_S2" <<<"$ORPHANS" || { echo "[FAIL] s2: session missing from orphan queue"; exit 1; }
echo "[PASS] s2: identity mismatch -> failed run, orphaned binding, orphan queue lists it"

echo "-- s3: operator cancel mid-session orphans honestly --"
write_stub hang
RUN_S3="$(queue_paseo codex gpt-6-luna full-access 600 1)"
for _ in $(seq 1 12); do
  N="$(auth "$BASE/api/v1/runs/$RUN_S3/sessions" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['sessions']))")"
  [[ "$N" -ge 1 ]] && break
  sleep 5
done
[[ "$N" -ge 1 ]] || { echo "[FAIL] s3: worker never intended the session"; exit 1; }
auth -X POST "$BASE/api/v1/runs/$RUN_S3/cancel" -H 'Content-Type: application/json' -d '{}' >/dev/null
ST_S3="$(wait_terminal "$RUN_S3" 90)"
[[ "$ST_S3" == "cancelled" ]] || { echo "[FAIL] s3: want cancelled, got $ST_S3"; exit 1; }
B_S3="$(binding_status "$RUN_S3")"
[[ "$B_S3" == orphaned* ]] || { echo "[FAIL] s3: binding must be orphaned (stop unconfirmed): $B_S3"; exit 1; }
EV_S3="$(auth "$BASE/api/v1/runs/$RUN_S3/events" | python3 -c "import json,sys; print(' '.join(e['eventType'] for e in json.load(sys.stdin)['events']))")"
grep -q PaseoSessionCancelled <<<"$EV_S3" && { echo "[FAIL] s3: Core cancel must not claim the session stopped"; exit 1; }
echo "[PASS] s3: cancel -> cancelled run, orphaned binding, no stopped claim"

echo "-- s4: insufficient lease budget fails closed before spending --"
RUN_S4="$(queue_paseo codex gpt-6-luna full-access 30 1)"
ST_S4="$(wait_terminal "$RUN_S4" 120)"
[[ "$ST_S4" == "failed" ]] || { echo "[FAIL] s4: want failed, got $ST_S4"; exit 1; }
B_S4="$(binding_status "$RUN_S4")"
[[ "$B_S4" == failed* ]] || { echo "[FAIL] s4: binding must be failed without a session: $B_S4"; exit 1; }
echo "[PASS] s4: tiny budget -> failed run, failed binding, no session started"

echo "-- s5: stub-success full lifecycle (deterministic, no paid session) --"
command -v node >/dev/null && command -v npm >/dev/null || { echo "[FAIL] s5: node/npm required"; exit 1; }
write_stub ok
RUN_S5="$(queue_paseo codex gpt-6-luna full-access 360 1)"
ST_S5="$(wait_terminal "$RUN_S5" 390)"
[[ "$ST_S5" == "succeeded" ]] || { echo "[FAIL] s5: want succeeded, got $ST_S5"; tail -30 "$WORKER_LOG"; exit 1; }
B_S5="$(binding_status "$RUN_S5")"
[[ "$B_S5" == succeeded* ]] || { echo "[FAIL] s5: binding must be succeeded: $B_S5"; exit 1; }
RES_S5="$(auth "$BASE/api/v1/runs/$RUN_S5" | python3 -c "import json,sys; r=json.load(sys.stdin)['run']; print(r['provider'],r['model'],r['paseoMode'],r['result']['diffBytes'],r['result']['testExit'],r['result']['formatExit'])")"
[[ "$RES_S5" == "codex gpt-6-luna full-access "* ]] || { echo "[FAIL] s5: routing not persisted: $RES_S5"; exit 1; }
echo "[PASS] s5: stub success -> succeeded run, succeeded binding, evidence $RES_S5"
grep -q "broad local access, not isolated" "$WORKER_LOG" || { echo "[FAIL] s5: worker must label TRUSTED_LOCAL runs"; exit 1; }
echo "[PASS] s5b: worker labels TRUSTED_LOCAL runs as broad local access, never isolated"
stop_worker

echo "-- s6: synthetic-echo regression (restricted default, no binding) --"
start_worker "$STUBBIN"
SYN="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d '{"workloadKind":"synthetic-echo","workloadInput":{"message":"s3b-regression"},"timeoutSecs":60,"maxAttempts":1}')"
SYN_ID="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$SYN")"
ST_SYN="$(wait_terminal "$SYN_ID" 120)"
[[ "$ST_SYN" == "succeeded" ]] || { echo "[FAIL] s6: synthetic must succeed, got $ST_SYN"; exit 1; }
SYN_SESS="$(auth "$BASE/api/v1/runs/$SYN_ID/sessions")"
grep -q '"sessions":\[\]' <<<"$SYN_SESS" || { echo "[FAIL] s6: synthetic must have no sessions"; exit 1; }
echo "[PASS] s6: synthetic-echo unchanged"
stop_worker

if [[ "$LIVE" != "1" ]]; then
  echo
  echo "S3-B stub lane: ALL PHASES DONE (live lane skipped by FACTORY_S3B_LIVE=0)"
  exit 0
fi

echo
echo "== live lane: one real Paseo session per allowlisted route (paid, bounded) =="
echo "Cost guard: exactly one session per route, maxAttempts=1, one nudge max."
python3 "$ROOT/scripts/live-e2e-authorization.py" \
  --campaign F1-S3-B --project default --max-sessions 3 \
  --route codex/gpt-6-luna \
  --route opencode/opencode-go/muse-spark-1.3-contributor \
  --route opencode/opencode-go/deepseek-v4.1-flash \
  --requested-route codex/gpt-6-luna \
  --requested-route opencode/opencode-go/muse-spark-1.3-contributor \
  --requested-route opencode/opencode-go/deepseek-v4.1-flash
start_worker

live_route() { # provider model mode
  local provider="$1" model="$2" mode="$3"
  echo "-- live: $provider/$model ($mode) --"
  local run st res sessions events
  run="$(queue_paseo "$provider" "$model" "$mode" 600 1)"
  echo "run: $run"
  st="$(wait_terminal "$run" 660)"
  [[ "$st" == "succeeded" ]] || { echo "[FAIL] live $model: want succeeded, got $st (run $run)"; return 1; }
  res="$(auth "$BASE/api/v1/runs/$run" | python3 -c "
import json,sys
r=json.load(sys.stdin)['run']['result']
print(r['provider'], r['model'], r['paseoMode'], r['cwd'], r['diffBytes'], r['testExit'], r['formatExit'], r['nudges'])")"
  echo "result: $res"
  python3 - "$res" <<'EOF'
import sys
prov, model, mode, cwd, diff, test, form, nudges = sys.argv[1].split(' ')
assert int(diff) > 0, "empty diff"
assert test == '0' and form == '0', "checks must pass"
assert int(nudges) <= 1, "at most one nudge"
EOF
  sessions="$(auth "$BASE/api/v1/runs/$run/sessions")"
  ws="$(python3 -c "import json,sys; b=json.load(sys.stdin)['sessions'][0]; print(b['workspaceId'], b['sessionId'], b['status'])" <<<"$sessions")"
  echo "binding: $ws"
  grep -q "succeeded" <<<"$ws" || { echo "[FAIL] live $model: binding not succeeded"; return 1; }
  sid="$(echo "$ws" | cut -d' ' -f2)"; wid="$(echo "$ws" | cut -d' ' -f1)"
  events="$(auth "$BASE/api/v1/runs/$run/events" | python3 -c "import json,sys; print(' '.join(e['eventType'] for e in json.load(sys.stdin)['events']))")"
  for want in PaseoSessionIntended PaseoSessionActive PaseoSessionSucceeded RunCompleted; do
    grep -q "$want" <<<"$events" || { echo "[FAIL] live $model: events miss $want"; return 1; }
  done
  if paseo workspace ls --json 2>/dev/null | grep -q "$wid"; then
    echo "[FAIL] live $model: workspace $wid still listed after archive"; return 1
  fi
  if paseo ls --json 2>/dev/null | grep -q "$sid"; then
    echo "[FAIL] live $model: session $sid still active after archive"; return 1
  fi
  echo "evidence: run=$run session=$sid workspace=$wid result=($res)"
  echo "[PASS] live $model: real session, verified evidence, resources cleaned"
}

live_route codex gpt-6-luna full-access
live_route opencode opencode-go/muse-spark-1.3-contributor build
live_route opencode opencode-go/deepseek-v4.1-flash build

stop_worker
echo
echo "S3-B end-to-end acceptance: ALL PHASES DONE"
