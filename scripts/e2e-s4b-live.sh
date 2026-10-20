#!/usr/bin/env bash
# F1-S4-B bounded REAL orchestration lane. One authorized Paseo Implementer
# session (maxAttempts=1, maxRepairs=0) is started by Factory, followed
# automatically by Core-spawned independent verifier, reviewer and gate.
# TRUSTED_LOCAL only; never isolated or certified. No retry loop.
set -euo pipefail

if [[ "${FACTORY_S4B_LIVE:-0}" != "1" ]]; then
  echo "Set FACTORY_S4B_LIVE=1 to authorize one real Paseo session (paid)."
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s4b-live-XXXXXX")"
TOKEN="s4blive-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s4blive-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
VTOKEN="s4blive-verifier-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
RTOKEN="s4blive-reviewer-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
PORT="${FACTORY_S4B_LIVE_PORT:-8496}"
BASE="http://127.0.0.1:${PORT}"
PROVIDER="${S4B_LIVE_PROVIDER:-opencode}"
MODEL="${S4B_LIVE_MODEL:-opencode-go/deepseek-v4.1-flash}"
PASEO_MODE="${S4B_LIVE_MODE:-build}"
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
[[ -x "$CHROMIUM_EXE" ]] || { echo "[BLOCKED] Chromium not found at $CHROMIUM_EXE"; exit 2; }
[[ -d "$PLAYWRIGHT_CORE" ]] || { echo "[BLOCKED] playwright-core not found at $PLAYWRIGHT_CORE"; exit 2; }
command -v paseo >/dev/null || { echo "[BLOCKED] paseo CLI is unavailable"; exit 2; }

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
  CHROMIUM_EXE="$CHROMIUM_EXE" FACTORY_PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" \
  "$TMP/factory-core" --addr "127.0.0.1:$PORT" --db "$TMP/factory.db" \
  --web-dir "$ROOT/office/dist" --verifier-bin "$TMP/factory-verifier" \
  --reviewer-bin "$TMP/factory-reviewer" >"$TMP/core.log" 2>&1 &
CORE_PID=$!
for _ in $(seq 1 80); do
  curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health" && break
  sleep 0.2
done
env FACTORY_WORKER_TOKEN="$WTOKEN" \
  FACTORY_BROWSER_CLI="$ROOT/office/e2e-browser/node_modules/.bin/playwright-cli" \
  "$TMP/factory-worker" --core "$BASE" --worker-id live-s4b --exec-mode local \
  --recovery-interval 10s >"$TMP/worker.log" 2>&1 &
WORKER_PID=$!
sleep 2
kill -0 "$WORKER_PID" 2>/dev/null || { echo "[BLOCKED] Worker failed"; tail -40 "$TMP/worker.log"; exit 2; }

echo "== create Action + frozen spec + campaign ($PROVIDER/$MODEL mode=$PASEO_MODE) =="
python3 - "$BASE" "$TOKEN" "$TMP/repo.git" "$BASE_REV" "$PROVIDER" "$MODEL" "$PASEO_MODE" >"$TMP/campaign.env" <<'PY'
import json, sys, urllib.request
base, token, repo, base_rev, provider, model, mode = sys.argv[1:8]
def call(method, path, body=None):
    req = urllib.request.Request(base+path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization":"Bearer "+token,"Content-Type":"application/json"})
    with urllib.request.urlopen(req, timeout=30) as res:
        return json.loads(res.read().decode())
action = call("POST","/api/v1/actions",{"title":"F1-S4-B live orchestration","description":"One bounded real Paseo Implementer managed by a Factory campaign"})["action"]
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
campaign = call("POST","/api/v1/campaigns",{
  "specId": spec["id"], "maxRepairCycles": 0,
  "maxWallClockSecs": 1800, "maxParallel": 2
})["campaign"]
call("POST","/api/v1/campaigns/"+campaign["id"]+"/start", {})
print("ACTION_ID="+action["id"])
print("SPEC_ID="+spec["id"])
print("CAMPAIGN_ID="+campaign["id"])
PY
source "$TMP/campaign.env"
echo "campaign=$CAMPAIGN_ID action=$ACTION_ID spec=$SPEC_ID"

echo "== Factory-managed workflow (one paid session, no manual role calls) =="
STATUS=""
for _ in $(seq 1 1200); do
  BODY="$(auth "$BASE/api/v1/campaigns/$CAMPAIGN_ID")"
  STATUS="$(printf '%s' "$BODY" | python3 -c 'import json,sys; print(json.load(sys.stdin)["campaign"]["status"])')"
  case "$STATUS" in
    accepted|repair_required|blocked|human_required|failed|cancelled) break ;;
  esac
  sleep 1
done
printf '%s' "$BODY" >"$TMP/campaign.json"
printf '%s\n' "$BODY" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("campaign status:",d["campaign"]["status"]); [print(t["key"],t["status"],t.get("outcome","")) for t in d["tasks"]]'
echo "core log: $TMP/core.log"
echo "worker log: $TMP/worker.log"
echo "campaign evidence: $TMP/campaign.json"
if [[ "$STATUS" != "accepted" ]]; then
  echo "[NON-ACCEPT] campaign terminal status is $STATUS (evidence retained)"
  exit 1
fi
echo "F1-S4-B live orchestration: ACCEPTED (TRUSTED_LOCAL only; not isolated)"
