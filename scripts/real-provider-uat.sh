#!/usr/bin/env bash
# Disposable Compose stack for the bounded real-provider UAT.
#
# This is developer/UAT tooling, not the product install path and not
# independent acceptance. It starts a disposable project whose name contains
# "uat", a local Git fixture and the real OpenCode provider route. It never
# receives the OpenCode Go credential: the Owner enters it directly in Office,
# and Core/Worker use it through the existing credential boundary.
#
# Commands:
#   prepare                         build/start the disposable stack and print
#                                   the one-time owner enrollment instructions
#   preflight                       read-only gate/preflight (requires the
#                                   dry-run env contract)
#   assess                          phase 1: real MODEL assessment
#   execute --plan-hash SHA256      phase 2: approve and execute the exact plan
#   resume                          resume a quota-paused run after verified recovery
#   status                          stack and provider readiness (no inference)
#   down                            remove the disposable stack and volumes
#
# Every live phase is gated by scripts/real-provider-uat.mjs. The current
# funding policy is subscription-only: there is no positive USD authorization,
# no spend cap and no paid-mode fallback. The Owner must provide fresh Console
# evidence for entitlement, Extra Usage OFF and quota before any request.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="${FACTORY_REAL_PROVIDER_PROJECT:-software-factory-rp-uat}"
PORT="${FACTORY_REAL_PROVIDER_PORT:-8899}"
BASE="http://127.0.0.1:${PORT}"
PROVIDER="${PODMAN_COMPOSE_PROVIDER:-/usr/bin/podman-compose}"
RUN_ROOT="${FACTORY_REAL_PROVIDER_RUN_ROOT:-/tmp/opencode/real-provider-uat-${PROJECT}}"
EVIDENCE_DIR="${FACTORY_REAL_PROVIDER_EVIDENCE_DIR:-$RUN_ROOT/evidence}"
SUBSCRIPTION_EVIDENCE="${FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE:-$RUN_ROOT/subscription-evidence.json}"
GIT_DIR="$RUN_ROOT/git"
COMPOSE=(podman compose -p "$PROJECT" -f "$ROOT/compose.yaml" -f "$ROOT/compose.real-provider-uat.yaml")

log() { printf '%s\n' "$*" >&2; }

usage() {
  cat >&2 <<'USAGE'
usage: real-provider-uat.sh <prepare|preflight|assess|execute|resume|status|down>

  prepare                      start the disposable stack and print enrollment instructions
  preflight                    read-only readiness check (FACTORY_REAL_PROVIDER_DRY_RUN=1)
  assess                       create Project/Request and run the real MODEL assessment
  execute --plan-hash SHA256   approve and execute one exact frozen plan
  resume                       resume a PAUSED / SUBSCRIPTION_QUOTA_EXHAUSTED run
  status                       show stack/provider readiness without inference
  down                         remove the disposable Compose project and volumes

Live phases additionally require:
  FACTORY_REAL_PROVIDER_UAT=1
  FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD=1
  FACTORY_REAL_PROVIDER_AUTHORIZATION=<owner authorization id>
  FACTORY_REAL_PROVIDER_ACCOUNTING=GO_SUBSCRIPTION_INCLUDED
  FACTORY_REAL_PROVIDER_EXTRA_USAGE=DISABLED
  FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE=<0600 Owner Console evidence JSON>

No spend cap or positive USD authorization is accepted. Billable / pay-as-you-go
modes are refused with PAID_MODE_DISABLED before any request.
USAGE
}

case "$PROJECT" in
  software-factory)
    log "FACTORY_REAL_PROVIDER_PROJECT must not be the installed stack"
    exit 2
    ;;
esac
if [[ "$PROJECT" != *uat* ]]; then
  log 'FACTORY_REAL_PROVIDER_PROJECT must contain "uat" so the run is disposable'
  exit 2
fi
case "$RUN_ROOT" in
  ""|"/"|"/tmp"|"/home"|"/root")
    log "FACTORY_REAL_PROVIDER_RUN_ROOT must be a dedicated disposable directory"
    exit 2
    ;;
