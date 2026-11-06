#!/usr/bin/env bash
# Real-product Compose browser UAT.
#
# Starts the same rootless Compose topology used by the product, with the
# deterministic fixture provider and the Compose model fixture enabled only as
# an explicit test override. It then drives real Chromium against the published
# Office URL and the real Core API. No /core-api request is intercepted, no
# paid inference is requested and no GitHub PR is created or merged.
#
# This is developer/UAT tooling, not the product install path.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROJECT="${FACTORY_UAT_PROJECT:-software-factory-uat60}"
PORT="${FACTORY_UAT_PORT:-8898}"
BASE="http://127.0.0.1:${PORT}"
EVIDENCE_DIR="${FACTORY_UAT_EVIDENCE_DIR:-$ROOT/docs/evidence/compose-browser-uat-2026-10-29}"
PROVIDER="${PODMAN_COMPOSE_PROVIDER:-/usr/bin/podman-compose}"
KEEP="${FACTORY_UAT_KEEP:-0}"
STAMP="$(date +%s)"
RUN_ROOT="$(mktemp -d "/tmp/opencode/factory-compose-browser-uat-${STAMP}.XXXXXXXX")"
GIT_DIR="$RUN_ROOT/git"
MODEL_DIR="$RUN_ROOT/model"
CHROMIUM_EXE="${CHROMIUM_EXE:-}"

case "$PROJECT" in
  software-factory)
    printf 'FACTORY_UAT_PROJECT must name a disposable project, not the installed stack\n' >&2
    exit 2
    ;;
esac
if [[ "$PROJECT" != *uat* ]]; then
  printf 'FACTORY_UAT_PROJECT must contain "uat" to keep the run disposable\n' >&2
  exit 2
fi
mkdir -p "$EVIDENCE_DIR" "$MODEL_DIR"

# The model fixture runs as an unprivileged container user and reads only this
# context file. It must never receive Factory or provider credentials.
chmod 755 "$MODEL_DIR"

COMPOSE=(podman compose -p "$PROJECT" -f compose.yaml -f compose.e2e.yaml)
export COMPOSE_PROJECT_NAME="$PROJECT"
export FACTORY_PORT="$PORT"
export FACTORY_E2E_GIT_DIR="$GIT_DIR/server"
export FACTORY_E2E_MODEL_DIR="$MODEL_DIR"
export PODMAN_COMPOSE_PROVIDER="$PROVIDER"
export FACTORY_PROVIDER_KIND=fixture

log() { printf '%s\n' "$*" >&2; }

