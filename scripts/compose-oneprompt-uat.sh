#!/usr/bin/env bash
# Isolated one-prompt autonomy lane.
#
# Starts the canonical rootless Compose topology in a dedicated project, drives
# one real Office prompt through one UI policy confirmation, waits for a real
# Campaign + deterministic Quality Gate, restarts Core only, and verifies the
# same durable identity with zero new model counters/reservations.
#
# This is developer/UAT harness tooling. It is not the product install path, not
# acceptance, not isolation and never uses a paid provider.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

STAMP="$(date +%s)"
PROJECT="${FACTORY_UAT_PROJECT:-software-factory-oneprompt-${STAMP}}"
PORT="${FACTORY_UAT_PORT:-19091}"
BASE="http://127.0.0.1:${PORT}"
EVIDENCE_DIR="${FACTORY_UAT_EVIDENCE_DIR:-/tmp/opencode/oneprompt-uat-${STAMP}}"
RUN_ROOT="${FACTORY_UAT_RUN_ROOT:-/tmp/opencode/oneprompt-uat-run-${STAMP}}"
GIT_DIR="$RUN_ROOT/git"
MODEL_DIR="$RUN_ROOT/model"
STATE_DIR="$RUN_ROOT/browser-state"
BROWSER_IMAGE="${FACTORY_UAT_BROWSER_IMAGE:-software-factory-dev:local}"
PROVIDER="${PODMAN_COMPOSE_PROVIDER:-/usr/bin/podman-compose}"
KEEP="${FACTORY_UAT_KEEP:-0}"
SKIP_START="${FACTORY_UAT_SKIP_START:-0}"
NEGATIVE="${FACTORY_UAT_NEGATIVE:-}"
RELOAD="${FACTORY_UAT_RELOAD_PREPARATION:-0}"
TIMEOUT="${FACTORY_UAT_TIMEOUT_SECS:-1800}"
BOOTSTRAP_CODE=""
CORE=""
NETWORK=""
PLAYWRIGHT_CORE="${FACTORY_PLAYWRIGHT_CORE:-}"
CHROMIUM_EXE="${CHROMIUM_EXE:-}"

log() { printf '%s\n' "$*" >&2; }
die() { log "ERROR: $*"; exit 2; }

case "$PROJECT" in
  software-factory|software-factory-uat-9a2a6678|factory-acceptance|software-factory-uat60)
    die "FACTORY_UAT_PROJECT must be a new disposable project, not $PROJECT"
    ;;
esac
[[ "$PROJECT" == *oneprompt* ]] || die "FACTORY_UAT_PROJECT must contain 'oneprompt'"
[[ "$PORT" != "18893" && "$PORT" != "8893" ]] || die "never use 18893/8893"
[[ "$PORT" =~ ^[0-9]+$ ]] || die "FACTORY_UAT_PORT must be numeric"
if curl -fsS --max-time 2 "$BASE/health" >/dev/null 2>&1; then
  die "port $PORT already answers /health; choose a free dedicated port"
fi

mkdir -p "$EVIDENCE_DIR" "$RUN_ROOT" "$MODEL_DIR" "$STATE_DIR"
chmod 700 "$EVIDENCE_DIR" "$RUN_ROOT" "$STATE_DIR"
chmod 755 "$MODEL_DIR"
COMPOSE=(podman compose -p "$PROJECT" -f compose.yaml -f compose.e2e.yaml)
export COMPOSE_PROJECT_NAME="$PROJECT"
export FACTORY_PORT="$PORT"
export FACTORY_E2E_GIT_DIR="$GIT_DIR/server"
export FACTORY_E2E_MODEL_DIR="$MODEL_DIR"
export PODMAN_COMPOSE_PROVIDER="$PROVIDER"
export FACTORY_PROVIDER_KIND=fixture

cleanup() {
  local status=$?
  if [[ $status -ne 0 ]]; then
    capture_diagnostics || true
  fi
  if [[ "$KEEP" == "1" ]]; then
    log "kept stack project: $PROJECT"
    log "kept evidence: $EVIDENCE_DIR"
    log "kept run root: $RUN_ROOT"
    return
  fi
  "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
  rm -rf "$RUN_ROOT"
}
trap cleanup EXIT

compose() { "${COMPOSE[@]}" "$@"; }

wait_for() {
  local url="$1" deadline=$((SECONDS + ${2:-180}))
  until curl -fsS --max-time 5 "$url" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      log "timed out waiting for $url"
      return 1
    fi
    sleep 1
  done
}