esac
if [[ "$RUN_ROOT" != /* ]]; then
  log "FACTORY_REAL_PROVIDER_RUN_ROOT must be absolute"
  exit 2
fi
if [[ "$RUN_ROOT" != *real-provider-uat* ]]; then
  log "FACTORY_REAL_PROVIDER_RUN_ROOT must contain real-provider-uat so cleanup is scoped"
  exit 2
fi
trap 'rm -f "$RUN_ROOT/operator.token"' EXIT

compose() { "${COMPOSE[@]}" "$@"; }

wait_for() {
  local url="$1" deadline=$((SECONDS + ${2:-300}))
  until curl -fsS "$url" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      log "timed out waiting for $url"
      return 1
    fi
    sleep 1
  done
}

wait_core() {
  local url="$1" deadline=$((SECONDS + ${2:-300}))
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

require_core() {
  CORE="$(core_container)"
  if [[ -z "$CORE" ]]; then
    log "the disposable Core is not running; run prepare first"
    exit 1
  fi
}

token_file() {
  require_core
  local file="$RUN_ROOT/operator.token"
  mkdir -p "$RUN_ROOT"
  (umask 077; podman exec "$CORE" cat /config/operator.token > "$file")
  chmod 600 "$file"
  export FACTORY_OPERATOR_TOKEN_FILE="$file"
  unset FACTORY_OPERATOR_TOKEN
}

create_git_fixture() {
  local source="$GIT_DIR/source" server="$GIT_DIR/server"
  rm -rf "$GIT_DIR"
  mkdir -p "$source/src" "$source/tests" "$server"
  git -C "$source" init -q
  git -C "$source" config user.name 'Real Provider UAT'
  git -C "$source" config user.email 'real-provider-uat@example.invalid'
  cat > "$source/Cargo.toml" <<'TOML'
[package]
name = "factory-real-provider-uat"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
TOML
  cat > "$source/src/lib.rs" <<'RUST'
pub mod pricing;

#[cfg(test)]
mod tests {
    use super::pricing::apply_discount;

    #[test]
    fn zero_percent_is_unchanged() {
        assert_eq!(apply_discount(1_000, 0), 1_000);
    }

    #[test]
    fn ten_percent_discount_is_applied_once() {
        assert_eq!(apply_discount(1_000, 10), 900);
    }
}
RUST
  cat > "$source/src/pricing.rs" <<'RUST'
/// Apply an integer percentage discount to a subtotal in cents.
///
/// The current implementation subtracts the discount twice. The bounded UAT
/// objective asks the Implementer to fix this and add boundary tests.
pub fn apply_discount(subtotal_cents: u64, percent: u64) -> u64 {
    let percent = u64::from(percent.min(100));
    let discount_cents = subtotal_cents.saturating_mul(percent) / 100;
    subtotal_cents
        .saturating_sub(discount_cents)
        .saturating_sub(discount_cents)
}
RUST
  printf 'Keep tests here.\n' > "$source/tests/README.txt"
  git -C "$source" add Cargo.toml src/lib.rs src/pricing.rs tests/README.txt
  git -C "$source" commit -qm 'seed discounted pricing fixture'
  git clone -q --bare "$source" "$server/repo.git"
  git -C "$server/repo.git" update-server-info
  chmod -R a+rX "$GIT_DIR"
  export FACTORY_RP_UAT_GIT_DIR="$GIT_DIR"
}

prepare() {
  command -v podman >/dev/null 2>&1 || { log "podman is required"; exit 1; }
  command -v git >/dev/null 2>&1 || { log "git is required"; exit 1; }
  command -v curl >/dev/null 2>&1 || { log "curl is required"; exit 1; }
  mkdir -p "$RUN_ROOT" "$EVIDENCE_DIR"
  create_git_fixture
  export FACTORY_PORT="$PORT"
  export PODMAN_COMPOSE_PROVIDER="$PROVIDER"
  export FACTORY_RP_UAT_GIT_DIR="$GIT_DIR"

  log "== starting disposable Compose project $PROJECT on $BASE"
  compose down -v >/dev/null 2>&1 || true
  compose up -d --build

  require_core
  wait_for "$BASE/health" 300
  wait_for "$BASE/core-api/health" 60
  wait_core http://supervisor:8766/health 180
  wait_core http://runtime:8770/health 900

  local code
  code="$(podman exec "$CORE" /usr/local/bin/factory-core issue-bootstrap-code)"
  log ""
  log "== disposable stack is ready: $BASE"
  if [[ "$code" == "OWNER_ALREADY_ENROLLED" || -z "$code" ]]; then
    log "The stack is already enrolled. Open $BASE and continue with Settings."
  else
    log "One-time owner confirmation: $code"
    log "Open $BASE, enroll this device with the code above, then:"
  fi
  log "1. Settings -> OpenCode Go: enter the API key directly, enable it and"
  log "   select 'Default route for Core model calls'."
  log "2. Click 'Test connection'. It must show AVAILABLE with deepseek-v4.1-flash."
  log "3. Settings -> Runtime: enable semantic review."
  log "4. Confirm Extra Usage is OFF, entitlement and quota in the Console,"
  log "   then create the 0600 subscription evidence file described in the runbook."
  log ""
  log "Evidence directory: $EVIDENCE_DIR"
  log "Subscription verification file: $SUBSCRIPTION_EVIDENCE"
  log "No paid inference was requested by prepare."
}

run_driver() {
  local driver_command="${1:-}"
  if [[ "$driver_command" != "preflight" ]]; then
    if [[ "${FACTORY_REAL_PROVIDER_UAT:-}" != "1" ]]; then
      log "FACTORY_REAL_PROVIDER_UAT=1 is required for a live phase; no request was made"
      exit 3
    fi
    if [[ "${FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD:-}" != "1" ]]; then
      log "a separate explicit Owner go-ahead is required; set FACTORY_REAL_PROVIDER_OWNER_GO_AHEAD=1; no request was made"
      exit 3
    fi
    if [[ -z "${FACTORY_REAL_PROVIDER_AUTHORIZATION:-}" ]]; then
      log "FACTORY_REAL_PROVIDER_AUTHORIZATION is required; no request was made"
      exit 3
    fi
    if [[ -z "${FACTORY_REAL_PROVIDER_ACCOUNTING:-}" ||
          -z "${FACTORY_REAL_PROVIDER_EXTRA_USAGE:-}" ||
          -z "${SUBSCRIPTION_EVIDENCE:-}" ]]; then
      log "the subscription accounting mode, Extra Usage state and Owner Console evidence file are required; no request was made"
      exit 3
    fi
    if [[ "${FACTORY_REAL_PROVIDER_ACCOUNTING}" != "GO_SUBSCRIPTION_INCLUDED" ]]; then
      log "PAID_MODE_DISABLED: only GO_SUBSCRIPTION_INCLUDED is permitted; no request was made"
      exit 3
    fi
    if [[ "${FACTORY_REAL_PROVIDER_EXTRA_USAGE}" != "DISABLED" ]]; then
      log "EXTRA_USAGE_NOT_DISABLED: Extra Usage must be DISABLED; no request was made"
      exit 3
    fi
    if [[ -n "${FACTORY_REAL_PROVIDER_SPEND_CAP_USD:-}" ]]; then
      log "a spend cap is not accepted in subscription-only mode; no request was made"
      exit 3
    fi
  fi
  command -v node >/dev/null 2>&1 || { log "node is required"; exit 1; }
  require_core
  token_file
  export FACTORY_CORE_API_URL="$BASE/core-api"
  export FACTORY_REAL_PROVIDER_EVIDENCE_DIR="$EVIDENCE_DIR"
  export FACTORY_REAL_PROVIDER_REPOSITORY="${FACTORY_REAL_PROVIDER_REPOSITORY:-http://git-fixture:8080/repo.git}"
  export FACTORY_REAL_PROVIDER_COMPOSE_PROJECT="$PROJECT"
  export FACTORY_REAL_PROVIDER_COMPOSE_FILES="compose.yaml compose.real-provider-uat.yaml"
  export FACTORY_REAL_PROVIDER_SUBSCRIPTION_EVIDENCE_FILE="$SUBSCRIPTION_EVIDENCE"
  if [[ -z "${FACTORY_REAL_PROVIDER_CANDIDATE_SHA:-}" ]]; then
    FACTORY_REAL_PROVIDER_CANDIDATE_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)"
    export FACTORY_REAL_PROVIDER_CANDIDATE_SHA
  fi
  node "$ROOT/scripts/real-provider-uat.mjs" "$@"
}

status() {
  require_core
  token_file
  command -v python3 >/dev/null 2>&1 || { log "python3 is required for status"; exit 1; }
  python3 - "$BASE" "$FACTORY_OPERATOR_TOKEN_FILE" <<'PY'
import json
import sys
import urllib.request

base, token_file = sys.argv[1], sys.argv[2]
with open(token_file, encoding="utf-8") as handle:
    token = handle.read().strip()


def get(path):
    request = urllib.request.Request(base + path)
    request.add_header("authorization", "Bearer " + token)
    with urllib.request.urlopen(request, timeout=15) as response:
        return json.load(response)


print(json.dumps(get("/health"), sort_keys=True))
for provider in get("/core-api/settings/providers").get("providers", []):
    print(json.dumps({
        "provider": provider.get("provider"),
        "configured": provider.get("configured"),
        "enabled": provider.get("enabled"),
        "isDefault": provider.get("isDefault"),
        "model": provider.get("model"),
        "baseUrl": provider.get("baseUrl"),
    }, sort_keys=True))
print(json.dumps(get("/core-api/workers"), sort_keys=True))
PY
}

down() {
  log "removing the disposable stack and volumes; ensure evidence is copied first"
  compose down -v >/dev/null 2>&1 || true
  if [[ "${FACTORY_REAL_PROVIDER_KEEP:-0}" == "1" ]]; then
    log "kept run root: $RUN_ROOT"
  else
    rm -rf "$RUN_ROOT"
  fi
  log "disposable stack removed"
}

command_name="${1:-}"
if [[ -z "$command_name" ]]; then
  usage
  exit 2
fi
shift

case "$command_name" in
  prepare) prepare ;;
  preflight) run_driver preflight "$@" ;;
  assess) run_driver assess "$@" ;;
  execute) run_driver execute "$@" ;;
  resume) run_driver resume "$@" ;;
  status) status ;;
  down) down ;;
  *)
    usage
    exit 2
    ;;
esac
