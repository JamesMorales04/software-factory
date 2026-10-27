#!/usr/bin/env bash
# Deterministic one-command Compose acceptance for the developer-preview stack.
# It never starts paid inference and never prints an operator credential.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PROJECT="${FACTORY_E2E_PROJECT:-software-factory-e2e}"
PORT="${FACTORY_PORT:-8894}"
BASE="http://127.0.0.1:${PORT}"
PROVIDER="${PODMAN_COMPOSE_PROVIDER:-/usr/bin/podman-compose}"
if [[ -z "$PROJECT" || "$PROJECT" == "software-factory" || "$PROJECT" != *e2e* ]]; then
  printf 'FACTORY_E2E_PROJECT must name a dedicated project containing "e2e"; refusing %s\n' "$PROJECT" >&2
  exit 2
fi
GIT_DIR="$(mktemp -d /tmp/factory-compose-e2e-git.XXXXXXXX)"
COMPOSE=(podman compose -f compose.yaml -f compose.e2e.yaml)
export COMPOSE_PROJECT_NAME="$PROJECT"
export FACTORY_PORT="$PORT"
export FACTORY_E2E_GIT_DIR="$GIT_DIR/server"
export PODMAN_COMPOSE_PROVIDER="$PROVIDER"
export FACTORY_PROVIDER_KIND=fixture

cleanup() {
  "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
  rm -rf "$GIT_DIR"
}
if [[ "${FACTORY_E2E_KEEP:-0}" == "1" ]]; then
  trap 'rm -rf "$GIT_DIR"' EXIT
else
  trap cleanup EXIT
fi

log() { printf '%s\n' "$*" >&2; }

compose() { "${COMPOSE[@]}" "$@"; }

wait_for() {
  local url="$1" deadline=$((SECONDS + ${2:-180}))
  until curl -fsS "$url" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      log "timed out waiting for $url"
      return 1
    fi
    sleep 1
  done
}

wait_core() {
  local url="$1" deadline=$((SECONDS + ${2:-180}))
  until podman exec "$CORE" curl -fsS "$url" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      log "timed out waiting for $url from Core"
      return 1
    fi
    sleep 1
  done
}

core_container() {
  podman ps --filter "label=com.docker.compose.project=$PROJECT" \
    --filter 'label=com.docker.compose.service=core' \
    --format '{{.Names}}' | head -1
}

api() {
  local method="$1" path="$2" body="${3:-}" response status payload
  if [[ -n "$body" ]]; then
    response=$(curl -sS -X "$method" "$BASE$path" \
      -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
      -d "$body" -w $'\n%{http_code}')
  else
    response=$(curl -sS -X "$method" "$BASE$path" \
      -H "authorization: Bearer $TOKEN" -w $'\n%{http_code}')
  fi
  status="${response##*$'\n'}"
  payload="${response%$'\n'*}"
  if [[ "$status" -ge 400 ]]; then
    log "HTTP $status for $method $path: $payload"
    return 1
  fi
  printf '%s' "$payload"
}

json() { python3 -c "$1"; }

log "== preparing disposable Git fixture"
mkdir -p "$GIT_DIR/source/src"
git -C "$GIT_DIR/source" init -q
git -C "$GIT_DIR/source" config user.name 'Compose E2E'
git -C "$GIT_DIR/source" config user.email 'compose-e2e@example.invalid'
printf 'seed\n' > "$GIT_DIR/source/src/seed.txt"
git -C "$GIT_DIR/source" add src/seed.txt
git -C "$GIT_DIR/source" commit -qm 'seed'
mkdir -p "$GIT_DIR/server"
git clone -q --bare "$GIT_DIR/source" "$GIT_DIR/server/repo.git"
git -C "$GIT_DIR/server/repo.git" update-server-info

log "== starting the stack with one Compose command"
compose down -v >/dev/null 2>&1 || true
compose up -d --build

CORE="$(core_container)"
[[ -n "$CORE" ]] || { log "Core container was not created"; exit 1; }
wait_for "$BASE/health" 300
wait_for "http://127.0.0.1:${PORT}/core-api/health" 60
log "== waiting for Supervisor, runtime engine and Worker readiness"
wait_core http://supervisor:8766/health 180
wait_core http://runtime:8770/health 600
GIT_READY=0
for _ in $(seq 1 60); do
  if podman exec "$CORE" curl -fsS http://git-fixture:8080/repo.git/HEAD >/dev/null 2>&1; then
    GIT_READY=1
    break
  fi
  sleep 1
