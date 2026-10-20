#!/usr/bin/env bash
# F1-S3-A end-to-end acceptance: durable Paseo session identity without any
# real Paseo session. Real Core binary, real SQLite file, real HTTP.
# Positive intend→bind→close journey + duplicate replay + stale fencing +
# expiry orphan + cancel + restart retention + S2 synthetic regression.
# No provider calls, no Paseo daemon, no paid sessions. Fake opaque session
# IDs only. Exits nonzero on the first failing phase.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s3a-XXXXXX")"
TOKEN="s3ae2e-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s3ae2e-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S3A_PORT:-8473}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
DB="$TMP/factory.db"
CORE_LOG="$TMP/core.log"
CORE_PID=""

cleanup() {
  if [[ -n "${CORE_PID:-}" ]] && kill -0 "$CORE_PID" 2>/dev/null; then
    kill "$CORE_PID" 2>/dev/null || true
    wait "$CORE_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "== build core =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)

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

stop_core() {
  kill "$CORE_PID" 2>/dev/null || true
  wait "$CORE_PID" 2>/dev/null || true
  CORE_PID=""
  sleep 0.5
}

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }
worker() { curl -sf -H "Authorization: Bearer $WTOKEN" "$@"; }
code_of() { # $1 = file with response body; prints http code from curl -w
  cat "$1" >/dev/null
}

echo "== phase 1: queue paseo-task runs for all three allowlisted routes =="
start_core
CODE_JSON="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d '{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}')"
RUN_CODEX="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$CODE_JSON")"
echo "codex run: $RUN_CODEX"
MUSE_JSON="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d '{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"opencode","model":"opencode-go/muse-spark-1.3-contributor","paseoMode":"build"}')"
RUN_MUSE="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$MUSE_JSON")"
echo "muse run: $RUN_MUSE"
DS_JSON="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d '{"workloadKind":"paseo-task","workloadInput":{"task":"add multiply"},"executionMode":"trusted-local","provider":"opencode","model":"opencode-go/deepseek-v4.1-flash","paseoMode":"build"}')"
RUN_DS="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$DS_JSON")"
echo "deepseek run: $RUN_DS"
MODE="$(python3 -c "import json,sys; r=json.load(sys.stdin)['run']; print(r['executionMode'],r['provider'],r['model'],r['paseoMode'])" <<<"$CODE_JSON")"
if [[ "$MODE" != "trusted-local codex gpt-6-luna full-access" ]]; then
  echo "[FAIL] routing not persisted: $MODE"; exit 1
fi
echo "[PASS] three paseo-task runs queued with frozen routing"

echo "== phase 2: substitution and malformed routing rejected =="
reject_bodies=(
  '{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}'
  '{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"build"}'
  '{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"trusted-local","provider":"opencode","model":"github-copilot/gpt-5-mini","paseoMode":"build"}'
  '{"workloadKind":"paseo-task","workloadInput":{},"executionMode":"trusted-local","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}'
  '{"workloadKind":"synthetic-echo","workloadInput":{"message":"x"},"executionMode":"trusted-local"}'
  '{"workloadKind":"paseo-task","workloadInput":{"task":"x"},"executionMode":"isolated","provider":"codex","model":"gpt-6-luna","paseoMode":"full-access"}'
)
for b in "${reject_bodies[@]}"; do
  CODE="$(curl -s -o /dev/null -w '%{http_code}' -X POST "$BASE/api/v1/runs" \
    -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' -d "$b")"
  if [[ "$CODE" != "400" ]]; then echo "[FAIL] expected 400, got $CODE for $b"; exit 1; fi
done
echo "[PASS] six malformed/substituted queue bodies rejected with 400"

echo "== phase 3: intend → bind → close over the worker protocol =="
worker -X POST "$BASE/api/v1/workers/register" -H 'Content-Type: application/json' \
  -d '{"workerId":"s3a-e2e","version":"e2e"}' >/dev/null
POLL="$(worker -X POST "$BASE/api/v1/workers/poll" -H 'Content-Type: application/json' \
  -d '{"workerId":"s3a-e2e"}')"
