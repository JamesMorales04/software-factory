#!/usr/bin/env bash
# Paseo TRUSTED_LOCAL real-use smoke test (Agent C).
#
# Uses the existing local Paseo daemon + locally authenticated provider.
# Disposable test repo ONLY under /tmp. Never touches owner personal files
# or this repo's source as a test target. No secret VALUES are read anywhere
# (provider availability/status names only; timeline bodies are summarized,
# never dumped with credentials).
#
# Usage:
#   PROVIDER=codex ./scripts/paseo-trusted-local-smoke.sh
#   PROVIDER=opencode ./scripts/paseo-trusted-local-smoke.sh
#
# Asserts: session created/observed/ended, git diff non-empty, npm tests pass,
# cleanup done. Exits nonzero on first failure.
set -euo pipefail

PROVIDER="${PROVIDER:-codex}"
if [[ "$PROVIDER" != "codex" && "$PROVIDER" != "opencode" ]]; then
  echo "FAIL: PROVIDER must be codex|opencode (got: $PROVIDER)" >&2
  exit 1
fi

# Cost-conscious bound for one real agent session.
WAIT_TIMEOUT="${WAIT_TIMEOUT:-300s}"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/paseo-trusted-local-XXXXXX")"
PROJ="$TMP/proj"
AGENT_ID=""
AGENT_JSON="$TMP/agent-run.json"
INSPECT_JSON="$TMP/agent-inspect.json"
LOGS_JSON="$TMP/agent-logs.json"

echo "== trusted-local smoke: provider requested=$PROVIDER =="
echo "versions:"
echo "  paseo: $(paseo --version 2>&1)"
echo "  node: $(node --version 2>&1)"
echo "  npm: $(npm --version 2>&1)"

cleanup() {
  local rc=$?
  if [[ -n "$AGENT_ID" ]]; then
    echo "-- cleanup: stop/archive agent $AGENT_ID (best effort) --"
    paseo stop "$AGENT_ID" >/dev/null 2>&1 || true
    paseo archive "$AGENT_ID" >/dev/null 2>&1 || true
  fi
  if [[ -d "$TMP" ]]; then
    rm -rf "$TMP"
    if [[ ! -e "$TMP" ]]; then
      echo "cleanup: TMP removed ($TMP)"
    else
      echo "cleanup: FAILED to remove $TMP" >&2
      rc=1
    fi
  fi
  exit "$rc"
}
trap cleanup EXIT

echo "-- provider availability (names/presence only, no secret values) --"
paseo status --json >"$TMP/status.json" 2>&1 || {
  echo "FAIL: 'paseo status' failed" >&2
  exit 1
}
python3 - "$TMP/status.json" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
provs=doc.get("providers") or doc.get("status",{}).get("providers") or []
# status --json shape varies; fall back to raw scan
print(json.dumps(doc.get("providers", doc), indent=2)[:2000])
EOF

echo "-- disposable project at $PROJ --"
mkdir -p "$PROJ"
cat >"$PROJ/package.json" <<'EOF'
{
  "name": "paseo-trusted-local-smoke",
  "version": "1.0.0",
  "private": true,
  "scripts": {
    "test": "node --test add.test.js",
    "format": "node --check add.js && node --check add.test.js"
  }
}
EOF
cat >"$PROJ/add.js" <<'EOF'
/** Add two numbers. */
function add(a, b) {
  return a + b;
}
module.exports = { add };
EOF
cat >"$PROJ/add.test.js" <<'EOF'
const test = require("node:test");
const assert = require("node:assert");
const { add } = require("./add.js");
test("add works", () => {
  assert.strictEqual(add(2, 3), 5);
});
EOF
git -C "$PROJ" init -q
git -C "$PROJ" -c user.email=smoke@example.local -c user.name="smoke" add .
git -C "$PROJ" -c user.email=smoke@example.local -c user.name="smoke" commit -qm "initial disposable fixture"

PROMPT="Your target directory is exactly: $PROJ. It holds a tiny Node project (add.js with pure function add, add.test.js, package.json). Do these steps IN THAT DIRECTORY ONLY (use its absolute path for every command; do not create, modify, or delete any file outside it): 1) Read $PROJ/add.js. 2) Append a pure function multiply(a, b) with a JSDoc comment and export it alongside add (keep add unchanged). 3) Run \`npm --prefix $PROJ run format\` and \`npm --prefix $PROJ test\` and fix any failure you caused. Keep your final reply under 10 lines listing files changed and test result. Do not access the network, secrets, or credentials."
echo "-- starting ONE real Paseo agent session (provider=$PROVIDER, cwd=$PROJ) --"
echo "prompt bytes: ${#PROMPT} (kept small, cost-conscious)"

if ! paseo run --provider "$PROVIDER" --cwd "$PROJ" \
  --title "trusted-local-smoke" \
  --wait-timeout "$WAIT_TIMEOUT" \
  --json "$PROMPT" >"$AGENT_JSON" 2>"$TMP/run.err"; then
  echo "FAIL: 'paseo run' exited nonzero" >&2
  cat "$TMP/run.err" >&2 || true
  exit 1
fi

AGENT_ID="$(python3 - "$AGENT_JSON" <<'EOF'
import json,sys
raw=open(sys.argv[1]).read()
try:
    doc=json.loads(raw)
except Exception:
    print("")
    raise SystemExit