done
[[ "$GIT_READY" -eq 1 ]] || { log "Git fixture did not become reachable"; exit 1; }

log "== first-run enrollment"
CODE="$(podman exec "$CORE" cat /data/enrollment-code)"
TOKEN="$(curl -fsS -X POST "$BASE/setup/enroll" -H 'content-type: application/json' \
  -d "$(python3 -c 'import json,sys; print(json.dumps({"code":sys.argv[1]}))' "$CODE")" \
  | json 'import json,sys; print(json.load(sys.stdin)["token"])')"
STATUS="$(curl -fsS "$BASE/setup/status")"
[[ "$STATUS" == *'"required":false'* ]] || { log "enrollment was not invalidated"; exit 1; }
for _ in $(seq 1 60); do
  if api GET /workers 2>/dev/null | grep -q 'compose-worker'; then
    break
  fi
  sleep 1
done
api GET /workers | grep -q 'compose-worker' || { log "Worker did not register"; exit 1; }

log "== importing a Git project"
PROJECT_ID="$(api POST /projects \
  '{"repository":"http://git-fixture:8080/repo.git","idempotency_key":"e2e-project"}' \
  | json 'import json,sys; print(json.load(sys.stdin)["id"])')"
CONFIG_REV="$(api GET "/projects/$PROJECT_ID/config" | json 'import json,sys; print(json.load(sys.stdin)["revision"])')"
api PUT "/projects/$PROJECT_ID/config" \
  "{\"expectedRevision\":$CONFIG_REV,\"allowedPaths\":[\"src\"],\"capabilities\":[\"repository-edit\",\"generic-toolchain\"],\"qualityProfile\":\"generic\",\"active\":true}" >/dev/null

log "== freezing an Action with functional and negative checks"
ACTION_ID="$(api POST /actions \
  "{\"project_id\":\"$PROJECT_ID\",\"objective\":\"compose deterministic candidate\",\"provider\":\"opencode\",\"model\":\"deepseek-v4.1-flash\",\"idempotency_key\":\"e2e-action\",\"allowed_paths\":[\"src\"],\"checks\":[{\"id\":\"functional\",\"kind\":\"functional\",\"description\":\"candidate content present\",\"command\":[\"grep\",\"-q\",\"provider candidate\",\"src/provider.txt\"],\"expectExit\":0,\"timeoutSecs\":30},{\"id\":\"negative\",\"kind\":\"negative\",\"description\":\"absent content rejected\",\"command\":[\"grep\",\"-q\",\"absent\",\"src/provider.txt\"],\"expectExit\":1,\"timeoutSecs\":30}]}" \
  | json 'import json,sys; print(json.load(sys.stdin)["id"])')"

log "== creating a scoped delegation"
EXPIRES="$(python3 -c 'import datetime; print((datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(days=1)).isoformat().replace("+00:00","Z"))')"
api POST /delegations \
  "{\"delegation\":{\"id\":\"e2e-grant\",\"principalId\":\"owner\",\"auditId\":\"e2e\",\"projectId\":\"$PROJECT_ID\",\"revision\":1,\"contentHash\":\"\",\"objectiveClasses\":[\"feature\"],\"allowedPaths\":[\"src\"],\"capabilities\":[\"repository-edit\",\"generic-toolchain\"],\"riskClasses\":[\"R0\",\"R1\"],\"autonomousStart\":false,\"autonomousRiskClasses\":[],\"route\":{\"provider\":\"opencode\",\"model\":\"deepseek-v4.1-flash\",\"mode\":\"build\"},\"maxAttempts\":2,\"maxTimeoutSecs\":300,\"maxRepairCycles\":0,\"maxWallClockSecs\":1800,\"maxParallel\":1,\"maxSessions\":16,\"maxRequests\":4,\"maxCumulativeSessions\":32,\"maxCumulativeAttempts\":32,\"expiresAt\":\"$EXPIRES\"},\"idempotency_key\":\"e2e-delegation\"}" >/dev/null

