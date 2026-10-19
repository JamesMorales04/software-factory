#!/usr/bin/env bash
# Exercise the compiled local lifecycle CLI from outside the source tree.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-cli-XXXXXX")"
DIST="$TMP/installation"
STATE="$TMP/state"
OUTSIDE="$TMP/outside"
PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
FACTORY="$DIST/factory"
BASE="http://127.0.0.1:$PORT"
LOG_FOLLOW_PID=""
mkdir -p "$OUTSIDE"

cleanup() {
  if [[ -n "$LOG_FOLLOW_PID" ]]; then
    kill -TERM "$LOG_FOLLOW_PID" 2>/dev/null || true
    wait "$LOG_FOLLOW_PID" 2>/dev/null || true
  fi
  (cd "$OUTSIDE" && FACTORY_HOME="$STATE" FACTORY_ADDR="127.0.0.1:$PORT" "$FACTORY" stop >/dev/null 2>&1) || true
  rm -rf "$TMP"
}
trap cleanup EXIT

echo "== build compiled distribution =="
"$ROOT/scripts/build-factory.sh" "$DIST"
cd "$OUTSIDE"
export FACTORY_HOME="$STATE"
export FACTORY_ADDR="127.0.0.1:$PORT"

echo "== start from outside source directory =="
START_OUTPUT="$("$FACTORY" start)"
printf '%s\n' "$START_OUTPUT"
[[ "$START_OUTPUT" == *"Factory status: ready"* ]]
[[ "$START_OUTPUT" == *"$BASE/"* ]]
[[ "$START_OUTPUT" == *"$STATE/factory.db"* ]]
[[ "$START_OUTPUT" == *"$STATE/api.token"* ]]
[[ "$START_OUTPUT" != *"one-time-test-token"* ]]

python3 - "$BASE" "$STATE/api.token" "$STATE/factory.log" <<'PY'
import json, os, stat, sys, urllib.error, urllib.request
base, token_path, log_path = sys.argv[1:]
mode = stat.S_IMODE(os.stat(token_path).st_mode)
assert mode == 0o600, oct(mode)
with open(token_path, encoding="utf-8") as f:
    token = f.read().strip()
assert token and len(token) >= 32
health = urllib.request.Request(base + "/api/v1/health",
    headers={"Authorization": "Bearer " + token})
with urllib.request.urlopen(health, timeout=3) as r:
    assert r.status == 200
with urllib.request.urlopen(base + "/", timeout=3) as r:
    assert r.status == 200 and b"<html" in r.read().lower()
try:
    req = urllib.request.Request(base + "/api/v1/actions", data=b'{"title":"unauthorized"}',
        method="POST", headers={"Content-Type":"application/json"})
    urllib.request.urlopen(req, timeout=3)
    raise AssertionError("unauthenticated create unexpectedly succeeded")
except urllib.error.HTTPError as error:
    assert error.code == 401, error.code
req = urllib.request.Request(base + "/api/v1/actions",
    data=json.dumps({"title":"factory-cli-persistence","description":"cli e2e"}).encode(),
    method="POST",
    headers={"Authorization": "Bearer " + token, "Content-Type":"application/json"})
with urllib.request.urlopen(req, timeout=5) as r:
    assert r.status == 201, r.status
    action_id = json.load(r)["action"]["id"]
print("authenticated create: PASS")
print("authentication negative: PASS")
with open(log_path, encoding="utf-8") as f:
    logs = f.read()
assert token not in logs, "API token found in persistent Core logs"
print("token absent from persistent logs: PASS")
PY

echo "== duplicate start and status =="
SECOND_OUTPUT="$("$FACTORY" start)"
[[ "$SECOND_OUTPUT" == *"already running"* ]]
STATUS_OUTPUT="$("$FACTORY" status)"
[[ "$STATUS_OUTPUT" == *"Factory status: running"* ]]
"$FACTORY" logs -n 5 >/dev/null
"$FACTORY" logs -n 0 -f >"$TMP/follow.log" 2>&1 &
LOG_FOLLOW_PID=$!
python3 - "$BASE" "$STATE/api.token" <<'PY'
import json, sys, urllib.request
base, token_path = sys.argv[1:]
with open(token_path, encoding="utf-8") as f:
    token = f.read().strip()
req = urllib.request.Request(base + "/api/v1/actions",
    data=json.dumps({"title":"factory-cli-log-follow"}).encode(),
    method="POST",
    headers={"Authorization":"Bearer " + token, "Content-Type":"application/json"})
with urllib.request.urlopen(req, timeout=5) as r:
    assert r.status == 201
PY
FOLLOWED=0
for _ in $(seq 1 30); do
  if grep -q '"msg":"action created"' "$TMP/follow.log"; then
    FOLLOWED=1
    break
  fi
  sleep 0.1
done
kill -TERM "$LOG_FOLLOW_PID" 2>/dev/null || true
wait "$LOG_FOLLOW_PID" 2>/dev/null || true
LOG_FOLLOW_PID=""
[[ "$FOLLOWED" == 1 ]]
echo "factory logs -f: PASS"

echo "== graceful stop and persisted restart =="
"$FACTORY" stop
STOP_OUTPUT="$("$FACTORY" status)"
[[ "$STOP_OUTPUT" == *"Factory status: stopped"* ]]
"$FACTORY" start >/dev/null
python3 - "$BASE" "$STATE/api.token" <<'PY'
import json, sys, urllib.request
base, token_path = sys.argv[1:]
with open(token_path, encoding="utf-8") as f:
    token = f.read().strip()
req = urllib.request.Request(base + "/api/v1/actions",
    headers={"Authorization": "Bearer " + token})
with urllib.request.urlopen(req, timeout=5) as r:
    actions = json.load(r)["actions"]
assert any(a["title"] == "factory-cli-persistence" for a in actions), actions
print("SQLite action retained after CLI restart: PASS")
PY
"$FACTORY" logs -n 20 >"$TMP/recent.log"
python3 - "$STATE/api.token" "$TMP/recent.log" <<'PY'
import sys
token = open(sys.argv[1], encoding="utf-8").read().strip()
logs = open(sys.argv[2], encoding="utf-8").read()
assert token not in logs, "API token found in displayed logs"
print("token absent from displayed logs: PASS")
PY
"$FACTORY" stop
[[ "$("$FACTORY" status)" == *"Factory status: stopped"* ]]

echo
echo "Factory CLI E2E: PASS (outside-directory startup, readiness, auth, private token, logs, stop, SQLite restart)"
