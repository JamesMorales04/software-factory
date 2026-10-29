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
MODEL_DIR="$(mktemp -d /tmp/factory-compose-e2e-model.XXXXXXXX)"
COOKIE_JAR="$(mktemp /tmp/factory-compose-e2e-cookie.XXXXXXXX)"
chmod 600 "$COOKIE_JAR"
# The model fixture runs as an unprivileged user and reads only this context.
chmod 755 "$MODEL_DIR"
# compose.yaml declares `name: software-factory`; COMPOSE_PROJECT_NAME alone is
# not enough to isolate this disposable acceptance run. Force the project name
# on every invocation so test volumes/containers can never collide with the
# owner's installed stack.
COMPOSE=(podman compose -p "$PROJECT" -f compose.yaml -f compose.e2e.yaml)
export COMPOSE_PROJECT_NAME="$PROJECT"
export FACTORY_PORT="$PORT"
export FACTORY_E2E_GIT_DIR="$GIT_DIR/server"
export FACTORY_E2E_MODEL_DIR="$MODEL_DIR"
export PODMAN_COMPOSE_PROVIDER="$PROVIDER"
export FACTORY_PROVIDER_KIND=fixture

cleanup() {
  "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
  rm -rf "$GIT_DIR" "$MODEL_DIR"
  rm -f "$COOKIE_JAR"
}
if [[ "${FACTORY_E2E_KEEP:-0}" == "1" ]]; then
  trap 'rm -rf "$GIT_DIR" "$MODEL_DIR"; rm -f "$COOKIE_JAR"' EXIT
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
  local method="$1" path="$2" body="${3:-}" response status payload csrf
  csrf="$(awk -F '\t' '$6 == "factory_csrf" {print $7}' "$COOKIE_JAR" | tail -1)"
  local args=(curl -sS -X "$method" "$BASE$path" -b "$COOKIE_JAR" -c "$COOKIE_JAR" -H "Origin: $BASE")
  if [[ "$method" != "GET" && "$method" != "HEAD" && "$method" != "OPTIONS" ]]; then
    args+=(-H "x-factory-csrf: $csrf")
  fi
  if [[ -n "$body" ]]; then
    args+=(-H 'content-type: application/json' -d "$body")
  else
    args+=(-H 'content-type: application/json')
  fi
  response=$("${args[@]}" -w $'\n%{http_code}')
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
if [[ -z "$CORE" ]]; then
  log "Core container was not created"
  podman ps -a \
    --filter "label=com.docker.compose.project=$PROJECT" \
    --format '{{.Names}} {{.Status}}' >&2 || true
  podman ps -a --format '{{.Names}} {{.Status}}' | head -20 >&2 || true
  exit 1
fi
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
wait_core http://model-fixture:8080/health 120

log "== first-run enrollment"
CODE="$(podman exec "$CORE" /usr/local/bin/factory-core issue-bootstrap-code)"
[[ "$CODE" != "OWNER_ALREADY_ENROLLED" ]] || { log "fresh stack was already enrolled"; exit 1; }
curl -fsS -X POST "$BASE/auth/bootstrap" \
  -H "Origin: $BASE" -H 'content-type: application/json' \
  -c "$COOKIE_JAR" -b "$COOKIE_JAR" \
  -d "$(python3 -c 'import json,sys; print(json.dumps({"code":sys.argv[1],"deviceName":"Compose E2E"}))' "$CODE")" \
  >/dev/null
unset CODE
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
CONFIG_JSON="$(api PUT "/projects/$PROJECT_ID/config" \
  "{\"expectedRevision\":$CONFIG_REV,\"allowedPaths\":[\"src\"],\"capabilities\":[\"repository-edit\",\"generic-toolchain\"],\"qualityProfile\":\"generic\",\"active\":true}")"
