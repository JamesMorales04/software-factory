#!/usr/bin/env bash
# F1-S4-A bounded real Implementer lane. Starts a real Paseo session through
# the existing Factory -> Worker -> Paseo lifecycle against a disposable git
# repository, freezes the candidate, runs the independent verifier (including
# the protected browser check) and reviewer, and reports the Core gate.
#
# Exactly ONE paid session is authorized per invocation (maxAttempts=1).
# No retry loop. TRUSTED_LOCAL only; never isolated or certified.
set -euo pipefail

LIVE="${FACTORY_S4A_LIVE:-0}"
if [[ "$LIVE" != "0" && "$LIVE" != "1" ]]; then
  echo "[BLOCKED] FACTORY_S4A_LIVE must be 0 or 1"
  exit 2
fi
if [[ "$LIVE" != "1" ]]; then
  echo "Set FACTORY_S4A_LIVE=1 to authorize one real Paseo session (paid)."
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s4a-live-XXXXXX")"
TOKEN="s4alive-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s4alive-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
VTOKEN="s4alive-verifier-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
RTOKEN="s4alive-reviewer-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S4A_LIVE_PORT:-8498}"
BASE="http://127.0.0.1:${PORT}"
PROVIDER="${S4A_LIVE_PROVIDER:-opencode}"
MODEL="${S4A_LIVE_MODEL:-opencode-go/deepseek-v4.1-flash}"
PASEO_MODE="build"
python3 "$ROOT/scripts/live-e2e-authorization.py" \
  --campaign F1-S4-A --project default --max-sessions 1 \
  --route opencode/opencode-go/deepseek-v4.1-flash \
  --requested-route "$PROVIDER/$MODEL"
CHROMIUM_EXE="${CHROMIUM_EXE:-$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome}"
PLAYWRIGHT_CORE="${FACTORY_PLAYWRIGHT_CORE:-$ROOT/office/e2e-browser/node_modules/playwright-core}"
CORE_PID=""
WORKER_PID=""

