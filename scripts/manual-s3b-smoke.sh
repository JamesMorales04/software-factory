#!/usr/bin/env bash
# Manual single-model Factory smoke test (F1-S3-B follow-up for the owner).
#
# Runs exactly ONE real Paseo coding session through the actual Factory
# Core + Worker (never direct `paseo` as a substitute) and displays the
# truthful outcome. Default model: OpenCode Go DeepSeek V4.1 Flash
# (any of the three S3 allowlisted triples can be selected; exactly one
# paid session is ever started: maxAttempts=1, one queued Run).
#
# What this proves — and what it does not:
#   This is a BOUNDED NODE FIXTURE test, not arbitrary-repository
#   development. The Worker seeds a tiny disposable project (add.js,
#   add.test.js, package.json), the agent appends a multiply function,
#   and the Worker verifies a real git diff plus passing format/tests.
#   It does not support pointing the Factory at an existing repository
#   yet; task text, fixture shape and evidence checks are fixed by S3-B.
#
# Usage (any shell; invokes bash explicitly for fish users):
#   bash scripts/manual-s3b-smoke.sh
#   PROVIDER=codex MODEL=gpt-6-luna MODE=full-access bash scripts/manual-s3b-smoke.sh
#   DRY_RUN=1 bash scripts/manual-s3b-smoke.sh   # harness check only: builds,
#                                                # starts Core, health-checks,
#                                                # then exits without queueing
#                                                # (spends nothing)
#
# Cost: exactly one paid session unless DRY_RUN=1 (zero).
# Time: up to ~11 minutes bounded (600 s Run budget + margin).
set -euo pipefail

PROVIDER="${PROVIDER:-opencode}"
MODEL="${MODEL:-opencode-go/deepseek-v4.1-flash}"
MODE="${MODE:-build}"
DRY_RUN="${DRY_RUN:-0}"
WAIT_BUDGET="${WAIT_BUDGET:-660}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SMOKE="$(mktemp -d "${TMPDIR:-/tmp}/factory-manual-smoke-XXXXXX")"
BIN_CORE="$SMOKE/factory-core"
BIN_WORKER="$SMOKE/factory-worker"
DB="$SMOKE/factory.db"
CORE_LOG="$SMOKE/core.log"
WORKER_LOG="$SMOKE/worker.log"
CORE_PID=""
WORKER_PID=""
TASK_TEXT="Append a pure function multiply(a, b) with a JSDoc comment to add.js and export it alongside add (keep add unchanged)."

