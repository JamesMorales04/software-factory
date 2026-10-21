#!/usr/bin/env bash
# F1-S4-C bounded REAL Paseo certification lane. Product Owner opt-in only.
# One Implementer session per selected Project (maximum two total), no paid
# repair cycles and no automatic retries. Factory Core owns the campaign,
# verifier, reviewer and Quality Gate. TRUSTED_LOCAL only; never isolated.
set -euo pipefail

if [[ "${FACTORY_S4C_LIVE:-0}" != "1" ]]; then
  echo "Set FACTORY_S4C_LIVE=1 to authorize bounded real Paseo Implementer session(s)."
  exit 0
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-s4c-live-XXXXXX")"
PORT="${FACTORY_S4C_LIVE_PORT:-8496}"
PROVIDER="${S4C_LIVE_PROVIDER:-opencode}"
MODEL="${S4C_LIVE_MODEL:-opencode-go/deepseek-v4.1-flash}"
PASEO_MODE="${S4C_LIVE_MODE:-build}"
PROJECTS="${S4C_LIVE_PROJECTS:-vlog calculator}"
CHROMIUM_EXE="${CHROMIUM_EXE:-$HOME/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome}"
PLAYWRIGHT_CORE="${FACTORY_PLAYWRIGHT_CORE:-$ROOT/office/e2e-browser/node_modules/playwright-core}"
BROWSER_CLI="${FACTORY_BROWSER_CLI:-$ROOT/office/e2e-browser/node_modules/.bin/playwright-cli}"
VLOG="${S4C_VLOG_REPO:-$HOME/Escritorio/Development/factory-fixtures/factory-fixture-vlog}"
CALC="${S4C_CALC_REPO:-$HOME/Escritorio/Development/factory-fixtures/factory-fixture-calculator}"
BASE="http://127.0.0.1:${PORT}"
TOKEN="s4clive-api-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
WTOKEN="s4clive-worker-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
VTOKEN="s4clive-verifier-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
RTOKEN="s4clive-reviewer-$(head -c16 /dev/urandom | od -An -tx1 | tr -d ' \n')"
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

command -v paseo >/dev/null || { echo "[BLOCKED] paseo CLI is unavailable"; exit 2; }
paseo provider ls 2>/dev/null | grep -q "opencode.*available" || { echo "[BLOCKED] opencode provider is unavailable"; exit 2; }
[[ -x "$CHROMIUM_EXE" ]] || { echo "[BLOCKED] Chromium not found at $CHROMIUM_EXE"; exit 2; }
[[ -d "$PLAYWRIGHT_CORE" ]] || { echo "[BLOCKED] playwright-core not found at $PLAYWRIGHT_CORE"; exit 2; }
[[ -x "$BROWSER_CLI" ]] || { echo "[BLOCKED] pinned Playwright CLI not found"; exit 2; }
[[ -d "$VLOG/.git" ]] || { echo "[BLOCKED] vlog fixture missing at $VLOG"; exit 2; }
[[ -d "$CALC/.git" ]] || { echo "[BLOCKED] calculator fixture missing at $CALC"; exit 2; }

echo "== build binaries and Office =="
(cd "$ROOT/core" && go build -o "$TMP/factory-core" ./cmd/factory-core)
(cd "$ROOT/core" && go build -o "$TMP/factory-worker" ./cmd/factory-worker)
(cd "$ROOT/core" && go build -o "$TMP/factory-verifier" ./cmd/factory-verifier)
(cd "$ROOT/core" && go build -o "$TMP/factory-reviewer" ./cmd/factory-reviewer)
(cd "$ROOT/office" && npm run build >/dev/null)

echo "== start Core and real Paseo Worker =="
env FACTORY_API_TOKEN="$TOKEN" FACTORY_WORKER_TOKEN="$WTOKEN" \
  FACTORY_VERIFIER_TOKEN="$VTOKEN" FACTORY_REVIEWER_TOKEN="$RTOKEN" \
  CHROMIUM_EXE="$CHROMIUM_EXE" FACTORY_PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" \
  "$TMP/factory-core" --addr "127.0.0.1:$PORT" --db "$TMP/factory.db" \
  --web-dir "$ROOT/office/dist" --verifier-bin "$TMP/factory-verifier" \
  --reviewer-bin "$TMP/factory-reviewer" >"$TMP/core.log" 2>&1 &
CORE_PID=$!
for _ in $(seq 1 100); do
  curl -sf -o /dev/null -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/health" && break
  sleep 0.2