PROJECT_CONFIG_HASH="$(printf '%s' "$CONFIG_JSON" | json 'import json,sys; print(json.load(sys.stdin)["configHash"])')"
PROJECT_BASE_COMMIT="$(printf '%s' "$CONFIG_JSON" | json 'import json,sys; print(json.load(sys.stdin)["baseCommit"])')"

log "== freezing an Action with functional and negative checks"
ACTION_ID="$(api POST /actions \
  "{\"project_id\":\"$PROJECT_ID\",\"objective\":\"compose deterministic candidate\",\"provider\":\"opencode\",\"model\":\"deepseek-v4.1-flash\",\"idempotency_key\":\"e2e-action\",\"allowed_paths\":[\"src\"],\"checks\":[{\"id\":\"functional\",\"kind\":\"functional\",\"description\":\"candidate content present\",\"command\":[\"grep\",\"-q\",\"provider candidate\",\"src/provider.txt\"],\"expectExit\":0,\"timeoutSecs\":30},{\"id\":\"negative\",\"kind\":\"negative\",\"description\":\"absent content rejected\",\"command\":[\"grep\",\"-q\",\"absent\",\"src/provider.txt\"],\"expectExit\":1,\"timeoutSecs\":30}]}" \
  | json 'import json,sys; print(json.load(sys.stdin)["id"])')"

log "== creating a scoped delegation"
EXPIRES="$(python3 -c 'import datetime; print((datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(days=1)).isoformat().replace("+00:00","Z"))')"
api POST /delegations \
  "{\"delegation\":{\"id\":\"e2e-grant\",\"principalId\":\"owner\",\"auditId\":\"e2e\",\"projectId\":\"$PROJECT_ID\",\"revision\":1,\"contentHash\":\"\",\"objectiveClasses\":[\"feature\"],\"allowedPaths\":[\"src\"],\"capabilities\":[\"repository-edit\",\"generic-toolchain\"],\"riskClasses\":[\"R0\",\"R1\"],\"autonomousStart\":false,\"autonomousRiskClasses\":[],\"route\":{\"provider\":\"opencode\",\"model\":\"deepseek-v4.1-flash\",\"mode\":\"build\"},\"maxAttempts\":2,\"maxTimeoutSecs\":300,\"maxRepairCycles\":0,\"maxWallClockSecs\":1800,\"maxParallel\":1,\"maxSessions\":16,\"maxRequests\":4,\"maxCumulativeSessions\":32,\"maxCumulativeAttempts\":32,\"expiresAt\":\"$EXPIRES\"},\"idempotency_key\":\"e2e-delegation\"}" >/dev/null

log "== configuring the fixture implementer connection"
# Campaign authorization validates that the declared implementer route exists as
# an enabled provider connection. The deterministic Worker never calls it.
api PUT /settings/providers/opencode \
  '{"model":"deepseek-v4.1-flash","baseUrl":"https://opencode.ai/inference/go/openai/v1","credential":"compose-fixture-credential-0123456789","enabled":true,"isDefault":false,"maxTurns":4,"maxInputTokens":20000,"maxOutputTokens":8000,"maxCostMicros":500000,"inputMicrosPerMillion":1000,"outputMicrosPerMillion":2000}' >/dev/null

log "== configuring the deterministic model gateway and executable profile"
api PUT /settings/providers/opencode-go \
  '{"model":"deepseek-v4.1-flash","baseUrl":"http://model-fixture:8080/v1","credential":"compose-fixture-credential-0123456789","enabled":true,"isDefault":true,"maxTurns":4,"maxInputTokens":100000,"maxOutputTokens":12000,"maxCostMicros":500000,"inputMicrosPerMillion":1000,"outputMicrosPerMillion":2000}' >/dev/null
api PUT /settings/runtime \
  '{"semanticReviewEnabled":true,"semanticReviewMaxOutputTokens":8192}' >/dev/null
