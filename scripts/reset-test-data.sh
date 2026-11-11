#!/usr/bin/env bash
# Reset transactional Factory test data with an explicit, backed-up lifecycle.
#
# DANGER: this is a development/operator recovery tool. It is never called by
# installation, Compose startup, Core, the Worker or any scheduled task. It
# deletes Campaigns, requests, Actions, Runs, attempts, candidates, Quality
# Gate records, provider sessions, model reservations, leases, events and
# related transactional rows from a disposable test database. Projects,
# execution profiles, provider settings, the GitHub connection, charters,
# policies and owner enrolment are preserved.
#
# It refuses to run against the installed project name, requires
# FACTORY_TEST_DATA_RESET=1, an explicit test/dev project marker, an empty
# evidence directory and interactive confirmation (or --yes plus
# --confirm-project). Raw SQL deletion is not a Factory product feature.
#
# Usage:
#   FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
#     --project software-factory-test --evidence-dir /path/to/empty/evidence
#
#   FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
#     --project software-factory-test --evidence-dir /path/to/empty/evidence \
#     --yes --confirm-project software-factory-test
#
#   # Dry-run a copy; never point --database at the live /data volume.
#   FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
#     --project wp6-test --database /tmp/factory-copy.sqlite \
#     --evidence-dir /tmp/reset-evidence --dry-run
#
#   # Restore a verified backup (the current database is backed up first).
#   FACTORY_TEST_DATA_RESET=1 scripts/reset-test-data.sh \
#     --project software-factory-test --evidence-dir /path/to/pre-restore \
#     --restore-from /path/to/previous/evidence \
#     --yes --confirm-project software-factory-test
#
# Environment:
#   FACTORY_TEST_DATA_RESET=1              mandatory destructive-run switch
#   COMPOSE_PROJECT_NAME                   project when --project is omitted
#   FACTORY_TEST_DATA_HELPER_IMAGE         image containing python3 for the
#                                          Compose volume helper
#                                          (default: software-factory-supervisor:dev)
#   FACTORY_TEST_DATA_CHOWN                restored database owner in the
#                                          helper container (default: 10001:10001)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
HELPER="$ROOT/scripts/reset-test-data.py"
HELPER_IMAGE="${FACTORY_TEST_DATA_HELPER_IMAGE:-software-factory-supervisor:dev}"
CHOWN="${FACTORY_TEST_DATA_CHOWN:-10001:10001}"

PROJECT="${COMPOSE_PROJECT_NAME:-}"
DATABASE=""
EVIDENCE_DIR=""
RESTORE_FROM=""
DRY_RUN=0
ASSUME_YES=0
CONFIRM_PROJECT=""

log() { printf '%s\n' "$*" >&2; }
die() {
  printf 'reset-test-data: refusing: %s\n' "$*" >&2
  exit 2
}

usage() {
  sed -n '2,/^set -euo pipefail$/p' "${BASH_SOURCE[0]}" | sed '$d' | sed 's/^# \{0,1\}//'
}

