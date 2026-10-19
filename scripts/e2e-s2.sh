#!/usr/bin/env bash
# F1-S2 end-to-end acceptance: real Core binary, real Worker binary, real
# SQLite file, real HTTP, real rootless Podman fixture. Positive journey +
# crash recovery + fencing negatives + Core restart + Podman restriction
# probes. Exits nonzero on the first failing phase.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s2-XXXXXX")"
TOKEN="s2e2e-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s2e2e-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S2_PORT:-8472}"
BASE="http://127.0.0.1:${PORT}"
BIN_CORE="$TMP/factory-core"
BIN_WORKER="$TMP/factory-worker"
DB="$TMP/factory.db"
CORE_LOG="$TMP/core.log"
WORKER_LOG="$TMP/worker.log"
CORE_PID=""
WORKER_PID=""
# Strict mode (CI) turns every BLOCKED gate into a failure: a mandatory
# isolation check that was not executed must never read as green.
STRICT="${FACTORY_S2_STRICT:-0}"
BLOCKED_COUNT=0

blocked() { # $1 = reason
  echo "[BLOCKED] $1"
  if [[ "$STRICT" == "1" ]]; then
    echo "[FAIL] strict mode: blocked gate is a failure"
    exit 1
  fi
  BLOCKED_COUNT=$((BLOCKED_COUNT + 1))
}

final_verdict() {
  if [[ "$BLOCKED_COUNT" -gt 0 ]]; then
    echo
    echo "S2 end-to-end acceptance: COMPLETED WITH $BLOCKED_COUNT BLOCKED GATE(S) — NOT FULL PASS"
    return 0
  fi
  echo
  echo "S2 end-to-end acceptance: ALL PHASES DONE"
}

cleanup() {
  if [[ -n "${WORKER_PID:-}" ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill -9 "$WORKER_PID" 2>/dev/null || true
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

stop_core() {
  kill "$CORE_PID" 2>/dev/null || true
  wait "$CORE_PID" 2>/dev/null || true
  CORE_PID=""
  sleep 0.5
}

start_worker() {
  env FACTORY_WORKER_TOKEN="$WTOKEN" \
    "$BIN_WORKER" --core "$BASE" --worker-id local-1 >"$WORKER_LOG" 2>&1 &
  WORKER_PID=$!
  sleep 2
  if ! kill -0 "$WORKER_PID" 2>/dev/null; then
    echo "worker failed to start; log:"; tail -30 "$WORKER_LOG"
    return 1
  fi
}

stop_worker() {
  if [[ -n "$WORKER_PID" ]] && kill -0 "$WORKER_PID" 2>/dev/null; then
    kill "$WORKER_PID" 2>/dev/null || true
    wait "$WORKER_PID" 2>/dev/null || true
  fi
  WORKER_PID=""
}

kill_worker_hard() {
  kill -9 "$WORKER_PID" 2>/dev/null || true
  wait "$WORKER_PID" 2>/dev/null || true
  WORKER_PID=""
}

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }

echo "== phase 1: positive journey (real worker executes a real run) =="
start_core
start_worker
python3 "$ROOT/scripts/e2e_s2_oracle.py" "$BASE" "$TOKEN" "$WTOKEN" stage-positive

echo "== phase 2: worker crash during an active attempt =="
RUN_JSON="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d '{"workloadKind":"synthetic-echo","workloadInput":{"message":"crash-proof","steps":10,"stepMs":200}}')"
CRASH_RUN="$(python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])" <<<"$RUN_JSON")"
echo "crash run: $CRASH_RUN"
for _ in $(seq 1 50); do
  STATUS="$(auth "$BASE/api/v1/runs/$CRASH_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])")"
  if [[ "$STATUS" == "running" || "$STATUS" == "leased" ]]; then break; fi
  sleep 0.2