ROLE='{"provider":"opencode-go","model":"deepseek-v4.1-flash","reasoning":"medium"}'
PROFILE_JSON="$(api POST /execution-profiles \
  "{\"name\":\"Compose Intake Balanced\",\"description\":\"Deterministic Compose intake profile\",\"category\":\"BALANCED\",\"roles\":{\"lead\":$ROLE,\"implementer\":$ROLE,\"semanticReviewer\":$ROLE},\"operations\":{\"modelRequestTimeoutSecs\":300,\"toolCallTimeoutSecs\":600,\"maxRequestBytes\":2097152,\"maxResponseBytes\":2097152,\"maxGenerationTokens\":8192,\"maxInFlightOperations\":2},\"progress\":{\"diagnoseAfterStagnantCheckpoints\":2,\"escalateAfterUnsuccessfulReplans\":1,\"observationTimeoutSecs\":300},\"auto_start\":{\"enabled\":false,\"risk_classes\":[]}}")"
PROFILE_ID="$(printf '%s' "$PROFILE_JSON" | json 'import json,sys; print(json.load(sys.stdin)["id"])')"
PROFILE_REVISION="$(printf '%s' "$PROFILE_JSON" | json 'import json,sys; print(json.load(sys.stdin)["revision"])')"
PROFILE_REVISION_ID="$(printf '%s' "$PROFILE_JSON" | json 'import json,sys; print(json.load(sys.stdin)["revisionId"])')"
PROFILE_HASH="$(printf '%s' "$PROFILE_JSON" | json 'import json,sys; print(json.load(sys.stdin)["contentHash"])')"
[[ "$(printf '%s' "$PROFILE_JSON" | json 'import json,sys; print(str(json.load(sys.stdin)["executable"]).lower())')" == "true" ]] || {
  log "execution profile is not executable"; exit 1;
}
api POST "/execution-profiles/$PROFILE_ID/default" '{"scope":"global"}' >/dev/null
api POST "/projects/$PROJECT_ID/execution-policy" \
  "{\"execution_profile_id\":\"$PROFILE_ID\",\"expected_revision\":$PROFILE_REVISION,\"expected_revision_hash\":\"$PROFILE_HASH\",\"project_config_hash\":\"$PROJECT_CONFIG_HASH\",\"acknowledge\":true,\"idempotency_key\":\"e2e-intake-policy\"}" >/dev/null

log "== creating and assessing the unified intake request"
INTAKE_OBJECTIVE='Add the deterministic Compose fixture candidate and cover it with frozen checks.'
INTAKE_CREATED="$(api POST /intake/requests \
  "{\"project_id\":\"$PROJECT_ID\",\"objective\":\"$INTAKE_OBJECTIVE\",\"execution_profile_id\":\"$PROFILE_ID\",\"mode\":\"REVIEW_PLAN\",\"idempotency_key\":\"e2e-intake-request\"}")"
INTAKE_ID="$(printf '%s' "$INTAKE_CREATED" | json 'import json,sys; print(json.load(sys.stdin)["id"])')"
INTAKE_ACTION_ID="$(printf '%s' "$INTAKE_CREATED" | json 'import json,sys; print(json.load(sys.stdin)["actionId"])')"
INTAKE_LEAD_REQUEST_ID="$(printf '%s' "$INTAKE_CREATED" | json 'import json,sys; print(json.load(sys.stdin)["leadRequestId"])')"
cat > "$MODEL_DIR/context.json" <<JSON
{
  "schema": "factory-compose-model-fixture-v1",
  "projectId": "$PROJECT_ID",
  "intakeRequestId": "$INTAKE_ID",
  "requestId": "$INTAKE_LEAD_REQUEST_ID",
  "actionId": "$INTAKE_ACTION_ID",
  "objective": "$INTAKE_OBJECTIVE",
  "baseRevision": "$PROJECT_BASE_COMMIT",
  "projectConfigHash": "$PROJECT_CONFIG_HASH",
  "qualityProfile": "generic",
  "allowedPaths": ["src"],
  "capabilities": ["repository-edit", "generic-toolchain"],
  "provider": "opencode-go",
  "model": "deepseek-v4.1-flash",
  "checks": [
    {"id":"build","kind":"build","description":"deterministic build","command":["true"],"expectExit":0,"timeoutSecs":30},
    {"id":"test","kind":"test","description":"deterministic test","command":["true"],"expectExit":0,"timeoutSecs":30},
    {"id":"functional","kind":"functional","description":"provider candidate content exists","command":["grep","-q","provider candidate","src/provider.txt"],"expectExit":0,"timeoutSecs":30},
    {"id":"negative","kind":"negative","description":"absent content is rejected","command":["grep","-q","absent","src/provider.txt"],"expectExit":1,"timeoutSecs":30}
  ]
}
JSON
chmod 644 "$MODEL_DIR/context.json"