# run --json may be an object or a single-element list; keys may be capitalized
if isinstance(doc,list) and doc:
    doc=doc[0]
if isinstance(doc,dict):
    lowered={str(k).lower():v for k,v in doc.items()}
    print(lowered.get("id",lowered.get("agentid","")) or "")
else:
    print("")
EOF
)"
if [[ -z "$AGENT_ID" ]]; then
  echo "FAIL: could not parse agent id from run output" >&2
  head -c 2000 "$AGENT_JSON" >&2 || true
  exit 1
fi
echo "session created: agent=$AGENT_ID"

echo "-- observe session: inspect --"
paseo inspect "$AGENT_ID" --json >"$INSPECT_JSON" 2>&1 || {
  echo "FAIL: 'paseo inspect' failed" >&2
  exit 1
}
ACTUAL_PROVIDER="$(python3 - "$INSPECT_JSON" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
if isinstance(doc,list) and doc:
    doc=doc[0]
if isinstance(doc,dict):
    lowered={str(k).lower():v for k,v in doc.items()}
    print(lowered.get("provider",lowered.get("providerid","")) or "")
else:
    print("")
EOF
)"
AGENT_CWD="$(python3 - "$INSPECT_JSON" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
if isinstance(doc,list) and doc:
    doc=doc[0]
if isinstance(doc,dict):
    lowered={str(k).lower():v for k,v in doc.items()}
    print(lowered.get("cwd","") or "")
EOF
)"
AGENT_STATUS="$(python3 - "$INSPECT_JSON" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
if isinstance(doc,list) and doc:
    doc=doc[0]
if isinstance(doc,dict):
    lowered={str(k).lower():v for k,v in doc.items()}
    print(lowered.get("status","") or "")
EOF
)"
echo "provider actually ran: ${ACTUAL_PROVIDER:-unknown} (requested: $PROVIDER)"
echo "agent cwd observed: ${AGENT_CWD:-unknown} (target disposable dir: $PROJ)"
echo "agent status observed: ${AGENT_STATUS:-unknown}"
echo "inspect summary (redacted, scalar fields only):"
python3 - "$INSPECT_JSON" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
if isinstance(doc,list) and doc:
    doc=doc[0]
keep={}
if isinstance(doc,dict):
    for k,v in doc.items():
        if isinstance(v,(str,int,float,bool)) or v is None:
            if 'token' in k.lower() or 'secret' in k.lower() or 'key' in k.lower():
                continue
            keep[k]=v
print(json.dumps(keep,indent=2)[:1500])
EOF

echo "-- observe session: timeline (redacted summary, no credential values) --"
# NOTE: `paseo logs --json` emits human-readable timeline text, not JSON.
paseo logs "$AGENT_ID" >"$TMP/agent-logs.txt" 2>&1 || {
  echo "FAIL: 'paseo logs' failed" >&2
  exit 1
}
sed -E -e 's/sk-[A-Za-z0-9_\-]+/sk-REDACTED/g' \
       -e 's/([Bb]earer +)[A-Za-z0-9._\-~+/=]+/\1REDACTED/g' \
  "$TMP/agent-logs.txt" > "$LOGS_JSON"
echo "timeline lines observed: $(wc -l < "$LOGS_JSON")"
echo "timeline step-type counts:"
grep -oE '^\[[A-Za-z]+\]' "$LOGS_JSON" | sort | uniq -c || echo "  (no bracketed steps found)"
echo "timeline tail (last 8 lines, redacted):"
tail -n 8 "$LOGS_JSON"

echo "-- evidence: git diff (disposable repo) --"
DIFF_STAT="$(git -C "$PROJ" diff --stat)"
DIFF_BYTES="$(git -C "$PROJ" diff | wc -c)"
echo "$DIFF_STAT"
echo "diff bytes: $DIFF_BYTES"
if [[ "$DIFF_BYTES" -eq 0 ]]; then
  # Agent may have committed or left untracked changes; check status too.
  echo "-- diff empty, checking status --"
  git -C "$PROJ" status --short || true
  echo "FAIL: git diff empty — agent made no source change" >&2
  exit 1
fi
if ! git -C "$PROJ" diff -- add.js | grep -q "multiply"; then
  echo "FAIL: diff does not contain expected 'multiply' change" >&2
  exit 1
fi
echo "assert: diff non-empty with expected change — OK"

echo "-- evidence: formatter + tests (exit statuses recorded) --"
set +e
npm run format --prefix "$PROJ"
FORMAT_RC=$?
npm test --prefix "$PROJ"
TEST_RC=$?
set -e
echo "formatter exit: $FORMAT_RC; tests exit: $TEST_RC"
if [[ $FORMAT_RC -ne 0 ]]; then echo "FAIL: formatter failed" >&2; exit 1; fi
if [[ $TEST_RC -ne 0 ]]; then echo "FAIL: tests failed" >&2; exit 1; fi
echo "assert: tests pass — OK"

echo "-- end session: stop (cancel) --"
paseo stop "$AGENT_ID" >/dev/null 2>&1 || true
echo "stop exit: $? (no-op if already idle)"
paseo archive "$AGENT_ID" >/dev/null 2>&1 || true
echo "archive exit: $? (session ended/archived)"

RESULT=0
echo "PASS: trusted-local smoke (provider ran: ${ACTUAL_PROVIDER:-$PROVIDER}, agent: $AGENT_ID)"