while (($#)); do
  case "$1" in
    --project)
      [[ $# -ge 2 ]] || die "--project requires a value"
      PROJECT="$2"
      shift 2
      ;;
    --database)
      [[ $# -ge 2 ]] || die "--database requires a value"
      DATABASE="$2"
      shift 2
      ;;
    --evidence-dir)
      [[ $# -ge 2 ]] || die "--evidence-dir requires a value"
      EVIDENCE_DIR="$2"
      shift 2
      ;;
    --restore-from)
      [[ $# -ge 2 ]] || die "--restore-from requires a value"
      RESTORE_FROM="$2"
      shift 2
      ;;
    --confirm-project)
      [[ $# -ge 2 ]] || die "--confirm-project requires a value"
      CONFIRM_PROJECT="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --yes)
      ASSUME_YES=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ "${FACTORY_TEST_DATA_RESET:-}" == "1" ]] ||
  die "set FACTORY_TEST_DATA_RESET=1 to acknowledge the destructive test-data reset"

[[ -n "$PROJECT" ]] || die "--project or COMPOSE_PROJECT_NAME is required"

case "$PROJECT" in
  software-factory|production|prod|default)
    die "project '$PROJECT' is not a disposable test/dev project"
    ;;
esac

if ! [[ "$PROJECT" =~ (^|[-_.])(test|tests|dev|development|e2e|uat|wp[0-9]+)([-_.]|$) ]]; then
  die "project '$PROJECT' must contain an explicit test/dev/e2e/uat/wp marker"
fi

[[ -n "$EVIDENCE_DIR" ]] || die "--evidence-dir is required"
[[ "$EVIDENCE_DIR" != "/" ]] || die "the evidence directory may not be /"
EVIDENCE_DIR="$(realpath -m "$EVIDENCE_DIR")"
[[ "$EVIDENCE_DIR" != "/" ]] || die "the evidence directory may not be /"
[[ "$EVIDENCE_DIR" != "/data" && "$EVIDENCE_DIR" != /data/* ]] ||
  die "the evidence directory may not be inside the live /data volume"

if [[ -e "$EVIDENCE_DIR" ]]; then
  [[ -d "$EVIDENCE_DIR" ]] || die "evidence path exists and is not a directory: $EVIDENCE_DIR"
  if [[ -n "$(find "$EVIDENCE_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    die "evidence directory must be empty: $EVIDENCE_DIR"
  fi
else
  mkdir -p "$EVIDENCE_DIR"
fi
chmod 700 "$EVIDENCE_DIR"

if [[ -n "$RESTORE_FROM" ]]; then
  [[ -d "$RESTORE_FROM" ]] || die "restore backup directory does not exist: $RESTORE_FROM"
  RESTORE_FROM="$(realpath -m "$RESTORE_FROM")"
  [[ "$RESTORE_FROM" != "$EVIDENCE_DIR" ]] ||
    die "--restore-from and --evidence-dir must be different directories"
  [[ -f "$RESTORE_FROM/SHA256SUMS" && -f "$RESTORE_FROM/manifest.json" ]] ||
    die "restore backup is missing SHA256SUMS or manifest.json: $RESTORE_FROM"
fi

if [[ -n "$DATABASE" ]]; then
  DATABASE_REAL="$(realpath -m "$DATABASE")"
  [[ "$DATABASE_REAL" != "/data" && "$DATABASE_REAL" != /data/* ]] ||
    die "--database may not point at the live /data volume"
  if [[ -z "$RESTORE_FROM" ]]; then
    [[ -f "$DATABASE_REAL" ]] || die "database file does not exist: $DATABASE_REAL"
  fi
  DATABASE="$DATABASE_REAL"
else
  [[ "$DRY_RUN" != "1" ]] ||
    die "Compose dry-run is not supported; copy the database and use --database"
fi

if [[ "$DRY_RUN" != "1" ]]; then
  if [[ "$ASSUME_YES" == "1" ]]; then
    [[ "$CONFIRM_PROJECT" == "$PROJECT" ]] ||
      die "--yes requires --confirm-project '$PROJECT'"
  else
    [[ -t 0 ]] ||
      die "non-interactive runs require --yes --confirm-project '$PROJECT'"
    printf 'Type the project name (%s) to confirm the destructive reset: ' "$PROJECT" >&2
    IFS= read -r answer
    [[ "$answer" == "$PROJECT" ]] || die "confirmation did not match '$PROJECT'"
  fi
fi

[[ -f "$HELPER" ]] || die "missing helper: $HELPER"
[[ -x "$HELPER" ]] || chmod +x "$HELPER"

helper_image() {
  if ! podman image exists "$HELPER_IMAGE" >/dev/null 2>&1; then
    die "helper image '$HELPER_IMAGE' is not present; build the Compose stack first or set FACTORY_TEST_DATA_HELPER_IMAGE"
  fi
}

compose() {
  if [[ -z "${PODMAN_COMPOSE_PROVIDER:-}" ]] && command -v podman-compose >/dev/null 2>&1; then
    PODMAN_COMPOSE_PROVIDER="$(command -v podman-compose)"
    export PODMAN_COMPOSE_PROVIDER
  fi
  podman compose -p "$PROJECT" "$@"
}

project_running() {
  podman ps --filter "label=com.docker.compose.project=$PROJECT" --format '{{.Names}}' 2>/dev/null |
    grep -q .
}

find_volume() {
  local matches core
  matches="$(podman volume ls \
    --filter "label=com.docker.compose.project=$PROJECT" \
    --filter "label=com.docker.compose.volume=factory-data" \
    --format '{{.Name}}' 2>/dev/null | sed '/^$/d')"
  if [[ -z "$matches" ]]; then
    matches="$(podman volume ls \
      --filter "name=${PROJECT}_factory-data" \
      --format '{{.Name}}' 2>/dev/null | sed '/^$/d')"
  fi
  if [[ -z "$matches" ]]; then
    core="$(podman ps -a \
      --filter "label=com.docker.compose.project=$PROJECT" \
      --filter "label=com.docker.compose.service=core" \
      --format '{{.Names}}' 2>/dev/null | head -n1)"
    if [[ -n "$core" ]]; then
      matches="$(podman inspect "$core" --format \
        '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Name}}{{end}}{{end}}' 2>/dev/null)"
    fi
  fi
  [[ -n "$matches" ]] || die "cannot find the '$PROJECT' factory-data volume"
  if [[ "$(wc -l <<<"$matches")" -ne 1 ]]; then
    die "multiple factory-data volumes matched project '$PROJECT': $matches"
  fi
  printf '%s' "$matches"
}

run_helper() {
  local volume="$1"
  shift
  local args=(
    --database /data/factory.sqlite
    --evidence-dir /evidence
    --project "$PROJECT"
    --chown "$CHOWN"
  )
  local mounts=(
    -v "$volume:/data:rw"
    -v "$EVIDENCE_DIR:/evidence:rw"
    -v "$HELPER:/usr/local/bin/reset-test-data.py:ro"
  )
  if [[ -n "$RESTORE_FROM" ]]; then
    mounts+=(-v "$RESTORE_FROM:/backup:ro")
    args+=(--restore-from /backup)
  fi
  if [[ "$DRY_RUN" == "1" ]]; then
    args+=(--dry-run)
  fi
  podman run --rm --network=none --user 0:0 \
    --security-opt no-new-privileges \
    "${mounts[@]}" \
    --entrypoint python3 "$HELPER_IMAGE" \
    /usr/local/bin/reset-test-data.py "${args[@]}" "$@"
}

wait_for_core_health() {
  local core attempt payload
  for attempt in $(seq 1 180); do
    core="$(podman ps \
      --filter "label=com.docker.compose.project=$PROJECT" \
      --filter "label=com.docker.compose.service=core" \
      --format '{{.Names}}' 2>/dev/null | head -n1)"
    if [[ -n "$core" ]] && payload="$(podman exec "$core" curl -fsS \
      --connect-timeout 3 --max-time 5 \
      -H 'Host: localhost:8765' http://core-control:8765/health 2>/dev/null)"; then
      printf 'Core health: %s\n' "$payload"
      return 0
    fi
    sleep 1
  done
  return 1
}

if [[ -n "$DATABASE" ]]; then
  command -v python3 >/dev/null 2>&1 ||
    die "python3 is required for --database (test-copy) mode"
  args=(
    --database "$DATABASE"
    --evidence-dir "$EVIDENCE_DIR"
    --project "$PROJECT"
  )
  if [[ -n "$RESTORE_FROM" ]]; then
    args+=(--restore-from "$RESTORE_FROM")
  fi
  if [[ "$DRY_RUN" == "1" ]]; then
    args+=(--dry-run)
  fi
  python3 "$HELPER" "${args[@]}"
  exit $?
fi

helper_image
VOLUME="$(find_volume)"
log "project: $PROJECT"
log "volume: $VOLUME"
log "evidence: $EVIDENCE_DIR"
if [[ -n "$RESTORE_FROM" ]]; then
  log "restore: $RESTORE_FROM"
fi

if project_running; then
  log "stopping every service in project '$PROJECT' before the database copy"
  compose stop
fi
if project_running; then
  die "project '$PROJECT' still has running containers; stop them and retry"
fi

status=0
run_helper "$VOLUME" || status=$?

log "restarting init and Core, then the configured project services"
compose up -d init core
if ! compose up -d; then
  log "warning: not every project service restarted; Core health is still checked"
fi
if ! wait_for_core_health; then
  log "Core did not become healthy after the reset; backup is in $EVIDENCE_DIR"
  exit 1
fi
if ((status != 0)); then
  log "database helper failed with status $status; the pre-reset backup is in $EVIDENCE_DIR"
  exit "$status"
fi
log "reset complete; evidence and checksums: $EVIDENCE_DIR"
