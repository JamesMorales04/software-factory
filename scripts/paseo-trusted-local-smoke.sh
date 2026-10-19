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
#   PROVIDER=opencode MODEL=github-copilot/gpt-5-mini ./scripts/paseo-trusted-local-smoke.sh
#
# PROVIDER selects the Paseo provider route (no silent substitution: the
# effective provider is asserted from session evidence below). MODEL, when
# set, is passed as `--model` to pin the exact model (required for routes
# whose default model is unsupported); the effective model is likewise
# asserted from session evidence.
#
# Workspace note: `paseo run --cwd` is a launch hint, NOT an enforced
# session working directory (observed: session Cwd stays the caller
# workspace). Containment of this test therefore rests on absolute-path
# prompt pinning PLUS post-hoc verification (diff confined to the
# disposable repo; no file-write step outside it) — never on --cwd alone.
#
# Asserts: requested provider/model match session evidence (no silent
# substitution), git diff non-empty and confined to the disposable repo,
# npm tests pass, stop/archive truly succeed, cleanup done.
# Exits nonzero on first failure.
set -euo pipefail

PROVIDER="${PROVIDER:-codex}"
if [[ "$PROVIDER" != "codex" && "$PROVIDER" != "opencode" ]]; then
  echo "FAIL: PROVIDER must be codex|opencode (got: $PROVIDER)" >&2
  exit 1
fi
# Optional exact model pin, passed as `paseo run --model` when set.
MODEL="${MODEL:-}"

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

# redact <file>: mask secret-shaped values before anything reaches the
# terminal. Covers API keys, bearer tokens, and KEY/TOKEN/SECRET-style
# assignments. Raw provider/session/status output must never be printed.
redact() {
  sed -E -e 's/sk-[A-Za-z0-9_\-]+/sk-REDACTED/g' \
         -e 's/([Bb]earer +)[A-Za-z0-9._\-~+/=]+/\1REDACTED/g' \
         -e 's/([A-Za-z_]*?(TOKEN|SECRET|KEY|PASSWORD|COOKIE|AUTH)[A-Za-z_]*=)[^ "&,]+/\1REDACTED/g' "$1"
}

echo "-- provider availability (names/presence only, no secret values) --"
paseo status --json >"$TMP/status.json" 2>&1 || {
  echo "FAIL: 'paseo status' failed" >&2
  exit 1
}
# Strict allowlist: provider id/label/availability only. Never dump raw
# status output, which may carry secret-bearing fields.
python3 - "$TMP/status.json" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
provs=doc.get("providers") or doc.get("status",{}).get("providers") or []
rows=[]
if isinstance(provs,dict):
    provs=[{"id":k,**v} if isinstance(v,dict) else {"id":k,"value":v} for k,v in provs.items()]
for p in provs if isinstance(provs,list) else []:
    if not isinstance(p,dict):
        continue
    rows.append({k:p.get(k) for k in ("id","label","available","status","enabled") if isinstance(p.get(k),(str,bool)) or p.get(k) is None})
print(json.dumps(rows,indent=2)[:1500])
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

RUN_ARGS=(--provider "$PROVIDER" --cwd "$PROJ"
  --title "trusted-local-smoke"
  --wait-timeout "$WAIT_TIMEOUT")
if [[ -n "$MODEL" ]]; then
  RUN_ARGS+=(--model "$MODEL")
fi
if ! paseo run "${RUN_ARGS[@]}" \
  --json "$PROMPT" >"$AGENT_JSON" 2>"$TMP/run.err"; then
  echo "FAIL: 'paseo run' exited nonzero (exit=$?; provider output suppressed — rerun manually to debug)" >&2
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
  echo "FAIL: could not parse agent id from run output (top-level keys only, no values):" >&2
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); d=d[0] if isinstance(d,list) and d else d; print(sorted(d.keys()) if isinstance(d,dict) else type(d).__name__)' "$AGENT_JSON" >&2 || true
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
# Normalize combined "provider/..." forms before comparing; the effective
# route must still match the requested provider (no silent substitution).
ACTUAL_PROVIDER="${ACTUAL_PROVIDER%%/*}"
if [[ -n "$PROVIDER" && -n "${ACTUAL_PROVIDER:-}" && "$ACTUAL_PROVIDER" != "$PROVIDER" ]]; then
  echo "FAIL: effective provider '${ACTUAL_PROVIDER}' != requested '${PROVIDER}' (no silent substitution)" >&2
  exit 1
fi
EFFECTIVE_MODEL="$(python3 - "$INSPECT_JSON" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
if isinstance(doc,list) and doc:
    doc=doc[0]
if isinstance(doc,dict):
    lowered={str(k).lower():v for k,v in doc.items()}
    print(lowered.get("model","") or "")
else:
    print("")