done
env PATH="$PATH" HOME="$HOME" TMPDIR="$TMP" FACTORY_WORKER_TOKEN="$WTOKEN" \
  CHROMIUM_EXE="$CHROMIUM_EXE" FACTORY_PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" \
  FACTORY_BROWSER_CLI="$BROWSER_CLI" \
  "$TMP/factory-worker" --core "$BASE" --worker-id "live-s4c-$$" \
  --exec-mode local --max-concurrent 1 --recovery-interval 10s \
  >"$TMP/worker.log" 2>&1 &
WORKER_PID=$!
sleep 2
kill -0 "$WORKER_PID" 2>/dev/null || { echo "[BLOCKED] real Worker failed"; tail -40 "$TMP/worker.log"; exit 2; }

RESULT_FILE="$TMP/live-results.json"
echo '[]' > "$RESULT_FILE"
FAILED=0
ATTEMPTS=0
for PROJECT in $PROJECTS; do
  if [[ "$ATTEMPTS" -ge 2 ]]; then break; fi
  ATTEMPTS=$((ATTEMPTS + 1))
  echo "== REAL Paseo Implementer: $PROJECT (session $ATTEMPTS/2) =="
  set +e
  PROJECT="$PROJECT" python3 - "$BASE" "$TOKEN" "$VLOG" "$CALC" "$PROVIDER" "$MODEL" "$PASEO_MODE" "$RESULT_FILE" <<'PY'
