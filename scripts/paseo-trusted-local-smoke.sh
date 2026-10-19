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
#   PROVIDER=opencode MODEL=opencode-go/muse-spark-1.3-contributor ./scripts/paseo-trusted-local-smoke.sh
#
# PROVIDER selects the Paseo provider route (no silent substitution: the
# effective provider is asserted from session evidence below). MODEL, when
# set, is passed as `--model` to pin the exact model (required for routes
# whose default model is unsupported); the effective model is likewise
# asserted from session evidence. MODE, when set, is passed as
# `paseo run --mode` (provider-specific mode, e.g. build for OpenCode).
#
# Workspace selection: the session runs in a real Paseo workspace rooted at
# the disposable project (`paseo workspace create --isolation local
# --path`), and the effective session Cwd is ASSERTED to equal the target
# (hard FAIL otherwise). Prompt absolute paths are belt-and-braces only.
# (`paseo run --cwd` alone does NOT set the session Cwd — verified:
# agent-scoped runs inherit the caller workspace Cwd, which pushes target
# files outside OpenCode's default agent into `external_directory = ask`.)
#
# Asserts: requested provider/model match session evidence (no silent
# substitution), git diff non-empty and confined to the disposable repo,
# npm tests pass, stop/archive truly succeed, cleanup done. At most ONE
# continuation message is ever sent (bounded second wait); a still-empty
# diff afterwards is a FAIL. Exits nonzero on first failure.
set -euo pipefail

PROVIDER="${PROVIDER:-codex}"
if [[ "$PROVIDER" != "codex" && "$PROVIDER" != "opencode" ]]; then
  echo "FAIL: PROVIDER must be codex|opencode (got: $PROVIDER)" >&2
  exit 1
fi
# Optional exact model pin, passed as `paseo run --model` when set.
MODEL="${MODEL:-}"
# Optional explicit provider mode, passed as `paseo run --mode` when set
# (provider-specific; e.g. build for OpenCode, auto for Codex).
# Default when unset is provider-appropriate for TRUSTED_LOCAL: codex runs
# full-access (fresh disposable workspaces are untrusted, so mode auto
# gates shell/edit behind approvals and the run stalls at status
# "permission"; TRUSTED_LOCAL explicitly accepts broad access, and the
# target stays disposable + post-hoc verified), opencode runs build
# (default agent with default-allow workspace tools).
MODE="${MODE:-}"
if [[ -z "$MODE" ]]; then
  if [[ "$PROVIDER" == "codex" ]]; then
    MODE="full-access"
  else
    MODE="build"
  fi
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
  if [[ -n "${WS_ID:-}" ]]; then
    echo "-- cleanup: archive smoke workspace $WS_ID (best effort) --"
    paseo workspace archive "$WS_ID" >/dev/null 2>&1 || true
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
echo "-- disposable Paseo workspace rooted at $PROJ (real session Cwd, not a prompt hint) --"
paseo workspace create --isolation local --path "$PROJ" \
  --title "trusted-local-smoke" --json >"$TMP/workspace.json" 2>&1 || {
  echo "FAIL: 'paseo workspace create' failed" >&2
  exit 1
}
WS_ID="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); d=d[0] if isinstance(d,list) and d else d; print(d.get("workspaceId",d.get("id","")) or "")' "$TMP/workspace.json")"
if [[ -z "$WS_ID" ]]; then
  echo "FAIL: could not parse workspace id (top-level keys only, no values):" >&2
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); d=d[0] if isinstance(d,list) and d else d; print(sorted(d.keys()) if isinstance(d,dict) else type(d).__name__)' "$TMP/workspace.json" >&2 || true
  exit 1
fi
echo "workspace created: id=$WS_ID (path=$PROJ)"
echo "-- starting ONE real Paseo agent session (provider=$PROVIDER, mode=$MODE, workspace=$WS_ID) --"
echo "prompt bytes: ${#PROMPT} (kept small, cost-conscious)"

RUN_ARGS=(--provider "$PROVIDER" --workspace "$WS_ID" --mode "$MODE"
  --title "trusted-local-smoke"
  --wait-timeout "$WAIT_TIMEOUT")
if [[ -n "$MODEL" ]]; then
  RUN_ARGS+=(--model "$MODEL")
fi
RUN_RC=0
set +e
paseo run "${RUN_ARGS[@]}" \
  --json "$PROMPT" >"$AGENT_JSON" 2>"$TMP/run.err"
RUN_RC=$?
set -e
if [[ $RUN_RC -ne 0 ]]; then
  echo "FAIL: 'paseo run' exited nonzero (exit=$RUN_RC; provider output suppressed — rerun manually to debug)" >&2
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
observe_timeline() {
  paseo logs "$AGENT_ID" >"$TMP/agent-logs.txt" 2>&1 || {
    echo "FAIL: 'paseo logs' failed" >&2
    exit 1
  }
  redact "$TMP/agent-logs.txt" > "$LOGS_JSON"
  echo "timeline lines observed: $(wc -l < "$LOGS_JSON")"
  echo "timeline step-type counts (headers only — bodies suppressed, never printed):"
  grep -oE '^\[[A-Za-z]+\]' "$LOGS_JSON" | sort | uniq -c || echo "  (no bracketed steps found)"
}
observe_timeline