EOF
)"
echo "model actually ran: ${EFFECTIVE_MODEL:-unknown}${MODEL:+ (pinned: $MODEL)}"
if [[ -n "$MODEL" && -n "${EFFECTIVE_MODEL:-}" && "$EFFECTIVE_MODEL" != "$MODEL" ]]; then
  echo "FAIL: effective model '${EFFECTIVE_MODEL}' != pinned '${MODEL}'" >&2
  exit 1
fi
echo "agent cwd observed: ${AGENT_CWD:-unknown} (target disposable dir: $PROJ)"
echo "agent status observed: ${AGENT_STATUS:-unknown}"
echo "inspect summary (strict allowlist of safe scalar fields only):"
python3 - "$INSPECT_JSON" <<'EOF'
import json,sys
doc=json.load(open(sys.argv[1]))
if isinstance(doc,list) and doc:
    doc=doc[0]
# Allowlist: identity/routing/observation fields only. Anything else
# (credentials, tokens, usage, raw provider payloads) is never printed.
SAFE=("Id","Name","Provider","Model","Thinking","Status","Archived",
      "ArchivedAt","Mode","Cwd","CreatedAt","UpdatedAt","Worktree",
      "ParentAgentId")
keep={}
if isinstance(doc,dict):
    lowered={str(k).lower():k for k in doc.keys()}
    for name in SAFE:
        k=lowered.get(name.lower())
        if k is None:
            continue
        v=doc[k]
        if isinstance(v,(str,int,float,bool)) or v is None:
            keep[k]=v
print(json.dumps(keep,indent=2)[:1500])
EOF

echo "-- observe session: timeline (redacted summary, no credential values) --"
# NOTE: `paseo logs` emits human-readable timeline text, not JSON.
paseo logs "$AGENT_ID" >"$TMP/agent-logs.txt" 2>&1 || {
  echo "FAIL: 'paseo logs' failed" >&2
  exit 1
}
redact "$TMP/agent-logs.txt" > "$LOGS_JSON"
echo "timeline lines observed: $(wc -l < "$LOGS_JSON")"
echo "timeline step-type counts:"
grep -oE '^\[[A-Za-z]+\]' "$LOGS_JSON" | sort | uniq -c || echo "  (no bracketed steps found)"
echo "timeline tail (bracketed step lines only, truncated, redacted):"
grep -E '^\[[A-Za-z]+\]' "$LOGS_JSON" | cut -c1-200 | tail -n 8

echo "-- containment tripwire: no observed file-write step outside the disposable dir --"
# --cwd is NOT enforced session containment (see header). This tripwire
# checks the observed evidence only: absolute paths in Edit/Write/Create/
# Apply timeline lines. It does NOT cover shell-output paths, relative
# paths, or writes invisible in the timeline — so it narrows, never proves,
# containment. The disposable-repo diff below is the primary evidence.
if grep -E '^\[(Edit|Write|Create|Apply)' "$LOGS_JSON" | grep -oE '/[^ "\"'"'"'`,;]*' | grep -v -F "$TMP" | grep -q .; then
  echo "FAIL: file-write step references a path outside $TMP:" >&2
  grep -E '^\[(Edit|Write|Create|Apply)' "$LOGS_JSON" | grep -oE '/[^ "\"'"'"'`,;]*' | grep -v -F "$TMP" | sort -u | head -n 10 >&2
  exit 1
fi
echo "assert: no out-of-dir file-write steps observed in timeline — OK (Edit/Write lines only; see limits above)"
if [[ -n "${AGENT_CWD:-}" && "$AGENT_CWD" != "$PROJ" && "$AGENT_CWD" != "$PROJ/" ]]; then
  echo "CWD-NOTE: --cwd did not set session Cwd (observed: $AGENT_CWD; target: $PROJ); containment verified post-hoc above, not via --cwd."
fi

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

echo "-- end session: stop then archive (real exit statuses asserted) --"
STOP_RC=0
ARCHIVE_RC=0
paseo stop "$AGENT_ID" >/dev/null 2>&1 || STOP_RC=$?
echo "stop exit: $STOP_RC (no-op when already idle)"
if [[ $STOP_RC -ne 0 ]]; then echo "FAIL: 'paseo stop' failed (rc=$STOP_RC)" >&2; exit 1; fi
paseo archive "$AGENT_ID" >/dev/null 2>&1 || ARCHIVE_RC=$?
echo "archive exit: $ARCHIVE_RC"
if [[ $ARCHIVE_RC -ne 0 ]]; then echo "FAIL: 'paseo archive' failed (rc=$ARCHIVE_RC)" >&2; exit 1; fi

RESULT=0
echo "PASS: trusted-local smoke (provider ran: ${ACTUAL_PROVIDER:-$PROVIDER}, agent: $AGENT_ID)"