import json, os, subprocess, sys, time, urllib.error, urllib.request
base, token, vlog, calc, provider, model, mode, result_file = sys.argv[1:9]
project = os.environ["PROJECT"]
repo = vlog if project == "vlog" else calc
def call(method, path, body=None):
    req = urllib.request.Request(base+path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Authorization":"Bearer "+token,"Content-Type":"application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as res:
            return res.status, json.loads(res.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode() or "{}")
sha = subprocess.check_output(["git","-C",repo,"rev-parse","HEAD"], text=True).strip()
if project == "vlog":
    status, body = call("POST","/api/v1/projects",{"id":"vlog-live","name":"Live Vlog","repositoryPath":repo,
        "baseRefs":["main"],"qualityProfile":"node-browser","workspaceScope":".",
        "capabilities":["repository-edit","node-toolchain","browser"],
        "browserEndpoints":["http://127.0.0.1:4173"],"executionMode":"trusted-local"})
    profile = "node-browser"
    allowed = ["src/filter.js","src/app.js"]
    goal = ("Implement functional category filtering with a no-results state in src/filter.js and src/app.js. "
            "Preserve navigation, accessibility and the five existing entries. Use the factory-browser skill and "
            "wrapper to exercise the UI at http://127.0.0.1:4173 after starting npm start. Do not edit tests, "
            "browser-check.mjs, package.json, data.js or render.js.")
    functional = ("node\n--input-type=module\n-e\nimport { filterEntries } from './src/filter.js'; "
                  "const entries=[{category:'Travel'},{category:'Technology'}]; "
                  "if(filterEntries(entries,'Travel').length!==1||filterEntries(entries,'Film').length!==0||filterEntries(entries,'All').length!==2)process.exit(1)")
    negative = ("node\n--input-type=module\n-e\nimport { entries } from './src/data.js'; "
                "if(entries.length!==5)process.exit(1)")
else:
    status, body = call("POST","/api/v1/projects",{"id":"calculator-live","name":"Live Calculator","repositoryPath":repo,
        "baseRefs":["main"],"qualityProfile":"node-basic","workspaceScope":".",
        "capabilities":["repository-edit","node-toolchain"],"browserEndpoints":[],"executionMode":"trusted-local"})
    profile = "node-basic"
    allowed = ["src/calc.js","src/app.js"]
    goal = ("Correct decimal precision handling and division-by-zero behavior in src/calc.js and src/app.js while "
            "preserving addition, subtraction, multiplication, division and button interaction. The browser must show "
            "0.3 for 0.1 + 0.2 and 'Cannot divide by zero' for division by zero. Do not edit tests or protected files.")
    functional = ("node\n--input-type=module\n-e\n"
                  "import {calculate,formatResult} from './src/calc.js';"
                  "if(formatResult(calculate(0.1,0.2,'add'))!=='0.3')process.exit(1);"
                  "let zero;try{zero=formatResult(calculate(1,0,'divide'))}catch(error){zero=String(error.message)}"
                  "if(zero!=='Cannot divide by zero')process.exit(1)")
    negative = ("node\n--input-type=module\n-e\nimport {add,subtract,multiply,divide} from './src/calc.js';"
                "if(add(2,3)!==5||subtract(9,4)!==5||multiply(6,7)!==42||divide(8,2)!==4)process.exit(1)")
if status not in (200,201):
    print(json.dumps({"ok":False,"project":project,"stage":"project","status":status,"body":body}))
    sys.exit(3)
action = call("POST","/api/v1/actions",{"projectId":body["project"]["id"],
    "title":"S4-C live "+project,"description":"Bounded real Paseo certification"})[1]["action"]
checks = [
 {"id":"format","kind":"format","description":"syntax","command":["npm","run","format"],"expectExit":0,"timeoutSecs":120},
 {"id":"test","kind":"test","description":"unit tests","command":["npm","test"],"expectExit":0,"timeoutSecs":180},
 {"id":"functional","kind":"functional","description":"task assertion","command":functional.split("\n"),"expectExit":0,"timeoutSecs":120},
 {"id":"browser","kind":"browser","description":"protected browser oracle","command":["node","tests/acceptance/browser-check.mjs"],"expectExit":0,"timeoutSecs":240},
]
status, body = call("POST","/api/v1/action-specs",{"projectId":body["project"]["id"],"actionId":action["id"],
    "goal":goal,"baseRevision":sha,"allowedPaths":allowed,"acceptanceCriteria":checks,
    "negativeCases":[{"id":"negative","kind":"negative","description":"regression",
        "command":negative.split("\n"),"expectExit":0,"timeoutSecs":120}],
    "riskClass":"R0","qualityProfile":profile,"budget":{"maxAttempts":1,"timeoutSecs":600},
    "executionMode":"trusted-local","provider":provider,"model":model,"paseoMode":mode})
if status != 201:
    print(json.dumps({"ok":False,"project":project,"stage":"spec","status":status,"body":body}))
    sys.exit(3)
spec = body["spec"]
campaign = call("POST","/api/v1/campaigns",{"specId":spec["id"],"maxRepairCycles":3,
    "maxWallClockSecs":3600,"maxParallel":2,"maxImplementerAttempts":4})[1]["campaign"]
call("POST","/api/v1/campaigns/"+campaign["id"]+"/start",{})
deadline = time.time() + 3900
projection = {}
while time.time() < deadline:
    status, projection = call("GET","/api/v1/campaigns/"+campaign["id"])
    if projection.get("campaign",{}).get("status") in {"accepted","repair_required","blocked","human_required","failed","cancelled"}:
        break
    time.sleep(2)
record = {"project":project,"campaignId":campaign["id"],"actionId":action["id"],"specId":spec["id"],
          "status":projection.get("campaign",{}).get("status"),"reason":projection.get("campaign",{}).get("terminalReason"),
          "candidate":(projection.get("candidate") or {}).get("commitSha"),
          "decision":(projection.get("decision") or {}).get("status"),
          "attemptsUsed":projection.get("budget",{}).get("attemptsUsed"),
          "maxAttempts":projection.get("budget",{}).get("maxAttempts"),
          "recoveries":projection.get("recoveries",[]),
          "tasks":[(t["key"],t["status"],t.get("runId","")) for t in projection.get("tasks",[])]}
if record["candidate"]:
    info = call("GET","/api/v1/candidates/"+projection["candidate"]["id"]+"/evidence")[1]["bundle"]
    record["verification"] = info.get("verifications",[{}])[0].get("checks",[])
    record["review"] = info.get("reviews",[{}])[0].get("verdict")
    if projection["candidate"].get("implementerRunId"):
        record["run"] = call("GET","/api/v1/runs/"+projection["candidate"]["implementerRunId"])[1].get("run")
results = json.load(open(result_file))
results.append(record)
json.dump(results, open(result_file,"w"), indent=2)
print(json.dumps(record, indent=2)[:4000])
sys.exit(0 if record["status"] == "accepted" else 1)
PY
  RC=$?
  set -e
  if [[ $RC -ne 0 ]]; then
    FAILED=1
    echo "[NON-ACCEPT] $PROJECT did not reach ACCEPTED; evidence retained."
  fi
done

echo "== results =="
cat "$RESULT_FILE"
echo "core log: $TMP/core.log"
echo "worker log: $TMP/worker.log"
echo "evidence: $TMP"
if [[ "$FAILED" -ne 0 ]]; then
  echo "[REPAIR_REQUIRED] at least one bounded live session did not accept (truthful result retained)."
  exit 1
fi
echo "F1-S4-C live certification: ACCEPTED (TRUSTED_LOCAL only; no delivery/merge)"