INTAKE_ASSESSED="$(api POST "/intake/requests/$INTAKE_ID/assess")"
[[ "$(printf '%s' "$INTAKE_ASSESSED" | json 'import json,sys; print(json.load(sys.stdin)["state"])')" == "AWAITING_APPROVAL" ]] || {
  log "intake assessment did not reach AWAITING_APPROVAL"; exit 1;
}
PLAN_HASH="$(printf '%s' "$INTAKE_ASSESSED" | json 'import json,sys; print(json.load(sys.stdin)["plan"]["plan_hash"])')"
[[ -n "$PLAN_HASH" ]] || { log "intake assessment produced no frozen plan"; exit 1; }

log "== approving the exact frozen plan through the operator boundary"
INTAKE_APPROVED="$(api POST "/intake/requests/$INTAKE_ID/approve" \
  "{\"expected_plan_hash\":\"$PLAN_HASH\",\"rationale\":\"Reviewed the exact frozen Compose intake plan.\",\"idempotency_key\":\"e2e-intake-approve\",\"authorized_micros\":5000000}")"
INTAKE_CAMPAIGN_ID="$(printf '%s' "$INTAKE_APPROVED" | json 'import json,sys; print(json.load(sys.stdin)["campaignId"])')"
INTAKE_ATTEMPT_ID="$(printf '%s' "$INTAKE_APPROVED" | json 'import json,sys; print(json.load(sys.stdin)["campaign"]["tasks"][0]["attempt_id"])')"
[[ "$(printf '%s' "$INTAKE_APPROVED" | json 'import json,sys; print(json.load(sys.stdin)["campaign"]["authorization"]["execution_mode"])')" == "TRUSTED_LOCAL_PROVIDER" ]] || {
  log "intake Campaign has an unexpected execution mode"; exit 1;
}
[[ "$(printf '%s' "$INTAKE_APPROVED" | json 'import json,sys; print(json.load(sys.stdin)["profile"]["revisionId"])')" == "$PROFILE_REVISION_ID" ]] || {
  log "intake Campaign did not bind the frozen profile revision"; exit 1;
}

log "== waiting for the intake candidate, receipts and deterministic gate"
INTAKE_CANDIDATE_ID=""
INTAKE_GATE=""
# Under loaded CI runners the reviewer can take several minutes; keep the
# bounded wait comfortably above observed completion while still failing closed.
for _ in $(seq 1 600); do
  INTAKE_CANDIDATE_ID="$(api GET /candidates \
    | python3 -c 'import json,sys; target=sys.argv[1]; print(next((c["id"] for c in json.load(sys.stdin) if c["attempt_id"]==target), ""))' "$INTAKE_ATTEMPT_ID")"
  if [[ -n "$INTAKE_CANDIDATE_ID" ]]; then
    INTAKE_GATE="$(api GET "/quality/candidates/$INTAKE_CANDIDATE_ID/gate" 2>/dev/null || true)"
    if [[ "$INTAKE_GATE" == *'"outcome":"ACCEPTED"'* ]]; then
      break
    fi
  fi
  sleep 1