# Owned PIDs only: TERM each recorded process and wait for it. Never
# pattern-kills, so unrelated services are never touched.
stop_owned() {
  if [[ -n "${WORKER_PID:-}" ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill "$WORKER_PID" 2>/dev/null || true
    wait "$WORKER_PID" 2>/dev/null || true
  fi
  WORKER_PID=""
  if [[ -n "${CORE_PID:-}" ]] && kill -0 "$CORE_PID" 2>/dev/null; then
    kill "$CORE_PID" 2>/dev/null || true
    wait "$CORE_PID" 2>/dev/null || true
  fi
  CORE_PID=""
}
trap stop_owned EXIT

pick_port() { # prints the first free port in 8476..8486
  local p
  for p in $(seq 8476 8486); do
    if ! (echo > "/dev/tcp/127.0.0.1/$p") 2>/dev/null; then
      echo "$p"
      return 0
    fi
  done
  echo "no free port in 8476..8486" >&2
  return 1
}

echo "== manual Factory smoke: $PROVIDER / $MODEL ($MODE) =="
echo "workspace: $SMOKE (kept for inspection afterwards)"
echo "== build core + worker =="
(cd "$ROOT/core" && go build -o "$BIN_CORE" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$BIN_WORKER" ./cmd/factory-worker)

PORT="$(pick_port)"
BASE="http://127.0.0.1:${PORT}"
API_T="$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WORKER_T="$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
# Tokens live in shell variables only and travel in HTTP headers.
# They are never printed, logged, or placed on command lines.

echo "== start Core on $BASE =="
env FACTORY_API_TOKEN="$API_T" FACTORY_WORKER_TOKEN="$WORKER_T" \
  "$BIN_CORE" --addr "127.0.0.1:${PORT}" --db "$DB" >"$CORE_LOG" 2>&1 &
CORE_PID=$!
READY=0
for _ in $(seq 1 60); do
  if curl -sf -o /dev/null -H "Authorization: Bearer $API_T" "$BASE/api/v1/health"; then
    READY=1
    break
  fi
  sleep 1
done
if [[ "$READY" != "1" ]]; then
  echo "Core did not become healthy; last log lines:"
  tail -20 "$CORE_LOG"
  exit 1
fi
echo "Core healthy."

if [[ "$DRY_RUN" == "1" ]]; then
  echo "DRY_RUN=1: harness verified (build, port, Core start, health). No Run queued, nothing spent."
  exit 0
fi

echo "== queue exactly ONE paseo-task Run (maxAttempts=1) =="
RUN="$(curl -sf -H "Authorization: Bearer $API_T" -H 'Content-Type: application/json' \
  -d "{\"workloadKind\":\"paseo-task\",\"workloadInput\":{\"task\":\"$TASK_TEXT\"},\"executionMode\":\"trusted-local\",\"provider\":\"$PROVIDER\",\"model\":\"$MODEL\",\"paseoMode\":\"$MODE\",\"timeoutSecs\":600,\"maxAttempts\":1}" \
  "$BASE/api/v1/runs" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])")"
echo "run: $RUN"

echo "== start Worker (single slot, explicit local fixture mode) =="
env FACTORY_WORKER_TOKEN="$WORKER_T" \
  "$BIN_WORKER" --core "$BASE" --worker-id local-1 --exec-mode local >"$WORKER_LOG" 2>&1 &
WORKER_PID=$!
sleep 2
if ! kill -0 "$WORKER_PID" 2>/dev/null; then
  echo "Worker failed to start; last log lines:"
  tail -20 "$WORKER_LOG"
  exit 1
fi

echo "== wait for the terminal outcome (bounded ${WAIT_BUDGET}s) =="
STATUS=""
for _ in $(seq 1 $((WAIT_BUDGET / 10))); do
  STATUS="$(curl -sf -H "Authorization: Bearer $API_T" "$BASE/api/v1/runs/$RUN" \
    | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])")"
  echo "  run status: $STATUS"
  case "$STATUS" in
    succeeded|failed|cancelled) break ;;
  esac
  sleep 10
done
case "$STATUS" in
  succeeded|failed|cancelled) ;;
  *) echo "TIMEOUT: run did not settle within ${WAIT_BUDGET}s (worker stopped by trap; Core expiry owns the Attempt)." >&2; exit 1 ;;
esac

echo
echo "== Run =="
curl -sf -H "Authorization: Bearer $API_T" "$BASE/api/v1/runs/$RUN" | python3 -c "
import json,sys
r=json.load(sys.stdin)['run']
print('status       :', r['status'])
print('executionMode:', r['executionMode'], '(TRUSTED_LOCAL = broad local access, not isolated)')
print('provider     :', r['provider'])
print('model        :', r['model'])
print('paseoMode    :', r['paseoMode'])
res=r.get('result') or {}
for k in ('diffBytes','testExit','formatExit','nudges','cleanup','timelineSteps'):
    if k in res: print(('%-13s:' % k), res[k])
print('diff excerpt :')
print((res.get('diffExcerpt') or '(none)')[:1000])
if r.get('error'): print('error        :', r['error'])
"

echo
echo "== Session binding =="
curl -sf -H "Authorization: Bearer $API_T" "$BASE/api/v1/runs/$RUN/sessions" | python3 -c "
import json,sys
ss=json.load(sys.stdin)['sessions']
print('bindings:', len(ss))
for b in ss:
    print('status     :', b['status'])
    print('workspaceId:', b.get('workspaceId'))
    print('sessionId  :', b.get('sessionId'))
"

echo
echo "== Events =="
curl -sf -H "Authorization: Bearer $API_T" "$BASE/api/v1/runs/$RUN/events" | python3 -c "
import json,sys
print(' '.join(e['eventType'] for e in json.load(sys.stdin)['events']))"

if [[ "$STATUS" != "succeeded" ]]; then
  echo
  echo "Run did not succeed. The Worker retains the Attempt workspace for inspection:"
  grep -h "retaining paseo-task workspace" "$WORKER_LOG" | tail -2 || true
  exit 1
fi
echo
echo "MANUAL SMOKE PASS: one real $MODEL session settled the Run through Factory Core + Worker."
echo "Evidence kept at: $SMOKE (database, core.log, worker.log)"