cleanup() {
  for var in WORKER_PID CORE_PID; do
    pid="${!var:-}"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT

auth() { curl -sf -H "Authorization: Bearer $TOKEN" "$@"; }

echo "== build binaries and Office =="
(cd "$ROOT/core" && go build -o "$TMP/factory-core" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$TMP/factory-worker" ./cmd/factory-worker)
(cd "$ROOT/core" && go build -o "$TMP/factory-verifier" ./cmd/factory-verifier)
(cd "$ROOT/core" && go build -o "$TMP/factory-reviewer" ./cmd/factory-reviewer)
(cd "$ROOT/office" && npm run build >/dev/null)
[[ -x "$CHROMIUM_EXE" ]] || { echo "Chromium not found at $CHROMIUM_EXE"; exit 1; }
[[ -d "$PLAYWRIGHT_CORE" ]] || { echo "playwright-core not found at $PLAYWRIGHT_CORE"; exit 1; }

echo "== disposable authorized repository =="
cp -r "$ROOT/scripts/fixtures/s4a-ui" "$TMP/src"
git -C "$TMP/src" init -q
git -C "$TMP/src" -c user.name=fixture -c user.email=fixture@factory.local add .
git -C "$TMP/src" -c user.name=fixture -c user.email=fixture@factory.local commit -qm "live fixture baseline"
git clone -q --bare "$TMP/src" "$TMP/repo.git"
BASE_REV="$(git -C "$TMP/repo.git" rev-parse HEAD)"

echo "== start Core + real Worker =="
env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
  FACTORY_VERIFIER_TOKEN="$VTOKEN" FACTORY_REVIEWER_TOKEN="$RTOKEN" \
  "$TMP/factory-core" --addr "127.0.0.1:$PORT" --db "$TMP/factory.db" \
  --web-dir "$ROOT/office/dist" >"$TMP/core.log" 2>&1 &
CORE_PID=$!
for _ in $(seq 1 80); do
  curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health" && break
  sleep 0.2
done
env FACTORY_WORKER_TOKEN="$WTOKEN" \
  FACTORY_BROWSER_CLI="$ROOT/office/e2e-browser/node_modules/.bin/playwright-cli" \
  "$TMP/factory-worker" --core "$BASE" --worker-id live-s4a --exec-mode local \
  --recovery-interval 0 >"$TMP/worker.log" 2>&1 &
WORKER_PID=$!
sleep 2
kill -0 "$WORKER_PID" 2>/dev/null || { echo "worker failed"; tail -40 "$TMP/worker.log"; exit 1; }

echo "== create Action + frozen spec =="
python3 - "$BASE" "$TOKEN" "$TMP/repo.git" "$BASE_REV" "$PROVIDER" "$MODEL" "$PASEO_MODE" <<'PY' >"$TMP/spec.env"
import json, sys, urllib.request
base, token, repo, base_rev, provider, model, mode = sys.argv[1:8]
def call(method, path, body=None):
    req = urllib.request.Request(base+path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization":"Bearer "+token,"Content-Type":"application/json"})
    with urllib.request.urlopen(req, timeout=30) as res:
        return json.loads(res.read().decode())
action = call("POST","/api/v1/actions",{"title":"F1-S4-A live implementer","description":"One bounded real Paseo session"})["action"]
functional = "const {multiply}=require('./add.js');if(multiply(2,3)!==6)process.exit(1);"
negative = "const {add}=require('./add.js');if(add(2,3)!==5)process.exit(1);"
spec = call("POST","/api/v1/action-specs",{
  "actionId": action["id"],
  "goal": "Append a correct multiply(a,b) export to add.js and wire the operation selector in app.js so multiply computes a*b. Keep add behavior and the existing UI contract unchanged. Never modify browser-check.mjs, add.test.js, index.html, or package.json.",
  "repository": repo, "baseRevision": base_rev,
  "allowedPaths": ["add.js","app.js"],
  "acceptanceCriteria": [
    {"id":"format","kind":"format","description":"syntax","command":["node","--check","add.js"],"expectExit":0,"timeoutSecs":30},
    {"id":"test","kind":"test","description":"unit tests","command":["node","--test","add.test.js"],"expectExit":0,"timeoutSecs":60},
    {"id":"functional","kind":"functional","description":"multiply works","command":["node","-e",functional],"expectExit":0,"timeoutSecs":30},
    {"id":"browser","kind":"browser","description":"UI multiply journey","command":["node","browser-check.mjs"],"expectExit":0,"timeoutSecs":120}
  ],
  "negativeCases":[{"id":"negative","kind":"negative","description":"add regression","command":["node","-e",negative],"expectExit":0,"timeoutSecs":30}],
  "riskClass":"R0","qualityProfile":"node-browser",
  "budget":{"maxAttempts":1,"timeoutSecs":600},
  "executionMode":"trusted-local","provider":provider,"model":model,"paseoMode":mode
})["spec"]
print("ACTION_ID="+action["id"])
print("SPEC_ID="+spec["id"])
PY
source "$TMP/spec.env"
echo "action=$ACTION_ID spec=$SPEC_ID"

echo "== queue ONE real Implementer Run ($PROVIDER/$MODEL mode=$PASEO_MODE) =="
RUN_ID="$(auth -X POST "$BASE/api/v1/runs" -H 'Content-Type: application/json' \
  -d "{\"specId\":\"$SPEC_ID\"}" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['id'])")"
echo "run=$RUN_ID"
STATUS=""
for _ in $(seq 1 900); do
  STATUS="$(auth "$BASE/api/v1/runs/$RUN_ID" | python3 -c "import json,sys; print(json.load(sys.stdin)['run']['status'])")"
  case "$STATUS" in succeeded|failed|cancelled) break ;; esac
  sleep 1
done
if [[ "$STATUS" != "succeeded" ]]; then
  echo "[FAIL] real Implementer Run ended $STATUS (no retry; one paid session only)"
  tail -80 "$TMP/worker.log"
  exit 1
fi
CANDIDATE_ID="$(auth "$BASE/api/v1/runs/$RUN_ID/candidate" | python3 -c "import json,sys; print(json.load(sys.stdin)['candidate']['id'])")"
COMMIT_SHA="$(auth "$BASE/api/v1/runs/$RUN_ID/candidate" | python3 -c "import json,sys; print(json.load(sys.stdin)['candidate']['commitSha'])")"
SESSION_ID="$(auth "$BASE/api/v1/runs/$RUN_ID/sessions" | python3 -c "import json,sys; s=json.load(sys.stdin)['sessions']; print(s[0]['sessionId'] if s else '')")"
echo "candidate=$CANDIDATE_ID commit=$COMMIT_SHA session=$SESSION_ID"

echo "== independent verifier (clean checkout, real Chromium) =="
env -u FACTORY_API_TOKEN -u FACTORY_WORKER_TOKEN FACTORY_VERIFIER_TOKEN="$VTOKEN" \
  CHROMIUM_EXE="$CHROMIUM_EXE" FACTORY_PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" \
  "$TMP/factory-verifier" --core "$BASE" --candidate "$CANDIDATE_ID" --instance-id verifier-live
echo "== read-only reviewer =="
env -u FACTORY_API_TOKEN -u FACTORY_WORKER_TOKEN FACTORY_REVIEWER_TOKEN="$RTOKEN" \
  "$TMP/factory-reviewer" --core "$BASE" --candidate "$CANDIDATE_ID" --instance-id reviewer-live

DECISION=""
for _ in $(seq 1 60); do
  DECISION="$(auth "$BASE/api/v1/candidates/$CANDIDATE_ID/evidence" | python3 -c "import json,sys; b=json.load(sys.stdin)['bundle']; d=b.get('decision'); print(d['status'] if d else '')")"
  [[ -n "$DECISION" ]] && break
  sleep 1
done
echo "gate=$DECISION"
auth "$BASE/api/v1/candidates/$CANDIDATE_ID/evidence" >"$TMP/bundle.json"
python3 - "$TMP/bundle.json" <<'PY'
import json, sys
b=json.load(open(sys.argv[1]))["bundle"]
print("candidate", b["candidate"]["id"], b["candidate"]["commitSha"])
print("changed", b["candidate"]["changedPaths"])
for c in (b.get("decision") or {}).get("checks", []):
    print("gate-check", c["id"], c["status"])
for v in b.get("verifications", []):
    for c in v["checks"]:
        print("verifier", c["id"], c["status"], c.get("detail",""))
PY

if [[ "$DECISION" != "ACCEPTED" ]]; then
  echo "[REPAIR_REQUIRED/BLOCKED] gate did not accept the real candidate"
  exit 1
fi
echo "[PASS] real Paseo Implementer -> immutable candidate -> independent verification/review -> ACCEPTED"
echo "evidence: $TMP"
echo "run=$RUN_ID candidate=$CANDIDATE_ID commit=$COMMIT_SHA session=$SESSION_ID"
echo "Exactly one paid session was started; no retry was attempted."