done
if [[ "$INTAKE_GATE" != *'"outcome":"ACCEPTED"'* ]]; then
  log "intake gate did not accept: ${INTAKE_GATE:-none}"
  log "intake request projection: $(api GET "/intake/requests/$INTAKE_ID" 2>/dev/null | head -c 3000 || true)"
  log "intake campaign projection: $(api GET "/campaigns/$INTAKE_CAMPAIGN_ID" 2>/dev/null | head -c 3000 || true)"
  if [[ -n "$INTAKE_CANDIDATE_ID" ]]; then
    log "intake receipts: $(api GET "/quality/candidates/$INTAKE_CANDIDATE_ID/receipts" 2>/dev/null | head -c 3000 || true)"
  fi
  log "== containers"
  podman ps -a --filter "label=com.docker.compose.project=$PROJECT" \
    --format '{{.Names}} {{.Status}}' >&2 || true
  for service in core supervisor worker runtime; do
    name="$(podman ps -a --filter "label=com.docker.compose.project=$PROJECT" \
      --format '{{.Names}}' | grep "_${service}_" | head -1 || true)"
    if [[ -n "$name" ]]; then
      log "== ${service} log tail"
      podman logs --tail 160 "$name" >&2 || true
    fi
  done
  exit 1
fi
INTAKE_RECEIPTS="$(api GET "/quality/candidates/$INTAKE_CANDIDATE_ID/receipts")"
[[ "$INTAKE_RECEIPTS" == *'"role":"verifier"'* && "$INTAKE_RECEIPTS" == *'"role":"reviewer"'* ]] || {
  log "intake independent receipts are missing"; exit 1;
}
[[ "$INTAKE_RECEIPTS" == *'"role":"reviewer"'*'"mechanism":"MODEL"'* || "$INTAKE_RECEIPTS" == *'"mechanism":"MODEL"'*'"role":"reviewer"'* ]] || {
  log "intake reviewer did not record MODEL provenance"; exit 1;
}
INTAKE_REVIEWER_RECEIPT_ID="$(printf '%s' "$INTAKE_RECEIPTS" | python3 -c 'import json,sys; print(next(r["receipt_id"] for r in json.load(sys.stdin) if r["role"]=="reviewer"))')"
INTAKE_VERIFIER_RECEIPT_ID="$(printf '%s' "$INTAKE_RECEIPTS" | python3 -c 'import json,sys; print(next(r["receipt_id"] for r in json.load(sys.stdin) if r["role"]=="verifier"))')"
INTAKE_CANDIDATE_COMMIT="$(api GET /candidates \
  | python3 -c 'import json,sys; target=sys.argv[1]; print(next(c["commit_sha"] for c in json.load(sys.stdin) if c["id"]==target))' "$INTAKE_CANDIDATE_ID")"
INTAKE_DIFF="$(api GET "/campaigns/$INTAKE_CAMPAIGN_ID/candidates/$INTAKE_CANDIDATE_ID/diff")"
[[ "$INTAKE_DIFF" == *'"changed_paths":["src/provider.txt"]'* ]] || {
  log "intake candidate changed-file projection is not the exact fixture path: $INTAKE_DIFF"; exit 1;
}
INTAKE_TASK="$(api GET "/campaigns/$INTAKE_CAMPAIGN_ID" \
  | python3 -c 'import json,sys; t=json.load(sys.stdin)["tasks"][0]; print(t["status"], t["candidate_id"], t["candidate_commit"])')"
