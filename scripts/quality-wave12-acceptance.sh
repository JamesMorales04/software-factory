#!/usr/bin/env bash
# WP12 deterministic quality-wave acceptance.
#
# Runs the registered Rust, Office and Compose checks for the Quality-first
# Wave 1/2 slices. It is an implementer harness, not independent acceptance:
# it never starts the installed stack, never calls a paid model provider and
# never treats a skipped live/opt-in test as a pass.
#
# Usage:
#   scripts/quality-wave12-acceptance.sh [--repeat N] [--skip-office]
#                                        [--skip-compose] [--no-install]
#                                        [--require-clean] [--self-test]
#
# A full run (no --skip-* option) prints `RESULT: PASS` and exits 0. Any
# required skipped step prints `RESULT: PARTIAL` and exits 3; it is not
# acceptance. A failed step exits 1 and missing tooling exits 2.
# `--skip-office`/`--skip-compose` are diagnostic only. `--no-install` fails
# closed when Office dependencies are missing or stale. `--self-test` runs the
# shell assertions for these semantics and exits without running acceptance.
#
# Environment:
#   FACTORY_WAVE12_TARGET_DIR  Cargo target directory (default: <repo>/target)
#   FACTORY_WAVE12_LOG_DIR     Per-step logs (default: <target>/quality-wave12)
#   FACTORY_WAVE12_TMP_DIR     Test temp root outside the repo (default: /var/tmp/factory-wave12)
#   FACTORY_WAVE12_REPEAT      Default repeat count for provider_chaos (3)
#   FACTORY_WAVE12_OFFICE_DIR  Office directory for the dependency assertions
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

REPEAT="${FACTORY_WAVE12_REPEAT:-3}"
SKIP_OFFICE=0
SKIP_COMPOSE=0
NO_INSTALL=0
REQUIRE_CLEAN=0
SELF_TEST=0
while (($#)); do
  case "$1" in
    --repeat)
      [[ $# -ge 2 ]] || { printf 'error: --repeat requires a value\n' >&2; exit 2; }
      REPEAT="$2"
      shift 2
      ;;
    --skip-office)
      SKIP_OFFICE=1
      shift
      ;;
    --skip-compose)
      SKIP_COMPOSE=1
      shift
      ;;
    --no-install)
      NO_INSTALL=1
      shift
      ;;
    --require-clean)
      REQUIRE_CLEAN=1
      shift
      ;;
    --self-test)
      SELF_TEST=1
      shift
      ;;
    -h|--help)
      sed -n '1,45p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *)
      printf 'error: unknown argument %s\n' "$1" >&2
      exit 2
      ;;
  esac
done
if ! [[ "$REPEAT" =~ ^[1-9][0-9]*$ ]]; then
  printf 'error: --repeat must be a positive integer\n' >&2
  exit 2
fi

TARGET_DIR="${FACTORY_WAVE12_TARGET_DIR:-$ROOT/target}"
LOG_DIR="${FACTORY_WAVE12_LOG_DIR:-$TARGET_DIR/quality-wave12}"
# Keep temporary test trees outside the repository. Several tests assert that
# std::env::temp_dir() is not a Git worktree, and /tmp may be a small tmpfs.
TMP_DIR="${FACTORY_WAVE12_TMP_DIR:-/var/tmp/factory-wave12}"
OFFICE_DIR="${FACTORY_WAVE12_OFFICE_DIR:-$ROOT/office}"
mkdir -p "$TARGET_DIR" "$LOG_DIR" "$TMP_DIR"
export CARGO_TARGET_DIR="$TARGET_DIR"
export TMPDIR="$TMP_DIR"
export CARGO_TERM_COLOR=never
export NO_COLOR=1
export npm_config_color=false

PASS=()
FAIL=()
SKIP=()
FAILED=0

log() { printf '%s\n' "$*" >&2; }

require() {
  local command="$1"
  command -v "$command" >/dev/null 2>&1 || {
    log "BLOCKED: required command not found: $command"
    exit 2
  }
}