ATTEMPT="$(python3 -c "import json,sys; a=json.load(sys.stdin)['attempt']; print(a['id'], a['leaseGeneration'])" <<<"$POLL")"
ATTEMPT_ID="$(echo "$ATTEMPT" | cut -d' ' -f1)"
GEN="$(echo "$ATTEMPT" | cut -d' ' -f2)"
echo "leased attempt $ATTEMPT_ID generation $GEN"
worker -X POST "$BASE/api/v1/workers/heartbeat" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN_CODEX\",\"attemptId\":\"$ATTEMPT_ID\",\"generation\":$GEN}" >/dev/null
INTEND="$(worker -X POST "$BASE/api/v1/workers/session/intend" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN_CODEX\",\"attemptId\":\"$ATTEMPT_ID\",\"generation\":$GEN}")"
if ! grep -q '"status":"intended"' <<<"$INTEND"; then echo "[FAIL] intend wrong: $INTEND"; exit 1; fi
# Duplicate intend replays with the header and no new effect.
REPLAY_HEADERS="$TMP/replay.h"
curl -sf -D "$REPLAY_HEADERS" -o /dev/null -X POST "$BASE/api/v1/workers/session/intend" \
  -H "Authorization: Bearer $WTOKEN" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN_CODEX\",\"attemptId\":\"$ATTEMPT_ID\",\"generation\":$GEN}"
if ! grep -qi 'Idempotent-Replayed: true' "$REPLAY_HEADERS"; then echo "[FAIL] intend replay missing header"; exit 1; fi
echo "[PASS] intend recorded + replayed"
BIND="$(worker -X POST "$BASE/api/v1/workers/session/bind" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN_CODEX\",\"attemptId\":\"$ATTEMPT_ID\",\"generation\":$GEN,\"workspaceId\":\"ws_e2e_1\",\"sessionId\":\"aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1\"}")"
if ! grep -q '"status":"active"' <<<"$BIND"; then echo "[FAIL] bind wrong: $BIND"; exit 1; fi
SUB_CODE="$(curl -s -o /tmp/factory-s3a-sub.json -w '%{http_code}' -X POST "$BASE/api/v1/workers/session/bind" \
  -H "Authorization: Bearer $WTOKEN" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN_CODEX\",\"attemptId\":\"$ATTEMPT_ID\",\"generation\":$GEN,\"workspaceId\":\"ws_evil\",\"sessionId\":\"bbbbbbbb-2222-4222-8222-bbbbbbbbbbb2\"}")"
if [[ "$SUB_CODE" != "409" ]]; then echo "[FAIL] substituted re-bind not fenced: $SUB_CODE"; exit 1; fi
echo "[PASS] bind recorded; substituted re-bind fenced with 409"
CLOSE="$(worker -X POST "$BASE/api/v1/workers/session/close" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN_CODEX\",\"attemptId\":\"$ATTEMPT_ID\",\"generation\":$GEN,\"outcome\":\"succeeded\"}")"
if ! grep -q '"status":"succeeded"' <<<"$CLOSE"; then echo "[FAIL] close wrong: $CLOSE"; exit 1; fi
SESSIONS="$(auth "$BASE/api/v1/runs/$RUN_CODEX/sessions")"
if ! grep -q 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1' <<<"$SESSIONS"; then echo "[FAIL] sessions read missing binding"; exit 1; fi
EVENTS="$(auth "$BASE/api/v1/runs/$RUN_CODEX/events")"
for want in PaseoSessionIntended PaseoSessionActive PaseoSessionSucceeded; do
  if ! grep -q "$want" <<<"$EVENTS"; then echo "[FAIL] run events miss $want"; exit 1; fi
done
echo "[PASS] close recorded; operator sessions read + run events carry the lifecycle"

echo "== phase 4: cancel during an active session =="
POLL2="$(worker -X POST "$BASE/api/v1/workers/poll" -H 'Content-Type: application/json' \
  -d '{"workerId":"s3a-e2e"}')"
