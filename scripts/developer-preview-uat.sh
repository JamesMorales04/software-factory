#!/usr/bin/env bash
# Disposable Linux developer-preview UAT for the native Rust Factory.
#
# This script is an author/operator smoke test, not independent acceptance or
# isolation certification. It uses deterministic fixture work, a disposable
# project, a local rootless-Podman image and no paid model calls.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${FACTORY_BIN_DIR:-$REPO_ROOT/target/debug}"
IMAGE="${FACTORY_IMAGE:-localhost/software-factory-core:dev}"
FIXTURE_IMAGE="${FACTORY_VERIFIER_IMAGE:-docker.io/library/alpine:3.20}"
CORE_PORT="${FACTORY_UAT_CORE_PORT:-8877}"
SUPERVISOR_PORT="${FACTORY_UAT_SUPERVISOR_PORT:-8878}"
STAMP="$(date +%s)"
ROOT="$(mktemp -d /tmp/factory-developer-preview-XXXXXX)"
export FACTORY_CORE_URL="http://127.0.0.1:${CORE_PORT}"
export FACTORY_IMAGE="$IMAGE"
export FACTORY_CONTAINER_NAME="factory-preview-${STAMP}"
export FACTORY_VOLUME="factory-preview-data-${STAMP}"
export FACTORY_ENV_FILE="$ROOT/core.env"
export FACTORY_PROJECT_ROOTS="$ROOT/projects"
export FACTORY_BIN_DIR="$BIN_DIR"
export FACTORY_SUPERVISOR_URL="http://127.0.0.1:${SUPERVISOR_PORT}"
export FACTORY_HOST_EXECUTION=1
EVIDENCE="${FACTORY_UAT_EVIDENCE:-$PWD/developer-preview-evidence.json}"
EVIDENCE_DIR="$(dirname "$EVIDENCE")"
ARTIFACTS="$EVIDENCE_DIR/developer-preview-artifacts-${STAMP}"

cleanup() {
  set +e
  # Stop only processes recorded in this run's private runtime directory. The
  # factory CLI verifies each PID file against the expected executable before
  # signalling it, so unrelated Supervisor/Worker processes are never touched.
  if [ -f "$FACTORY_ENV_FILE" ]; then
    # shellcheck disable=SC1090
    set -a; . "$FACTORY_ENV_FILE"; set +a
    "$BIN_DIR/factory" stop >/dev/null 2>&1
  fi
  case "$FACTORY_CONTAINER_NAME" in
    factory-preview-"$STAMP") podman rm --force "$FACTORY_CONTAINER_NAME" >/dev/null 2>&1 ;;
  esac
  if [ "${FACTORY_UAT_KEEP:-0}" != "1" ]; then
    case "$FACTORY_VOLUME" in
      factory-preview-data-"$STAMP") podman volume rm "$FACTORY_VOLUME" >/dev/null 2>&1 ;;
    esac
    case "$ROOT" in
      /tmp/factory-developer-preview-*) rm -rf "$ROOT" ;;
    esac
  else
    echo "kept UAT root: $ROOT"
  fi
}
trap cleanup EXIT

require() { command -v "$1" >/dev/null 2>&1 || { echo "missing required command: $1" >&2; exit 1; }; }
require git
require curl
require python3
require podman
for binary in factory factory-core factory-supervisor factory-worker factory-verifier factory-reviewer; do
  [ -x "$BIN_DIR/$binary" ] || { echo "missing binary: $BIN_DIR/$binary (run cargo build --locked --bins)" >&2; exit 1; }
done
podman image exists "$IMAGE" || { echo "missing image $IMAGE; build deploy/Containerfile.core" >&2; exit 1; }
if ! podman image exists "$FIXTURE_IMAGE"; then podman pull "$FIXTURE_IMAGE" >/dev/null; fi