preflight() {
  require cargo
  require rustc
  require python3
  require podman
  require sha256sum
  if ((SKIP_OFFICE == 0)); then
    require node
    require npm
  fi
  if ((SKIP_COMPOSE == 0)); then
    if ! command -v docker-compose >/dev/null 2>&1 \
      && ! (command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1); then
      log "BLOCKED: compose policy needs docker-compose (JSON config) or docker compose"
      exit 2
    fi
  fi
}

run_step() {
  local name="$1"
  shift
  local logfile="$LOG_DIR/${name}.log"
  local started=$SECONDS
  printf '\n== %s ==\n$ %s\n' "$name" "$*"
  if "$@" 2>&1 | tee "$logfile"; then
    PASS+=("$name")
    printf 'RESULT %-32s PASS (%ss)\n' "$name" "$((SECONDS - started))"
  else
    local status=$?
    FAIL+=("$name")
    FAILED=1
    printf 'RESULT %-32s FAIL (%ss, exit %s)\n' "$name" "$((SECONDS - started))" "$status"
  fi
}

skip_step() {
  local name="$1"
  local reason="$2"
  SKIP+=("$name: $reason")
  printf '\n== %s ==\nRESULT %-32s SKIPPED (%s)\n' "$name" "$name" "$reason"
}