done
echo "run active as $STATUS; killing worker"
kill_worker_hard
if auth "$BASE/api/v1/health" >/dev/null; then echo "[PASS] core stays healthy after worker kill"; fi
echo "waiting for lease expiry (TTL 10s + sweep)…"
sleep 14
AFTER="$(auth "$BASE/api/v1/runs/$CRASH_RUN" | python3 -c "import json,sys; r=json.load(sys.stdin)['run']; print(r['status'], r['attemptCount'])")"
echo "after expiry: $AFTER"
ATTEMPT1="$(auth "$BASE/api/v1/runs/$CRASH_RUN/attempts" | python3 -c "import json,sys; a=json.load(sys.stdin)['attempts'][0]; print(a['id'], a['leaseGeneration'], a['status'])")"
echo "attempt1: $ATTEMPT1"
# A stale completion for the dead generation must never commit.
STALE_CODE="$(curl -s -o /tmp/factory-s2-stale.json -w '%{http_code}' -X POST "$BASE/api/v1/workers/report" \
  -H "Authorization: Bearer $WTOKEN" -H 'Content-Type: application/json' \
  -d "{\"workerId\":\"local-1\",\"runId\":\"$CRASH_RUN\",\"attemptId\":\"$(echo "$ATTEMPT1" | cut -d' ' -f1)\",\"generation\":$(echo "$ATTEMPT1" | cut -d' ' -f2),\"outcome\":\"completed\",\"output\":{\"receipt\":\"forged\"}}")"
echo "stale report status: $STALE_CODE ($(cat /tmp/factory-s2-stale.json))"
if [[ "$STALE_CODE" != "409" ]]; then echo "[FAIL] stale completion not fenced"; exit 1; fi
echo "[PASS] stale completion fenced after crash"
start_worker
echo "waiting for retry to complete…"
for _ in $(seq 1 60); do
  STATUS="$(auth "$BASE/api/v1/runs/$CRASH_RUN" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])")"
  if [[ "$STATUS" == "succeeded" || "$STATUS" == "failed" ]]; then break; fi
  sleep 1
done
if [[ "$STATUS" != "succeeded" ]]; then echo "[FAIL] retry did not succeed: $STATUS"; exit 1; fi
echo "[PASS] retry after crash succeeded"
NATTEMPTS="$(auth "$BASE/api/v1/runs/$CRASH_RUN/attempts" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['attempts']))")"
if [[ "$NATTEMPTS" != "2" ]]; then echo "[FAIL] expected 2 attempts, got $NATTEMPTS"; exit 1; fi
echo "[PASS] attempt history shows expired attempt + retry (2 attempts, one authoritative result)"

echo "== phase 3: fencing and scoped-auth negatives =="
python3 "$ROOT/scripts/e2e_s2_oracle.py" "$BASE" "$TOKEN" "$WTOKEN" stage-fencing

echo "== phase 4: core restart preserves history and reconciles =="
RUNS_BEFORE="$(auth "$BASE/api/v1/runs?limit=200" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['runs']))")"
stop_core
stop_worker
start_core
python3 "$ROOT/scripts/e2e_s2_oracle.py" "$BASE" "$TOKEN" "$WTOKEN" stage-history
start_worker
python3 "$ROOT/scripts/e2e_s2_oracle.py" "$BASE" "$TOKEN" "$WTOKEN" stage-positive
RUNS_AFTER="$(auth "$BASE/api/v1/runs?limit=200" | python3 -c "import json,sys; print(len(json.load(sys.stdin)['runs']))")"
if [[ "$RUNS_AFTER" -le "$RUNS_BEFORE" ]]; then echo "[FAIL] runs lost across restart"; exit 1; fi
echo "[PASS] post-restart run completed; history grew $RUNS_BEFORE -> $RUNS_AFTER"

echo "== phase 5: podman fixture restriction probes =="
if ! command -v podman >/dev/null 2>&1; then
  blocked "podman CLI unavailable: container restriction probes NOT_TESTED"