cleanup() {
  if [[ "$KEEP" == "1" ]]; then
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

find_chromium() {
  if [[ -n "$CHROMIUM_EXE" ]]; then
    printf '%s\n' "$CHROMIUM_EXE"
    return
  fi
  local candidate
  candidate="$(find "$HOME/.cache/ms-playwright" -maxdepth 4 -type f -name chrome 2>/dev/null \
    | sort | tail -1)"
  if [[ -z "$candidate" ]]; then
    printf 'CHROMIUM_EXE is required (no cached Playwright Chromium found)\n' >&2
    return 1
  fi
  printf '%s\n' "$candidate"
}

if [[ "${FACTORY_UAT_SKIP_START:-0}" != "1" ]]; then
  log "== preparing disposable Git fixture"
  mkdir -p "$GIT_DIR/source/src" "$GIT_DIR/server"
  git -C "$GIT_DIR/source" init -q
  git -C "$GIT_DIR/source" config user.name 'Compose Browser UAT'
  git -C "$GIT_DIR/source" config user.email 'compose-browser-uat@example.invalid'
  printf 'seed\n' > "$GIT_DIR/source/src/seed.txt"
  git -C "$GIT_DIR/source" add src/seed.txt
  git -C "$GIT_DIR/source" commit -qm seed
  git clone -q --bare "$GIT_DIR/source" "$GIT_DIR/server/repo.git"
  git -C "$GIT_DIR/server/repo.git" update-server-info
  chmod -R a+rX "$GIT_DIR"

  log "== starting the rootless Compose stack (fixture provider)"
  compose down -v >/dev/null 2>&1 || true
  compose up -d --build

  CORE="$(core_container)"
  if [[ -z "$CORE" ]]; then
    log "Core container was not created"
    podman ps -a \
      --filter "label=com.docker.compose.project=$PROJECT" \
      --format '{{.Names}} {{.Status}}' >&2 || true
    exit 1
  fi
  wait_for "$BASE/health" 300
  wait_for "$BASE/core-api/health" 60
  wait_core http://supervisor:8766/health 180
  wait_core http://runtime:8770/health 900
  for _ in $(seq 1 60); do
    if podman exec "$CORE" curl -fsS http://git-fixture:8080/repo.git/HEAD >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
  wait_core http://model-fixture:8080/health 120
  # The operator token stays in this shell only; it is never printed or
  # written to evidence. The browser UAT enrolls a separate device session.
  OPERATOR_TOKEN="$(podman exec "$CORE" cat /config/operator.token)"
  for _ in $(seq 1 60); do
    if podman exec "$CORE" curl -fsS \
      -H "Authorization: Bearer $OPERATOR_TOKEN" \
      http://127.0.0.1:8765/workers 2>/dev/null | grep -q 'compose-worker'; then
      break
    fi
    sleep 1
  done
  if ! podman exec "$CORE" curl -fsS \
    -H "Authorization: Bearer $OPERATOR_TOKEN" \
    http://127.0.0.1:8765/workers 2>/dev/null | grep -q 'compose-worker'; then
    log "Worker did not register"
    exit 1
  fi
  unset OPERATOR_TOKEN

  log "== issuing one-time owner confirmation"
  BOOTSTRAP_CODE="$(podman exec "$CORE" /usr/local/bin/factory-core issue-bootstrap-code)"
  if [[ "$BOOTSTRAP_CODE" == "OWNER_ALREADY_ENROLLED" || -z "$BOOTSTRAP_CODE" ]]; then
    log "the disposable stack was unexpectedly already enrolled"
    exit 1
  fi
else
  CORE="$(core_container)"
  BOOTSTRAP_CODE="${FACTORY_BOOTSTRAP_CODE:?FACTORY_BOOTSTRAP_CODE is required with FACTORY_UAT_SKIP_START=1}"
fi

CHROMIUM_EXE="$(find_chromium)"
if [[ ! -x "$CHROMIUM_EXE" ]]; then
  log "Chromium executable is not executable: $CHROMIUM_EXE"
  exit 1
fi
if ! command -v node >/dev/null 2>&1; then
  log "node is required to run the browser UAT harness"
  exit 1
fi
if [[ ! -d "$ROOT/office/e2e-browser/node_modules/playwright-core" ]]; then
  log "installing playwright-core for the UAT harness"
  (cd "$ROOT/office/e2e-browser" && npm ci --no-audit --no-fund)
fi

log "== driving real Chromium against $BASE"
FACTORY_OFFICE_URL="$BASE" \
FACTORY_BOOTSTRAP_CODE="$BOOTSTRAP_CODE" \
CHROMIUM_EXE="$CHROMIUM_EXE" \
FACTORY_EVIDENCE_DIR="$EVIDENCE_DIR" \
FACTORY_MODEL_CONTEXT="$MODEL_DIR/context.json" \
FACTORY_GIT_REPOSITORY="http://git-fixture:8080/repo.git" \
FACTORY_UAT_CANDIDATE="$(git -C "$ROOT" rev-parse HEAD)" \
FACTORY_UAT_COMPOSE_PROJECT="$PROJECT" \
FACTORY_UAT_COMPOSE_FILES="compose.yaml compose.e2e.yaml" \
node "$ROOT/scripts/compose-product-browser-uat.mjs"

log "== browser UAT evidence: $EVIDENCE_DIR/compose-browser-uat.json"