mkdir -p "$ROOT/projects/demo"
git -C "$ROOT/projects/demo" init -q
git -C "$ROOT/projects/demo" config user.name "Developer Preview UAT"
git -C "$ROOT/projects/demo" config user.email "preview@example.invalid"
printf 'seed\n' > "$ROOT/projects/demo/seed.txt"
git -C "$ROOT/projects/demo" add seed.txt
git -C "$ROOT/projects/demo" commit -qm seed

echo "== Factory start =="
"$BIN_DIR/factory" start | tee "$ROOT/start.log"
set -a; . "$FACTORY_ENV_FILE"; set +a
echo "== Factory doctor =="
"$BIN_DIR/factory" doctor | tee "$ROOT/doctor.log"
curl -fsS "$FACTORY_CORE_URL/health" > "$ROOT/health.json"
curl -fsS "$FACTORY_CORE_URL/" > "$ROOT/office.html"
grep -qi 'factory office' "$ROOT/office.html"

python3 - "$FACTORY_CORE_URL" "$ROOT" <<'PY'
import json, os, sys, urllib.request
base, root = sys.argv[1], sys.argv[2]
token = os.environ["FACTORY_OPERATOR_TOKEN"]
def api(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(base + path, data=data, method=method)
    request.add_header("content-type", "application/json")
    request.add_header("authorization", "Bearer " + token)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)
project = api("POST", "/projects", {"repository": root + "/projects/demo", "idempotency_key": "preview-project"})
pid = project["id"]
api("PUT", f"/projects/{pid}/config", {"expectedRevision": 1, "allowedPaths": ["factory-fixture-change.txt"], "capabilities": ["repository-edit", "generic-toolchain"], "qualityProfile": "generic", "active": True})
action = api("POST", "/actions", {
    "project_id": pid, "objective": "developer preview fixture", "provider": "opencode", "model": "deepseek-v4.1-flash",
    "idempotency_key": "preview-action", "allowed_paths": ["factory-fixture-change.txt"],
    "checks": [
        {"id": "functional", "kind": "functional", "description": "fixture content", "command": ["grep", "-q", "fixture candidate", "factory-fixture-change.txt"], "expectExit": 0, "timeoutSecs": 30},
        {"id": "negative", "kind": "negative", "description": "absent content rejected", "command": ["grep", "-q", "absent", "factory-fixture-change.txt"], "expectExit": 1, "timeoutSecs": 30}
    ]
})
campaign = api("POST", "/campaigns", {"action_id": action["id"], "idempotency_key": "preview-campaign"})
cid = campaign["id"]
api("POST", f"/campaigns/{cid}/authorize", {"authorization_id": "preview-auth", "risk_class": "R0", "execution_mode": "FIXTURE", "actor": "preview-operator", "rationale": "developer preview UAT", "authorized_paths": ["factory-fixture-change.txt"], "max_attempts": 1})
run = api("POST", f"/campaigns/{cid}/runs")
attempt = api("POST", f"/runs/{run['id']}/attempts")
json.dump({"project": pid, "action": action["id"], "campaign": cid, "run": run["id"], "attempt": attempt["id"]}, open(root + "/ids.json", "w"), indent=2)
print("created", pid, action["id"], cid, run["id"], attempt["id"])
PY

echo "== Waiting for candidate =="
CANDIDATE=""
for _ in $(seq 1 120); do
  CANDIDATE="$(curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" "$FACTORY_CORE_URL/candidates" | python3 -c 'import json,sys; items=json.load(sys.stdin); print(items[0]["id"] if items else "")')"
  [ -n "$CANDIDATE" ] && break
  sleep 1
done
[ -n "$CANDIDATE" ] || { echo "no candidate produced" >&2; exit 1; }
echo "candidate: $CANDIDATE"

echo "== Independent verifier =="
FACTORY_CORE_URL="$FACTORY_CORE_URL" FACTORY_VERIFIER_TOKEN="$FACTORY_VERIFIER_TOKEN" \
FACTORY_CANDIDATE_ID="$CANDIDATE" FACTORY_VERIFIER_PRINCIPAL="preview-verifier" \
FACTORY_VERIFIER_WORKSPACE="$ROOT/verifier-work" FACTORY_VERIFIER_IMAGE="$FIXTURE_IMAGE" \
FACTORY_VERIFIER_TEST_BACKEND=local FACTORY_VERIFIER_TEST_VOLUME_ROOT="$ROOT/verifier-volumes" \
"$BIN_DIR/factory-verifier" | tee "$ROOT/verifier.log"

