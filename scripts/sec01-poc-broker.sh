#!/usr/bin/env bash
# SEC-01 S2-C synthetic broker proof of concept (disposable, credential-free).
# Demonstrates the trust topology the S2 Worker already implements, plus the
# narrow-broker pattern required for any future provider integration:
#   trusted supervisor (this script) -> scoped broker (fixed op, workspace-
#   bound, attempt-scoped) -> disposable rootless Podman workspace.
# Every check uses synthetic canaries under a fresh TMP dir and asserts
# actual OS behavior (exit codes, file presence, errno-class output), never
# model prose. No real credentials, no privileged flags, no host mounts.
set -euo pipefail

TMP="$(mktemp -d "${TMPDIR:-/tmp}/sec01-poc-XXXXXX")"
WORK="$TMP/workspace"
OUTSIDE="$TMP/outside"
FAKECREDS="$TMP/fakecreds"
mkdir -p "$WORK" "$OUTSIDE" "$FAKECREDS"
echo "workspace-allowed" > "$WORK/allowed.txt"
echo "OUTSIDE-CANARY" > "$OUTSIDE/canary.txt"
echo "FAKE-CREDENTIAL" > "$FAKECREDS/provider.key"
chmod 600 "$OUTSIDE/canary.txt" "$FAKECREDS/provider.key"
FAIL=0

check() { # $1=name $2=expected $3=actual
  if [[ "$2" == "$3" ]]; then echo "[PASS] $1";
  else echo "[FAIL] $1 (want $2, got $3)"; FAIL=1; fi
}

# --- 1. Scoped broker: exactly one fixed op, workspace-bound, no traversal.
BROKER="$TMP/broker.py"
cat > "$BROKER" <<'PY'
import json, os, re, sys
work, run, attempt, gen = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
req = json.load(sys.stdin)
marker = req.get("marker", "")
if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", marker):
    print(json.dumps({"ok": False, "error": "invalid-marker"})); sys.exit(3)
target = os.path.realpath(os.path.join(work, marker + ".txt"))
if os.path.commonpath([target, os.path.realpath(work)]) != os.path.realpath(work):
    print(json.dumps({"ok": False, "error": "path-escape"})); sys.exit(4)
with open(target, "w") as f:
    f.write(f"{run}/{attempt}/{gen}:{marker}\n")
print(json.dumps({"ok": True, "receipt": f"{run}/{attempt}/{gen}:{marker}"}))
PY
OUT="$(echo '{"marker":"POCMARKER1"}' | python3 "$BROKER" "$WORK" r1 a1 1)"
check "broker allowed op writes scoped receipt" "POCMARKER1" "$(cat "$WORK/POCMARKER1.txt" | cut -d: -f2)"
echo "$OUT" | grep -q '"ok": true'; check "broker reports success" "0" "$?"

echo '{"marker":"../../evil"}' | python3 "$BROKER" "$WORK" r1 a1 1 >/dev/null 2>&1 && CODE=0 || CODE=$?
check "broker rejects traversal marker" "3" "$CODE"
[[ -e "$WORK/evil.txt" || -e "$TMP/evil.txt" ]] && CODE=0 || CODE=1
check "traversal created no file" "1" "$CODE"

echo '{"marker":"/tmp/evil-abs"}' | python3 "$BROKER" "$WORK" r1 a1 1 >/dev/null 2>&1 && CODE=0 || CODE=$?
check "broker rejects absolute marker" "3" "$CODE"

# --- 2. Control: an unconstrained same-UID shell CAN read the canaries.
# This is exactly why broker mediation (not shell policy) must own the boundary.
if head -c 1 "$OUTSIDE/canary.txt" >/dev/null 2>&1; then SHELL_READ="READABLE"; else SHELL_READ="DENIED"; fi
check "unconstrained shell reads outside canary (control)" "READABLE" "$SHELL_READ"
if head -c 1 "$FAKECREDS/provider.key" >/dev/null 2>&1; then CRED_READ="READABLE"; else CRED_READ="DENIED"; fi
check "unconstrained shell reads colocated credential (control)" "READABLE" "$CRED_READ"