wait_core() {
  local url="$1" deadline=$((SECONDS + ${2:-180}))
  until podman exec "$CORE" curl -fsS --max-time 5 "$url" >/dev/null 2>&1; do
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

capture_diagnostics() {
  mkdir -p "$EVIDENCE_DIR/logs"
  podman ps -a --filter "label=com.docker.compose.project=$PROJECT" \
    >"$EVIDENCE_DIR/logs/containers.txt" 2>&1 || true
  for container in "$CORE" "${PROJECT}_model-fixture_1" "${PROJECT}_worker_1"; do
    [[ -n "$container" ]] || continue
    podman logs --tail 300 "$container" \
      >"$EVIDENCE_DIR/logs/${container}.log" 2>&1 || true
  done
}

prepare_git_fixture() {
  mkdir -p "$GIT_DIR/source/src" "$GIT_DIR/server"
  if [[ ! -d "$GIT_DIR/source/.git" ]]; then
    git -C "$GIT_DIR/source" init -q
    git -C "$GIT_DIR/source" config user.name 'One-prompt UAT'
    git -C "$GIT_DIR/source" config user.email 'oneprompt-uat@example.invalid'
    printf 'seed\n' >"$GIT_DIR/source/src/seed.txt"
    git -C "$GIT_DIR/source" add src/seed.txt
    git -C "$GIT_DIR/source" commit -qm seed
  fi
  if [[ ! -d "$GIT_DIR/server/repo.git" ]]; then
    git clone -q --bare "$GIT_DIR/source" "$GIT_DIR/server/repo.git"
    git -C "$GIT_DIR/server/repo.git" update-server-info
  fi
  chmod -R a+rX "$GIT_DIR"
}

choose_browser_paths() {
  if [[ -z "$PLAYWRIGHT_CORE" ]]; then
    for candidate in \
      /opt/factory-browser/node_modules/playwright-core \
      /usr/local/lib/node_modules/playwright/node_modules/playwright-core; do
      if podman run --rm --entrypoint test "$BROWSER_IMAGE" -d "$candidate" >/dev/null 2>&1; then
        PLAYWRIGHT_CORE="$candidate"
        break
      fi
    done
  fi
  [[ -n "$PLAYWRIGHT_CORE" ]] || die "playwright-core not found in $BROWSER_IMAGE; set FACTORY_PLAYWRIGHT_CORE"
  if [[ -z "$CHROMIUM_EXE" ]]; then
    CHROMIUM_EXE="$(podman run --rm --entrypoint bash "$BROWSER_IMAGE" -c \
      'for c in /home/factory/.cache/ms-playwright/chromium-*/chrome-linux64/chrome /home/factory/.nix-profile/bin/chromium; do [ -x "$c" ] && { printf "%s\n" "$c"; break; }; done' \
      2>/dev/null | tail -1)"
  fi
  [[ -n "$CHROMIUM_EXE" ]] || die "Chromium not found in $BROWSER_IMAGE; set CHROMIUM_EXE"
}

browser_run() {
  local phase="$1"
  local bootstrap_env=()
  if [[ -n "$BOOTSTRAP_CODE" ]]; then
    bootstrap_env=(-e "FACTORY_BOOTSTRAP_CODE=$BOOTSTRAP_CODE")
  fi
  podman run --rm \
    --userns=keep-id \
    --network "$NETWORK" \
    --security-opt no-new-privileges:true \
    -v "$ROOT:/work:ro" \
    -v "$EVIDENCE_DIR:/evidence:rw" \
    -v "$MODEL_DIR:/model:rw" \
    -v "$STATE_DIR:/state:rw" \
    -e FACTORY_OFFICE_URL="http://core:8765" \
    -e FACTORY_EVIDENCE_DIR=/evidence \
    -e FACTORY_MODEL_CONTEXT=/model/context.json \
    -e FACTORY_MODEL_FIXTURE_URL=http://model-fixture:8080 \
    -e FACTORY_GIT_REPOSITORY=http://git-fixture:8080/repo.git \
    -e FACTORY_UAT_PHASE="$phase" \
    -e FACTORY_UAT_NEGATIVE="$NEGATIVE" \
    -e FACTORY_UAT_RELOAD_PREPARATION="$RELOAD" \
    -e FACTORY_UAT_ALLOW_ENROLLED="${FACTORY_UAT_ALLOW_ENROLLED:-0}" \
    -e FACTORY_UAT_ED_HELPER="${FACTORY_UAT_ED_HELPER:-0}" \
    -e FACTORY_UAT_CANDIDATE="$(git -C "$ROOT" rev-parse HEAD)" \
    -e FACTORY_UAT_COMPOSE_PROJECT="$PROJECT" \
    -e FACTORY_UAT_BROWSER_IMAGE="$BROWSER_IMAGE" \
    -e FACTORY_UAT_TIMEOUT_SECS="$TIMEOUT" \
    -e FACTORY_STORAGE_STATE=/state/oneprompt-storage-state.json \
    -e FACTORY_UAT_SNAPSHOT=/evidence/oneprompt-snapshot.json \
    -e FACTORY_PLAYWRIGHT_CORE="$PLAYWRIGHT_CORE" \
    -e CHROMIUM_EXE="$CHROMIUM_EXE" \
    "${bootstrap_env[@]}" \
    --entrypoint node \
    "$BROWSER_IMAGE" /work/office/e2e-browser/one-prompt-autonomy.mjs
}

grep -q 'FACTORY_ENABLE_AUTONOMOUS_START: "1"' compose.yaml \
  || die "compose.yaml does not enable autonomous start"
podman image exists "$BROWSER_IMAGE" \
  || die "browser image $BROWSER_IMAGE is missing; build it in the Root window, not here"
choose_browser_paths

if [[ "$SKIP_START" != "1" ]]; then
  log "== preparing disposable Git fixture"
  prepare_git_fixture
  log "== starting isolated Compose project $PROJECT on $BASE"
  compose down -v >/dev/null 2>&1 || true
  compose up -d --build
else
  log "== attaching to existing isolated project $PROJECT"
  [[ "$PROJECT" == *oneprompt* ]] || die "skip-start requires a dedicated oneprompt project"
fi

CORE="$(core_container)"
[[ -n "$CORE" ]] || die "Core container was not created"
NETWORK="$(podman inspect "$CORE" \
  --format '{{range $name, $_ := .NetworkSettings.Networks}}{{$name}}{{"\n"}}{{end}}' \
  | grep -m1 'factory-control' || true)"
[[ -n "$NETWORK" ]] || die "Core has no factory-control network"

wait_for "$BASE/health" 300
wait_for "$BASE/core-api/health" 60
wait_core http://supervisor:8766/health 180
wait_core http://runtime:8770/health 1200
wait_core http://model-fixture:8080/health 120
for _ in $(seq 1 60); do
  if podman exec "$CORE" curl -fsS http://git-fixture:8080/repo.git/HEAD >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
OPERATOR_TOKEN="$(podman exec "$CORE" cat /config/operator.token)"
for _ in $(seq 1 60); do
  if curl -fsS -H "Authorization: Bearer $OPERATOR_TOKEN" \
    "$BASE/core-api/workers" 2>/dev/null | grep -q 'compose-worker'; then
    break
  fi
  sleep 1
done
if ! curl -fsS -H "Authorization: Bearer $OPERATOR_TOKEN" \
  "$BASE/core-api/workers" 2>/dev/null | grep -q 'compose-worker'; then
  unset OPERATOR_TOKEN
  die "Worker did not register"
fi
unset OPERATOR_TOKEN

if [[ -z "$BOOTSTRAP_CODE" ]]; then
  BOOTSTRAP_CODE="$(podman exec "$CORE" /usr/local/bin/factory-core issue-bootstrap-code 2>/dev/null || true)"
  if [[ "$BOOTSTRAP_CODE" == "OWNER_ALREADY_ENROLLED" || -z "$BOOTSTRAP_CODE" ]]; then
    if [[ "$SKIP_START" == "1" ]]; then
      BOOTSTRAP_CODE=""
      export FACTORY_UAT_ALLOW_ENROLLED=1
    else
      die "the disposable stack was unexpectedly already enrolled"
    fi
  fi
fi

log "== phase run: one prompt through real UI"
browser_run run

if [[ -z "$NEGATIVE" ]]; then
  log "== restarting Core only; preserving DB, volumes and fixture"
  if ! podman ps --filter "name=${PROJECT}_model-fixture_1" --format '{{.Names}}' | grep -q .; then
    die "model fixture is not running before restart"
  fi
  podman restart "$CORE" >/dev/null
  wait_for "$BASE/health" 300
  wait_for "$BASE/core-api/health" 60
  if ! podman ps --filter "name=${PROJECT}_model-fixture_1" --format '{{.Names}}' | grep -q .; then
    die "model fixture did not survive the Core-only restart"
  fi
  log "== phase verify-restart: same identity, zero new model"
  browser_run verify-restart
else
  log "== negative lane $NEGATIVE complete; no restart"
fi

capture_diagnostics
log "== evidence: $EVIDENCE_DIR"
log "== result files:"
find "$EVIDENCE_DIR" -maxdepth 2 -type f -printf '%p\n' | sort