[[ "$INTAKE_TASK" == "SUCCEEDED $INTAKE_CANDIDATE_ID $INTAKE_CANDIDATE_COMMIT" ]] || {
  log "intake WorkGraph task lost the exact candidate lineage: $INTAKE_TASK"; exit 1;
}
INTAKE_PROJECTION="$(api GET "/intake/requests/$INTAKE_ID")"
[[ "$INTAKE_PROJECTION" == *'"state":"COMPLETED"'* && "$INTAKE_PROJECTION" == *'"gateOutcome":"ACCEPTED"'* ]] || {
  log "intake request projection is not completed with the accepted gate: $INTAKE_PROJECTION"; exit 1;
}
[[ "$INTAKE_PROJECTION" == *"$INTAKE_CAMPAIGN_ID"* && "$INTAKE_PROJECTION" == *"$PLAN_HASH"* ]] || {
  log "intake request projection lost its Campaign or plan identity"; exit 1;
}

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
GATE_AFTER="$(api GET "/quality/candidates/$INTAKE_CANDIDATE_ID/gate")"
[[ "$GATE_AFTER" == *'"outcome":"ACCEPTED"'* ]] || { log "gate was not preserved"; exit 1; }

INTAKE_PROJECTION_AFTER="$(api GET "/intake/requests/$INTAKE_ID")"
[[ "$INTAKE_PROJECTION_AFTER" == *'"state":"COMPLETED"'* && "$INTAKE_PROJECTION_AFTER" == *'"gateOutcome":"ACCEPTED"'* ]] || {
  log "intake request projection was not preserved"; exit 1;
}
[[ "$INTAKE_PROJECTION_AFTER" == *"$INTAKE_CAMPAIGN_ID"* && "$INTAKE_PROJECTION_AFTER" == *"$INTAKE_CANDIDATE_ID"* ]] || {
  log "intake request lost its Campaign or candidate identity"; exit 1;
}
INTAKE_CAMPAIGN_AFTER="$(api GET "/campaigns/$INTAKE_CAMPAIGN_ID")"
[[ "$INTAKE_CAMPAIGN_AFTER" == *"$INTAKE_CANDIDATE_ID"* && "$INTAKE_CAMPAIGN_AFTER" == *"$INTAKE_CANDIDATE_COMMIT"* ]] || {
  log "intake Campaign candidate identity was not preserved"; exit 1;
}
INTAKE_GATE_AFTER="$(api GET "/quality/candidates/$INTAKE_CANDIDATE_ID/gate")"
[[ "$INTAKE_GATE_AFTER" == *'"outcome":"ACCEPTED"'* ]] || { log "intake gate was not preserved"; exit 1; }
INTAKE_RECEIPTS_AFTER="$(api GET "/quality/candidates/$INTAKE_CANDIDATE_ID/receipts")"
[[ "$INTAKE_RECEIPTS_AFTER" == *"$INTAKE_VERIFIER_RECEIPT_ID"* && "$INTAKE_RECEIPTS_AFTER" == *"$INTAKE_REVIEWER_RECEIPT_ID"* ]] || {
  log "intake receipts were not preserved"; exit 1;
}

SUMMARY="$(python3 -c '
import json,sys
print(json.dumps({
  "status":"PASS",
  "intake_request_id":sys.argv[1],
  "intake_campaign_id":sys.argv[2],
  "intake_attempt_id":sys.argv[3],
  "intake_candidate_id":sys.argv[4],
  "intake_candidate_commit":sys.argv[5],
  "intake_plan_hash":sys.argv[6],
  "intake_profile_revision_id":sys.argv[7],
  "intake_verifier_receipt_id":sys.argv[8],
  "intake_reviewer_receipt_id":sys.argv[9],
  "gate":"ACCEPTED",
  "model_mechanism":"DETERMINISTIC_FIXTURE",
  "restart_persistence":True,
  "paid_inference":False
}))' "$INTAKE_ID" "$INTAKE_CAMPAIGN_ID" "$INTAKE_ATTEMPT_ID" "$INTAKE_CANDIDATE_ID" "$INTAKE_CANDIDATE_COMMIT" "$PLAN_HASH" "$PROFILE_REVISION_ID" "$INTAKE_VERIFIER_RECEIPT_ID" "$INTAKE_REVIEWER_RECEIPT_ID")"
log "$SUMMARY"