log "== authorizing and dispatching a Campaign"
CAMPAIGN_ID="$(api POST /campaigns \
  "{\"action_id\":\"$ACTION_ID\",\"idempotency_key\":\"e2e-campaign\"}" \
  | json 'import json,sys; print(json.load(sys.stdin)["id"])')"
api POST "/campaigns/$CAMPAIGN_ID/authorize" \
  "{\"authorization_id\":\"e2e-auth\",\"risk_class\":\"R0\",\"execution_mode\":\"TRUSTED_LOCAL_PROVIDER\",\"actor\":\"e2e-operator\",\"rationale\":\"deterministic Compose acceptance\",\"authorized_paths\":[\"src\"],\"max_attempts\":1,\"provider\":\"opencode\",\"model\":\"deepseek-v4.1-flash\",\"provider_mode\":\"build\",\"delegation_id\":\"e2e-grant\",\"max_sessions\":1,\"max_turns\":2,\"max_input_tokens\":20000,\"max_output_tokens\":8000,\"max_cost_micros\":500000,\"deadline_secs\":120,\"allowed_tools\":[\"write_file\"]}" >/dev/null
RUN_ID="$(api POST "/campaigns/$CAMPAIGN_ID/runs" | json 'import json,sys; print(json.load(sys.stdin)["id"])')"
ATTEMPT_ID="$(api POST "/runs/$RUN_ID/attempts" | json 'import json,sys; print(json.load(sys.stdin)["id"])')"

log "== waiting for the candidate, verifier, reviewer and gate"
CANDIDATE_ID=""
GATE=""
for _ in $(seq 1 300); do
  CANDIDATE_ID="$(api GET /candidates \
    | python3 -c 'import json,sys; target=sys.argv[1]; print(next((c["id"] for c in json.load(sys.stdin) if c["attempt_id"]==target), ""))' "$ATTEMPT_ID")"
  if [[ -n "$CANDIDATE_ID" ]]; then
    GATE="$(api GET "/quality/candidates/$CANDIDATE_ID/gate" 2>/dev/null || true)"
    if [[ "$GATE" == *'"outcome":"ACCEPTED"'* ]]; then
      break
    fi
  fi
  sleep 1
done
[[ "$GATE" == *'"outcome":"ACCEPTED"'* ]] || { log "gate did not accept: ${GATE:-none}"; exit 1; }
RECEIPTS="$(api GET "/quality/candidates/$CANDIDATE_ID/receipts")"
[[ "$RECEIPTS" == *'"role":"verifier"'* && "$RECEIPTS" == *'"role":"reviewer"'* ]] || {
  log "missing independent receipts"; exit 1;
}
[[ "$RECEIPTS" != *'"result":"FAIL"'* ]] || { log "receipt failed"; exit 1; }

log "== verifying durable restart"
compose down >/dev/null
compose up -d
CORE="$(core_container)"
[[ -n "$CORE" ]] || { log "Core was not recreated"; exit 1; }
wait_for "$BASE/health" 300
CORE="$(core_container)"
wait_core http://runtime:8770/health 900
wait_core http://supervisor:8766/health 180
for _ in $(seq 1 60); do
  if api GET /workers 2>/dev/null | grep -q 'compose-worker'; then
    break
  fi
  sleep 1
done
api GET /workers | grep -q 'compose-worker' || { log "Worker did not re-register"; exit 1; }
PROJECTS="$(api GET /projects)"
[[ "$PROJECTS" == *"$PROJECT_ID"* ]] || { log "project was not preserved"; exit 1; }
GATE_AFTER="$(api GET "/quality/candidates/$CANDIDATE_ID/gate")"
[[ "$GATE_AFTER" == *'"outcome":"ACCEPTED"'* ]] || { log "gate was not preserved"; exit 1; }

SUMMARY="$(python3 -c '
import json,sys
print(json.dumps({
  "status":"PASS",
  "candidate_id":sys.argv[1],
  "attempt_id":sys.argv[2],
  "gate":"ACCEPTED",
  "restart_persistence":True,
  "paid_inference":False
}))' "$CANDIDATE_ID" "$ATTEMPT_ID")"
log "$SUMMARY"