echo "-- containment tripwire: no observed file-write step outside the disposable dir --"
# Defense in depth behind the asserted session Cwd above. This tripwire
# checks the observed evidence only: absolute paths in Edit/Write/Create/
# Apply timeline lines. It does NOT cover shell-output paths, relative
# paths, or writes invisible in the timeline — so it narrows, never proves,
# containment. The disposable-repo diff below is the primary evidence.
if grep -E '^\[(Edit|Write|Create|Apply)' "$LOGS_JSON" | grep -oE '/[^ "\"'"'"'`,;]*' | grep -v -F "$TMP" | grep -q .; then
  echo "FAIL: file-write step references a path outside the disposable dir (count: $(grep -E '^\[(Edit|Write|Create|Apply)' "$LOGS_JSON" | grep -oE '/[^ "\"'"'"'`,;]*' | grep -v -F "$TMP" | sort -u | wc -l); paths suppressed)" >&2
  exit 1
fi
echo "-- assert: session Cwd equals the disposable target (real workspace selection, hard FAIL otherwise) --"
if [[ -z "${AGENT_CWD:-}" ]]; then
  echo "FAIL: session Cwd missing from inspect output" >&2
  exit 1
fi
# Normalize trailing slash and $HOME-relative display before comparing.
NORM_CWD="${AGENT_CWD%/}"
NORM_PROJ="${PROJ%/}"
if [[ "$NORM_CWD" == "~"* ]]; then
  NORM_CWD="$HOME${NORM_CWD#"~"}"
fi
echo "session Cwd observed: $AGENT_CWD (target: $PROJ)"
if [[ "$NORM_CWD" != "$NORM_PROJ" ]]; then
  echo "FAIL: session Cwd ($AGENT_CWD) != disposable target ($PROJ) — workspace selection not enforced" >&2
  exit 1
fi
echo "assert: session Cwd is the disposable target — OK"

echo "-- evidence: git diff (disposable repo) --"
# Single bounded continuation: some provider routes return after a
# productive partial turn (e.g. reads only). If the diff is empty BUT the
# timeline shows real tool activity in the target dir, send ONE follow-up
# nudge in the same session (bounded second wait), then evaluate finally.
# A second empty diff is a FAIL, never a second nudge.
NUDGE_USED=0
if [[ -z "$(git -C "$PROJ" diff --stat)" ]]; then
  TOOL_STEPS=$(grep -cE '^\[(Read|Glob|Grep|List|Shell|Edit|Write|Create|Apply)' "$LOGS_JSON" || true)
  if [[ "${TOOL_STEPS:-0}" -gt 0 ]]; then
    echo "-- one bounded continuation nudge (partial productive turn: $TOOL_STEPS tool steps, empty diff) --"
    if paseo send --no-wait "$AGENT_ID" "Continue exactly where you left off in $PROJ: finish the multiply() edit now, run format and tests, keep your reply under 10 lines." >/dev/null 2>&1 \
      && paseo wait --timeout 180 "$AGENT_ID" >/dev/null 2>&1; then
      NUDGE_USED=1
      observe_timeline
    else
      echo "continuation nudge failed or timed out; evaluating partial evidence" >&2
    fi
  fi
fi
echo "continuation nudges used: $NUDGE_USED (max 1)"
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
WS_RC=0
paseo stop "$AGENT_ID" >/dev/null 2>&1 || STOP_RC=$?
echo "stop exit: $STOP_RC (no-op when already idle)"
if [[ $STOP_RC -ne 0 ]]; then echo "FAIL: 'paseo stop' failed (rc=$STOP_RC)" >&2; exit 1; fi
paseo archive "$AGENT_ID" >/dev/null 2>&1 || ARCHIVE_RC=$?
echo "archive exit: $ARCHIVE_RC"
if [[ $ARCHIVE_RC -ne 0 ]]; then echo "FAIL: 'paseo archive' failed (rc=$ARCHIVE_RC)" >&2; exit 1; fi
paseo workspace archive "$WS_ID" >/dev/null 2>&1 || WS_RC=$?
echo "workspace archive exit: $WS_RC"
if [[ $WS_RC -ne 0 ]]; then echo "FAIL: 'paseo workspace archive' failed (rc=$WS_RC)" >&2; exit 1; fi
if paseo workspace ls --json 2>/dev/null | grep -q "$WS_ID"; then
  echo "FAIL: smoke workspace still listed after archive" >&2
  exit 1
fi
echo "assert: session ended and smoke workspace deregistered — OK"

RESULT=0
echo "PASS: trusted-local smoke (provider ran: ${ACTUAL_PROVIDER:-$PROVIDER}, model ran: ${EFFECTIVE_MODEL:-unknown}, nudges: $NUDGE_USED, agent: $AGENT_ID)"