final_result() {
  if ((FAILED != 0)); then
    printf 'FAIL'
    return 1
  fi
  if ((${#SKIP[@]} > 0)); then
    printf 'PARTIAL'
    return 3
  fi
  printf 'PASS'
  return 0
}

self_check() {
  local label="$1" actual="$2" expected="$3"
  if [[ "$actual" == "$expected" ]]; then
    printf 'self-test PASS: %s (%s)\n' "$label" "$actual"
    return 0
  fi
  printf 'self-test FAIL: %s expected=%s actual=%s\n' "$label" "$expected" "$actual" >&2
  return 1
}

self_test() {
  local failures=0 actual status tmp expected
  PASS=()
  FAIL=()
  SKIP=()
  FAILED=0
  status=0
  actual="$(final_result)" || status=$?
  self_check "full run result" "$actual" "PASS" || failures=$((failures + 1))
  self_check "full run exit" "$status" "0" || failures=$((failures + 1))

  SKIP=("office: disabled by --skip-office")
  status=0
  actual="$(final_result)" || status=$?
  self_check "skipped run result" "$actual" "PARTIAL" || failures=$((failures + 1))
  self_check "skipped run exit" "$status" "3" || failures=$((failures + 1))

  FAIL=("lib")
  SKIP=()
  FAILED=1
  status=0
  actual="$(final_result)" || status=$?
  self_check "failed run result" "$actual" "FAIL" || failures=$((failures + 1))
  self_check "failed run exit" "$status" "1" || failures=$((failures + 1))

  PASS=()
  FAIL=()
  SKIP=()
  FAILED=0
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/factory-wave12-selftest.XXXXXXXX")"
  printf '{"lockfileVersion":3}\n' >"$tmp/package-lock.json"

  # Missing dependencies with --no-install must fail before npm.
  status=0
  if ( OFFICE_DIR="$tmp"; NO_INSTALL=1; step_office_install ) >/dev/null 2>&1; then
    status=0
  else
    status=$?
  fi
  self_check "no-install missing dependencies exit" "$status" "1" || failures=$((failures + 1))

  # Stale dependencies with --no-install must fail before npm.
  mkdir -p "$tmp/node_modules"
  printf 'stale\n' >"$tmp/node_modules/.factory-wave12-lock"
  status=0
  if ( OFFICE_DIR="$tmp"; NO_INSTALL=1; step_office_install ) >/dev/null 2>&1; then
    status=0
  else
    status=$?
  fi
  self_check "no-install stale dependencies exit" "$status" "1" || failures=$((failures + 1))

  # Fresh dependencies must pass without invoking npm. A failing npm shim
  # proves the install path is not entered when --no-install is set.
  mkdir -p "$tmp/bin"
  printf '#!/bin/sh\nexit 99\n' >"$tmp/bin/npm"
  chmod +x "$tmp/bin/npm"
  expected="$(sha256sum "$tmp/package-lock.json" | awk '{print $1}')"
  printf '%s\n' "$expected" >"$tmp/node_modules/.factory-wave12-lock"
  status=0
  if ( PATH="$tmp/bin:$PATH"; OFFICE_DIR="$tmp"; NO_INSTALL=1; step_office_install ) >/dev/null 2>&1; then
    status=0
  else
    status=$?
  fi
  self_check "no-install fresh dependencies exit" "$status" "0" || failures=$((failures + 1))

  rm -rf "$tmp"
  if ((failures != 0)); then
    printf 'self-test FAILED: %s assertion(s)\n' "$failures" >&2
    return 1
  fi
  printf 'self-test PASS: all assertions passed\n'
  return 0
}

step_self_test() { bash "$ROOT/scripts/quality-wave12-acceptance.test.sh"; }

cargo_test() {
  cargo test -p factory-native --locked "$@"
}

compose_json() {
  local input="$1" profile="$2" output="$3"
  local compose_args=()
  if [[ -n "$profile" ]]; then
    compose_args=(--profile "$profile")
  fi
  if command -v docker-compose >/dev/null 2>&1; then
    docker-compose "${compose_args[@]}" -f "$input" config --format json >"$output"
  elif command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    docker compose "${compose_args[@]}" -f "$input" config --format json >"$output"
  else
    return 127
  fi
}

# --- steps -----------------------------------------------------------------

step_fmt() { cargo fmt --all -- --check; }

step_lib() { cargo_test --lib; }

step_bins() { cargo_test --bins; }

step_targeted_integration() {
  cargo_test \
    --test planning_pipeline \
    --test provider_chaos \
    --test dependency_broker_http \
    --test openspec_compose_validation \
    --test migration_skill_refresh
}

step_filter_workflow() { cargo_test --lib workflow; }

step_filter_quality() { cargo_test --lib quality; }

step_filter_trace() { cargo_test --lib trace; }

step_filter_review_roles() { cargo_test --lib review_roles; }

step_filter_gate() { cargo_test --lib gate_; }

step_provider_repeat() {
  local run
  for ((run = 1; run <= REPEAT; run++)); do
    printf '\n-- provider_chaos repeat %s/%s --\n' "$run" "$REPEAT"
    cargo_test --test provider_chaos || return $?
  done
}

step_clippy() {
  cargo clippy -p factory-native --locked --all-targets --all-features -- -D warnings
}

office_dependencies_state() {
  local stamp="$OFFICE_DIR/node_modules/.factory-wave12-lock"
  local expected
  if [[ ! -d "$OFFICE_DIR/node_modules" ]]; then
    printf 'missing\n'
    return 1
  fi
  if [[ ! -f "$OFFICE_DIR/package-lock.json" ]]; then
    printf 'lockfile-missing\n'
    return 1
  fi
  expected="$(sha256sum "$OFFICE_DIR/package-lock.json" | awk '{print $1}')"
  if [[ ! -f "$stamp" ]] || [[ "$(cat "$stamp")" != "$expected" ]]; then
    printf 'stale\n'
    return 1
  fi
  printf 'ready\n'
  return 0
}

step_office_install() {
  local stamp="$OFFICE_DIR/node_modules/.factory-wave12-lock"
  local expected state
  state="$(office_dependencies_state)" || true
  if [[ "$state" == "ready" ]]; then
    printf 'office dependencies already match package-lock.json\n'
    return 0
  fi
  if [[ "$NO_INSTALL" == "1" ]]; then
    printf 'error: --no-install was set but Office dependencies are %s\n' "$state" >&2
    return 1
  fi
  ( cd "$OFFICE_DIR" && npm ci --no-audit --no-fund --prefer-offline ) || return $?
  expected="$(sha256sum "$OFFICE_DIR/package-lock.json" | awk '{print $1}')"
  printf '%s\n' "$expected" >"$stamp"
}

step_office_test() { ( cd "$OFFICE_DIR" && npm test -- --run ); }

step_office_typecheck() { ( cd "$OFFICE_DIR" && npm run typecheck ); }

step_office_lint() { ( cd "$OFFICE_DIR" && npm run lint ); }

step_compose_base() {
  podman compose -f compose.yaml config >"$LOG_DIR/compose-base.yaml"
}

step_compose_profile() {
  podman compose --profile openspec-validation -f compose.yaml config \
    >"$LOG_DIR/compose-openspec.yaml"
}

step_policy_base() {
  compose_json "$LOG_DIR/compose-base.yaml" "" "$LOG_DIR/compose-base.json" || return $?
  python3 .github/scripts/check-compose-policy.py "$LOG_DIR/compose-base.json" || return $?
  COMPOSE_CONFIG_JSON="$LOG_DIR/compose-base.json" \
    python3 .github/scripts/test_check_compose_policy.py || return $?
}

step_policy_profile() {
  compose_json "$LOG_DIR/compose-openspec.yaml" "openspec-validation" \
    "$LOG_DIR/compose-openspec.json" || return $?
  python3 .github/scripts/check-compose-policy.py "$LOG_DIR/compose-openspec.json" || return $?
  COMPOSE_CONFIG_JSON="$LOG_DIR/compose-openspec.json" \
    python3 .github/scripts/test_check_compose_policy.py || return $?
}

# --- run -------------------------------------------------------------------

if ((SELF_TEST == 1)); then
  self_test
  exit $?
fi

preflight
if ((REQUIRE_CLEAN == 1)); then
  if [[ -n "$(git status --porcelain)" ]]; then
    log "BLOCKED: --require-clean was set but the worktree has changes"
    git status --short >&2
    exit 2
  fi
fi
printf 'WP12 acceptance\n'
printf 'repo: %s\n' "$ROOT"
printf 'cargo target: %s\n' "$TARGET_DIR"
printf 'logs: %s\n' "$LOG_DIR"
printf 'repeat: %s\n' "$REPEAT"
printf 'git: %s %s\n' "$(git rev-parse --short=12 HEAD 2>/dev/null || printf unknown)" \
  "$(git branch --show-current 2>/dev/null || printf unknown)"
printf 'tree: %s\n' "$(git rev-parse HEAD^{tree} 2>/dev/null || printf unknown)"
printf 'dirty entries: %s\n' "$(git status --porcelain | wc -l)"
printf 'versions: %s | %s | %s | %s | %s\n' \
  "$(cargo --version)" "$(rustc --version)" \
  "$(node --version 2>/dev/null || printf 'node skipped')" \
  "$(npm --version 2>/dev/null || printf 'npm skipped')" \
  "$(podman --version)"

if [[ "${FACTORY_WAVE12_SKIP_SELF_TEST:-0}" != "1" ]]; then
  run_step self_test step_self_test
fi
run_step fmt step_fmt
run_step lib step_lib
run_step bins step_bins
run_step targeted_integration step_targeted_integration
run_step filter_workflow step_filter_workflow
run_step filter_quality step_filter_quality
run_step filter_trace step_filter_trace
run_step filter_review_roles step_filter_review_roles
run_step filter_gate step_filter_gate
run_step provider_repeat step_provider_repeat
run_step clippy step_clippy

if ((SKIP_OFFICE == 0)); then
  run_step office_install step_office_install
  run_step office_test step_office_test
  run_step office_typecheck step_office_typecheck
  run_step office_lint step_office_lint
else
  skip_step office "disabled by --skip-office"
fi

if ((SKIP_COMPOSE == 0)); then
  run_step compose_config_base step_compose_base
  run_step compose_config_profile step_compose_profile
  run_step compose_policy_base step_policy_base
  run_step compose_policy_profile step_policy_profile
else
  skip_step compose "disabled by --skip-compose"
fi

printf '\n== Summary ==\n'
printf 'PASS: %s\n' "${#PASS[@]}"
for name in "${PASS[@]}"; do printf '  PASS %s\n' "$name"; done
printf 'FAIL: %s\n' "${#FAIL[@]}"
for name in "${FAIL[@]}"; do printf '  FAIL %s\n' "$name"; done
printf 'SKIPPED: %s\n' "${#SKIP[@]}"
for entry in "${SKIP[@]}"; do printf '  SKIP %s\n' "$entry"; done
printf 'logs: %s\n' "$LOG_DIR"

status=0
result="$(final_result)" || status=$?
printf 'RESULT: %s\n' "$result"
if [[ "$result" == "PARTIAL" ]]; then
  printf 'PARTIAL: required steps were skipped; this run is not acceptance.\n' >&2
fi
exit "$status"