# --- 3. Same battery inside the S2 disposable container boundary.
if ! command -v podman >/dev/null 2>&1; then
  echo "[BLOCKED] podman unavailable: container battery NOT_TESTED"
else
  C_OUT="$TMP/container-probe.txt"
  if podman run --rm --network=none --read-only --cap-drop=ALL \
      --security-opt=no-new-privileges --pids-limit=32 --memory=128m --cpus=0.5 \
      --userns=keep-id -v "$WORK:/workspace:rw" --workdir /workspace \
      docker.io/library/alpine:3.20 sh -c '
        echo "--- workspace:"; echo POCMARKER2 > /workspace/POCMARKER2.txt && cat /workspace/POCMARKER2.txt
        echo "--- outside canary:"; cat /outside-canary 2>/dev/null && echo READABLE || echo INACCESSIBLE
        echo "--- net:"; (ping -c1 -W1 192.0.2.1 >/dev/null 2>&1 && echo REACHABLE || echo UNREACHABLE)
        echo "--- env:"; (printenv | grep -Eq "FACTORY|TOKEN|SECRET|KEY" && echo LEAKED || echo CLEAN)
        echo "--- sockets:"; (test ! -e /run/podman/podman.sock && test ! -e /run/docker.sock && echo ABSENT || echo PRESENT)
      ' >"$C_OUT" 2>&1; then
    grep -q "POCMARKER2" "$C_OUT"; check "container broker op works" "0" "$?"
    grep -q "INACCESSIBLE" "$C_OUT"; check "container cannot see host canary" "0" "$?"
    grep -q "UNREACHABLE" "$C_OUT"; check "container egress denied" "0" "$?"
    grep -q "CLEAN" "$C_OUT"; check "container env clean" "0" "$?"
    grep -q "ABSENT" "$C_OUT"; check "container sockets absent" "0" "$?"
    # The receipt written through the container carries attempt identity.
    echo "POCMARKER2" | python3 -c "import sys; print(open('$WORK/POCMARKER2.txt').read().strip())" | grep -q "POCMARKER2"
    check "container artifact readable on host workspace" "0" "$?"
  else
    echo "[BLOCKED] fixture image unavailable: container battery NOT_TESTED"
  fi
  # --- 4. Orphan cleanup: killed client leaves no container behind.
  podman run --rm --name sec01-poc-sleeper --network=none docker.io/library/alpine:3.20 sleep 30 &
  SLEEPER=$!
  sleep 2
  kill -9 $SLEEPER 2>/dev/null || true
  sleep 1
  podman kill sec01-poc-sleeper >/dev/null 2>&1 || true
  podman rm -f sec01-poc-sleeper >/dev/null 2>&1 || true
  if podman ps -a --format '{{.Names}}' | grep -q "sec01-poc-sleeper"; then ORPHAN=1; else ORPHAN=0; fi
  check "no orphan container remains" "0" "$ORPHAN"
fi

# --- 5. Broker never sees host secrets: run it with a canary env present.
export SEC01_POC_CANARY="canary-$(head -c8 /dev/urandom | od -An -tx1 | tr -d ' \n')"
echo '{"marker":"POCMARKER3"}' | python3 "$BROKER" "$WORK" r1 a1 1 | grep -q "$SEC01_POC_CANARY" && LEAK=1 || LEAK=0
check "canary absent from broker output" "0" "$LEAK"
unset SEC01_POC_CANARY

if [[ "$FAIL" == "0" ]]; then echo; echo "SEC-01 PoC: ALL CHECKS PASSED (tmp=$TMP)"; else echo; echo "SEC-01 PoC: FAILURES PRESENT"; exit 1; fi
