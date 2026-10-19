#!/usr/bin/env bash
# SEC-01 redesign PoC: STANDARD dev profile inside a scoped rootless container.
# Credential-free, disposable, non-privileged. Proves a capable engineering
# environment (node tests, loopback HTTP, writable workspace) can coexist with
# an OS boundary (unmounted canaries invisible, read-only root, no egress).
# Does NOT test provider credentials, Paseo integration, or auth — see
# docs/security/SEC-01-redesign-campaign.md for blocked tests.
set -euo pipefail

IMAGE="${SEC01_DEV_IMAGE:-ghcr.io/getpaseo/paseo:latest}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/sec01-dev-probe-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
WORK="$TMP/ws"; OUTSIDE="$TMP/outside"
mkdir -p "$WORK" "$OUTSIDE"
echo "OUTSIDE-CANARY" > "$OUTSIDE/canary.txt"
chmod 600 "$OUTSIDE/canary.txt"
FAIL=0
check() { # name expected actual
  if [[ "$2" == "$3" ]]; then echo "[PASS] $1";
  else echo "[FAIL] $1 (want $2, got $3)"; FAIL=1; fi
}

# --- Fixture: minimal node project with a real test.
cd "$WORK" && git init -q
cat > package.json <<'JSON'
{"name":"sec01-demo","scripts":{"test":"node --test"}}
JSON
mkdir -p src test
echo 'export function add(a,b){return a+b}' > src/add.js
cat > test/add.test.js <<'JS'
import {test} from "node:test"; import assert from "node:assert";
import {add} from "../src/add.js";
test("add",()=>assert.equal(add(1,2),3));
JS

# --- 1. Startup overhead probe (alpine, cheapest image).
T0=$(date +%s%N)
podman run --rm --network=none --read-only --cap-drop=ALL \
  --security-opt=no-new-privileges \
  docker.io/library/alpine:3.20 true
T1=$(date +%s%N)
echo "[INFO] container startup_ms=$(( (T1-T0)/1000000 ))"

# --- 2. Full dev workflow battery in the dev image.
# NOTE: stock paseo image entrypoint drops to user "paseo", which fails under
# --userns=keep-id without --user; pass --user 0:0 is WRONG (root). A custom
# Factory dev image must not UID-drop. Here we run as the current UID via
# --user "$(id -u):$(id -g)" with keep-id mapping.
OUT="$TMP/battery.txt"
if podman run --rm --network=none --read-only --cap-drop=ALL \
    --security-opt=no-new-privileges --pids-limit=64 --memory=512m --cpus=1 \
    --userns=keep-id --user "$(id -u):$(id -g)" \
    -v "$WORK:/workspace:rw" --workdir /workspace \
    -e HOME=/tmp --tmpfs /tmp:rw,size=64m \
    "$IMAGE" sh -c '
      echo "--- node test:"; node --test 2>&1 | tail -n 8
      echo "--- loopback:"; node -e "require(\"http\").createServer((q,s)=>s.end(\"hi\")).listen(18080,()=>fetch(\"http://127.0.0.1:18080/\").then(r=>r.text()).then(t=>{console.log(\"LOOPBACK:\"+t);process.exit(0)}).catch(e=>{console.log(\"LOOPBACK-FAIL:\"+e.message);process.exit(1)}))"
      echo "--- canary:"; cat /outside/canary.txt 2>/dev/null && echo READABLE || echo INACCESSIBLE
      echo "--- ro-root:"; touch /should-fail 2>/dev/null && echo WROTE-ROOT || echo ROOT-READONLY
      echo "--- workspace:"; echo PROOF > /workspace/proof.txt && cat /workspace/proof.txt
      echo "--- egress:"; (getent hosts registry.npmjs.org >/dev/null 2>&1 && echo REACHABLE || echo UNREACHABLE)
    ' >"$OUT" 2>&1; then
  grep -q "# pass 1" "$OUT"; check "node tests pass in container" "0" "$?"
  grep -q "LOOPBACK:hi" "$OUT"; check "loopback HTTP works with --network=none" "0" "$?"
  grep -q "INACCESSIBLE" "$OUT"; check "unmounted canary invisible" "0" "$?"
  grep -q "ROOT-READONLY" "$OUT"; check "read-only root enforced" "0" "$?"
  grep -q "^PROOF$" "$WORK/proof.txt" 2>/dev/null; check "workspace writable, artifact on host" "0" "$?"
  grep -q "UNREACHABLE" "$OUT"; check "default egress denied" "0" "$?"
else
  echo "[BLOCKED] dev image battery did not run (see $OUT)"; FAIL=1
fi

# --- 3. Orphan cleanup.
podman run --rm -d --name sec01-dev-probe-sleeper --network=none \
  docker.io/library/alpine:3.20 sleep 30 >/dev/null
sleep 2
podman kill sec01-dev-probe-sleeper >/dev/null 2>&1 || true
podman rm -f sec01-dev-probe-sleeper >/dev/null 2>&1 || true
if podman ps -a --format '{{.Names}}' | grep -q sec01-dev-probe-sleeper; then ORPHAN=1; else ORPHAN=0; fi
check "no orphan container remains" "0" "$ORPHAN"

if [[ "$FAIL" == "0" ]]; then echo; echo "SEC-01 DEV-PROBE: ALL CHECKS PASSED"
else echo; echo "SEC-01 DEV-PROBE: FAILURES PRESENT"; exit 1; fi