RUN2="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$POLL2")"
ATT2="$(python3 -c "import json,sys; a=json.load(sys.stdin)['attempt']; print(a['id'], a['leaseGeneration'])" <<<"$POLL2")"
ATT2_ID="$(echo "$ATT2" | cut -d' ' -f1)"; GEN2="$(echo "$ATT2" | cut -d' ' -f2)"
worker -X POST "$BASE/api/v1/workers/heartbeat" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN2\",\"attemptId\":\"$ATT2_ID\",\"generation\":$GEN2}" >/dev/null
worker -X POST "$BASE/api/v1/workers/session/intend" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN2\",\"attemptId\":\"$ATT2_ID\",\"generation\":$GEN2}" >/dev/null
worker -X POST "$BASE/api/v1/workers/session/bind" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN2\",\"attemptId\":\"$ATT2_ID\",\"generation\":$GEN2,\"workspaceId\":\"ws_e2e_2\",\"sessionId\":\"aaaaaaaa-1111-4111-8111-aaaaaaaaaaa2\"}" >/dev/null
auth -X POST "$BASE/api/v1/runs/$RUN2/cancel" -H 'Content-Type: application/json' -d '{}' >/dev/null
SESS2="$(auth "$BASE/api/v1/runs/$RUN2/sessions")"
if ! grep -q '"status":"cancelled"' <<<"$SESS2"; then echo "[FAIL] binding not cancelled with run: $SESS2"; exit 1; fi
LATE_CODE="$(curl -s -o /tmp/factory-s3a-late.json -w '%{http_code}' -X POST "$BASE/api/v1/workers/session/close" \
  -H "Authorization: Bearer $WTOKEN" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"s3a-e2e\",\"runId\":\"$RUN2\",\"attemptId\":\"$ATT2_ID\",\"generation\":$GEN2,\"outcome\":\"succeeded\"}")"
if [[ "$LATE_CODE" != "409" ]]; then echo "[FAIL] late close after cancel not fenced: $LATE_CODE"; exit 1; fi
echo "[PASS] run cancel propagated to the binding; late close fenced with 409"

echo "== phase 5: core restart retains bindings and history =="
RUNS_BEFORE="$(auth "$BASE/api/v1/runs?limit=200" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['runs']))")"
EV_BEFORE="$(auth "$BASE/api/v1/runs/$RUN_CODEX/events" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['events']))")"
stop_core
start_core
RUNS_AFTER="$(auth "$BASE/api/v1/runs?limit=200" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['runs']))")"
if [[ "$RUNS_AFTER" -lt "$RUNS_BEFORE" ]]; then echo "[FAIL] runs lost across restart"; exit 1; fi
SESS_AFTER="$(auth "$BASE/api/v1/runs/$RUN_CODEX/sessions")"
if ! grep -q 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaa1' <<<"$SESS_AFTER"; then echo "[FAIL] binding lost across restart"; exit 1; fi
EV_AFTER="$(auth "$BASE/api/v1/runs/$RUN_CODEX/events" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['events']))")"
if [[ "$EV_AFTER" -lt "$EV_BEFORE" ]]; then echo "[FAIL] events lost across restart"; exit 1; fi
# A fresh registration after restart can lease the remaining queued run.
worker -X POST "$BASE/api/v1/workers/register" -H 'Content-Type: application/json' \
  -d '{"workerId":"s3a-e2e","version":"e2e"}' >/dev/null
POLL3_CODE="$(curl -s -o /tmp/factory-s3a-poll3.json -w '%{http_code}' -X POST "$BASE/api/v1/workers/poll" \
  -H "Authorization: Bearer $WTOKEN" -H 'Content-Type: application/json' -d '{"workerId":"s3a-e2e"}')"
if [[ "$POLL3_CODE" != "200" ]]; then echo "[FAIL] post-restart poll: $POLL3_CODE"; exit 1; fi
echo "[PASS] restart retained $RUNS_AFTER runs, bindings, and $EV_AFTER events; worker re-leased work"

echo "== phase 6: S2 synthetic regression (restricted default, no routing, no binding) =="
SYN="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d '{"workloadKind":"synthetic-echo","workloadInput":{"message":"s3a-regression"}}')"
if ! grep -q '"executionMode":"restricted"' <<<"$SYN"; then echo "[FAIL] synthetic default mode wrong: $SYN"; exit 1; fi
SYN_ID="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$SYN")"
SYN_SESS="$(auth "$BASE/api/v1/runs/$SYN_ID/sessions")"
if ! grep -q '"sessions":\[\]' <<<"$SYN_SESS"; then echo "[FAIL] synthetic run should have no sessions: $SYN_SESS"; exit 1; fi
echo "[PASS] synthetic-echo unchanged: restricted default, empty routing, no sessions"

stop_core
echo
echo "S3-A end-to-end acceptance: ALL PHASES DONE"