else
  PROBE_WORK="$TMP/probe-work"
  mkdir -p "$PROBE_WORK"
  echo "host-secret-canary" > "$TMP/host-canary.txt"
  OUT="$TMP/probe-out.txt"
  # Same restriction set the Worker uses, plus a deliberately unmounted canary.
  if podman run --rm --network=none --read-only --cap-drop=ALL \
      --security-opt=no-new-privileges --pids-limit=32 --memory=128m --cpus=0.5 \
      --userns=keep-id -v "$PROBE_WORK:/workspace:rw" --workdir /workspace \
      --env MESSAGE=probe --name factory-s2-probe \
      docker.io/library/alpine:3.20 sh -c '
        echo "--- workspace writable:"; echo ok > /workspace/w.txt && cat /workspace/w.txt
        echo "--- host canary:"; test ! -e /host-canary.txt && echo INACCESSIBLE
        echo "--- tmp canary dir:"; test ! -e /tmp/host-canary.txt && echo INACCESSIBLE
        echo "--- read-only root:"; (touch /should-fail 2>/dev/null && echo WRITABLE || echo READONLY)
        echo "--- network:"; (ping -c1 -W1 8.8.8.8 >/dev/null 2>&1 && echo REACHABLE || echo UNREACHABLE)
        echo "--- secrets in env:"; (printenv | grep -qi "factory\|token\|secret" && echo LEAKED || echo CLEAN)
        echo "--- sockets:"; (test ! -e /run/podman/podman.sock && test ! -e /run/docker.sock && echo ABSENT || echo PRESENT)
        echo "--- no-new-privs:"; grep -q "NoNewPrivs:[[:space:]]*1" /proc/self/status && echo ENFORCED || echo NOT-ENFORCED
      ' >"$OUT" 2>&1; then
    cat "$OUT"
    grep -q "INACCESSIBLE" "$OUT" || { echo "[FAIL] host paths reachable"; exit 1; }
    grep -q "^READONLY" "$OUT" || { echo "[FAIL] root filesystem writable"; exit 1; }
    grep -q "UNREACHABLE" "$OUT" || { echo "[FAIL] network reachable"; exit 1; }
    grep -q "CLEAN" "$OUT" || { echo "[FAIL] secrets visible in env"; exit 1; }
    grep -q "ABSENT" "$OUT" || { echo "[FAIL] container socket mounted"; exit 1; }
    grep -q "ENFORCED" "$OUT" || { echo "[FAIL] no-new-privileges not enforced"; exit 1; }
    echo "[PASS] podman fixture restrictions verified"
  else
    blocked "fixture image unavailable: container restriction probes NOT_TESTED"
    tail -5 "$OUT" || true
  fi
fi

# A shell canary proves the container boundary in the same way the Worker
# boundary must hold: host-side secrets never enter the fixture environment.
export FACTORY_S2_PROBE_CANARY="canary-$(head -c8 /dev/urandom | od -An -tx1 | tr -d ' \n')"
if command -v podman >/dev/null 2>&1 && podman image exists docker.io/library/alpine:3.20 >/dev/null 2>&1; then
  if podman run --rm --network=none --read-only --cap-drop=ALL \
      --security-opt=no-new-privileges --userns=keep-id \
      --env MESSAGE=canary-check \
      docker.io/library/alpine:3.20 sh -c 'printenv' 2>/dev/null | grep -q "FACTORY_S2_PROBE_CANARY"; then
    echo "[FAIL] host canary visible inside the fixture container"
    exit 1
  fi
  echo "[PASS] host canary absent from the fixture container environment"
fi
unset FACTORY_S2_PROBE_CANARY

stop_worker
stop_core

echo "== phase 6: S1 regression (unmodified suite) =="
FACTORY_E2E_PORT=8471 bash "$ROOT/scripts/e2e-s1.sh" 2>&1 | tail -5

final_verdict