echo "== Independent reviewer =="
FACTORY_CORE_URL="$FACTORY_CORE_URL" FACTORY_REVIEWER_TOKEN="$FACTORY_REVIEWER_TOKEN" \
FACTORY_CANDIDATE_ID="$CANDIDATE" FACTORY_REVIEWER_PRINCIPAL="preview-reviewer" \
"$BIN_DIR/factory-reviewer" | tee "$ROOT/reviewer.log"

GATE="$(curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" "$FACTORY_CORE_URL/quality/candidates/$CANDIDATE/gate")"
echo "$GATE" | python3 -c 'import json,sys; gate=json.load(sys.stdin); assert gate["outcome"]=="ACCEPTED", gate; print("gate:", gate["outcome"])'
echo "$GATE" > "$ROOT/gate.json"
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" "$FACTORY_CORE_URL/campaigns/$(python3 -c 'import json;print(json.load(open("'$ROOT'/ids.json"))["campaign"])')" > "$ROOT/campaign.json"
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" "$FACTORY_CORE_URL/model/calls?limit=10" > "$ROOT/model-calls.json" || true
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" "$FACTORY_CORE_URL/observability" > "$ROOT/observability.json" || true

echo "== Restart persistence =="
"$BIN_DIR/factory" stop | tee "$ROOT/stop.log"
"$BIN_DIR/factory" start | tee "$ROOT/restart.log"
curl -fsS -H "Authorization: Bearer $FACTORY_OPERATOR_TOKEN" "$FACTORY_CORE_URL/projects" > "$ROOT/projects-after-restart.json"
python3 - "$ROOT" <<'PY'
import json, sys
root = sys.argv[1]
projects = json.load(open(root + "/projects-after-restart.json"))
assert len(projects) == 1, projects
gate = json.load(open(root + "/gate.json"))
assert gate["outcome"] == "ACCEPTED", gate
print("persistence: PASS", {"projects": len(projects), "gate": gate["outcome"]})
PY

mkdir -p "$ARTIFACTS"
for artifact in start.log doctor.log verifier.log reviewer.log restart.log gate.json model-calls.json observability.json; do
  [ -f "$ROOT/$artifact" ] && cp "$ROOT/$artifact" "$ARTIFACTS/$artifact"
done
python3 - "$ROOT" "$EVIDENCE" "$ARTIFACTS" <<'PY'
import json, os, sys
root, evidence_path, artifacts = sys.argv[1], sys.argv[2], sys.argv[3]
ids = json.load(open(root + "/ids.json"))
gate = json.load(open(root + "/gate.json"))
record = {
    "schema": "factory.developer-preview-uat.v1",
    "result": "PASS",
    "scope": "Linux TRUSTED_LOCAL deterministic fixture; no paid model calls or isolation certification",
    "ids": ids,
    "gate": gate,
    "artifacts": {
        "directory": artifacts,
        "start": artifacts + "/start.log",
        "doctor": artifacts + "/doctor.log",
        "verifier": artifacts + "/verifier.log",
        "reviewer": artifacts + "/reviewer.log",
        "restart": artifacts + "/restart.log",
        "gate": artifacts + "/gate.json",
        "model_calls": artifacts + "/model-calls.json",
        "observability": artifacts + "/observability.json"
    }
}
os.makedirs(os.path.dirname(evidence_path) or ".", exist_ok=True)
json.dump(record, open(evidence_path, "w"), indent=2)
print("evidence:", evidence_path)
PY

"$BIN_DIR/factory" stop | tee "$ROOT/final-stop.log"
echo "DEVELOPER_PREVIEW_UAT=PASS"
