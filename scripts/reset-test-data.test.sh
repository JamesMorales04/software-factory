#!/usr/bin/env bash
# Shell assertions for scripts/reset-test-data.sh.
#
# The test builds a real migrated SQLite database, seeds preserved configuration
# and transactional rows (including immutable-delete triggers), and exercises
# guards, dry-run, backup checksums, reset, restore and integrity checks. It
# never touches /data, a Compose volume, a live Core process or a paid provider.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$ROOT/scripts/reset-test-data.sh"
HELPER="$ROOT/scripts/reset-test-data.py"
SEED="$ROOT/scripts/fixtures/reset-test-data-seed.sql"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-reset-test-data.XXXXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'reset-test-data test: FAIL: %s\n' "$*" >&2
  exit 1
}

sql() {
  sqlite3 -batch -noheader "$1" "$2"
}

build_database() {
  local database="$1" migration
  for migration in "$ROOT"/migrations/*.sql; do
    sqlite3 -batch "$database" <"$migration" ||
      fail "migration failed: $(basename "$migration")"
  done
  sqlite3 -batch "$database" <"$SEED" || fail "seed failed"
  [[ "$(sql "$database" 'PRAGMA foreign_key_check;')" == "" ]] ||
    fail "seeded database has foreign key violations"
}

assert_count() {
  local database="$1" table="$2" expected="$3" actual
  actual="$(sql "$database" "SELECT COUNT(*) FROM \"$table\";")"
  [[ "$actual" == "$expected" ]] ||
    fail "$table count: expected $expected, got $actual"
}

assert_refused() {
  local expected="$1"
  shift
  local status
  set +e
  "$@" </dev/null >"$TMP/refusal.log" 2>&1
  status=$?
  set -e
  [[ "$status" -eq 2 ]] ||
    fail "expected refusal exit 2, got $status for: $*"
  grep -Fq -- "$expected" "$TMP/refusal.log" ||
    fail "refusal did not contain '$expected': $(cat "$TMP/refusal.log")"
}

assert_checksums() {
  (cd "$1" && sha256sum -c SHA256SUMS >/dev/null) ||
    fail "backup checksums do not verify in $1"
}

MASTER="$TMP/master.sqlite"
build_database "$MASTER"

# All current transactional tables are empty after the reset and every
# preserved table keeps its exact pre-reset count.
assert_cutover() {
  local database="$1"
  if ! python3 - "$HELPER" "$MASTER" "$database" <<'PY'
import importlib.util
import sqlite3
import sys

helper_path, master_path, database_path = sys.argv[1:]
spec = importlib.util.spec_from_file_location("reset_test_data", helper_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
master = sqlite3.connect(master_path)
reset = sqlite3.connect(database_path)
for table in sorted(module.TRANSACTIONAL_TABLES):
    if reset.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0] != 0:
        raise SystemExit(f"transactional table is not empty: {table}")
for table in sorted(module.PRESERVED_TABLES - {"_sqlx_migrations"}):
    before = master.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0]
    after = reset.execute(f'SELECT COUNT(*) FROM "{table}"').fetchone()[0]
    if before != after:
        raise SystemExit(f"preserved table changed: {table}: {before} -> {after}")
PY
  then
    fail "reset did not produce the expected table cutover"
  fi
}

assert_helper_failure() {
  local expected="$1"
  shift
  local status
  set +e
  "$@" </dev/null >"$TMP/helper-failure.log" 2>&1
  status=$?
  set -e
  [[ "$status" -ne 0 ]] || fail "expected helper failure, got success: $*"
  grep -Fq -- "$expected" "$TMP/helper-failure.log" ||
    fail "helper failure did not contain '$expected': $(cat "$TMP/helper-failure.log")"
}

assert_trigger_parity() {
  local before after
  before="$(sql "$MASTER" "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger';")"
  after="$(sql "$1" "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger';")"
  [[ "$before" == "$after" ]] ||
    fail "trigger count changed: $before -> $after"
}

# --- guards -----------------------------------------------------------------
mkdir -p "$TMP/evidence"
set +e
env -u FACTORY_TEST_DATA_RESET "$SCRIPT" \
  --project wp6-test --database "$MASTER" --evidence-dir "$TMP/evidence" \
  --dry-run >"$TMP/no-env.log" 2>&1
status=$?
set -e
[[ "$status" -eq 2 ]] || fail "missing FACTORY_TEST_DATA_RESET was not refused"
grep -Fq 'FACTORY_TEST_DATA_RESET=1' "$TMP/no-env.log" ||
  fail "missing-env refusal did not explain the switch"

assert_refused "not a disposable test/dev project" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project software-factory --database "$MASTER" --evidence-dir "$TMP/evidence" --dry-run
assert_refused "explicit test/dev/e2e/uat/wp marker" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project quality-wave12 --database "$MASTER" --evidence-dir "$TMP/evidence" --dry-run
assert_refused "may not point at the live /data volume" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database /data/factory.sqlite \
  --evidence-dir "$TMP/evidence" --dry-run

mkdir -p "$TMP/nonempty"
printf 'do not overwrite\n' >"$TMP/nonempty/old-evidence"
assert_refused "evidence directory must be empty" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$MASTER" --evidence-dir "$TMP/nonempty" --dry-run

assert_refused "non-interactive runs require --yes" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$MASTER" --evidence-dir "$TMP/evidence"
assert_refused "--yes requires --confirm-project" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$MASTER" --evidence-dir "$TMP/evidence" \
  --yes --confirm-project another-project

# --- dry-run ----------------------------------------------------------------
DRY="$TMP/dry.sqlite"
cp "$MASTER" "$DRY"
DRY_EVIDENCE="$TMP/dry-evidence"
FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$DRY" --evidence-dir "$DRY_EVIDENCE" \
  --dry-run >"$TMP/dry.log"
assert_count "$DRY" campaigns 1
assert_count "$DRY" model_call_reservations 1
assert_count "$DRY" candidates 1
assert_checksums "$DRY_EVIDENCE"
grep -Fq '"database_modified": false' "$DRY_EVIDENCE/reset-report.json" ||
  fail "dry-run report claims a database modification"
assert_trigger_parity "$DRY"

# --- reset ------------------------------------------------------------------
RUN="$TMP/run.sqlite"
cp "$MASTER" "$RUN"
RUN_EVIDENCE="$TMP/run-evidence"
FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$RUN" --evidence-dir "$RUN_EVIDENCE" \
  --yes --confirm-project wp6-test >"$TMP/run.log"
assert_cutover "$RUN"
assert_trigger_parity "$RUN"
assert_checksums "$RUN_EVIDENCE"
[[ "$(sql "$RUN" 'PRAGMA integrity_check;')" == "ok" ]] ||
  fail "reset database integrity check failed"
grep -Fq '"result": "RESET"' "$RUN_EVIDENCE/reset-report.json" ||
  fail "reset report did not record RESET"
[[ -f "$RUN_EVIDENCE/factory.sqlite" ]] || fail "backup database is missing"

# --- corrupt backup is refused ---------------------------------------------
CORRUPT="$TMP/corrupt-evidence"
cp -a "$RUN_EVIDENCE" "$CORRUPT"
printf 'x' >>"$CORRUPT/factory.sqlite"
assert_helper_failure "backup checksum mismatch" \
  env FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$TMP/restored-corrupt.sqlite" \
  --evidence-dir "$TMP/restore-corrupt-evidence" \
  --restore-from "$CORRUPT" --yes --confirm-project wp6-test

# --- restore ----------------------------------------------------------------
RESTORED="$TMP/restored.sqlite"
cp "$RUN" "$RESTORED"
RESTORE_EVIDENCE="$TMP/restore-evidence"
FACTORY_TEST_DATA_RESET=1 "$SCRIPT" \
  --project wp6-test --database "$RESTORED" --evidence-dir "$RESTORE_EVIDENCE" \
  --restore-from "$RUN_EVIDENCE" --yes --confirm-project wp6-test \
  >"$TMP/restore.log"
assert_count "$RESTORED" campaigns 1
assert_count "$RESTORED" model_call_reservations 1
assert_count "$RESTORED" projects 1
assert_count "$RESTORED" execution_profiles 1
assert_count "$RESTORED" provider_settings 1
assert_count "$RESTORED" github_connections 1
assert_count "$RESTORED" agentic_owner_charters 1
[[ "$(sql "$RESTORED" 'PRAGMA integrity_check;')" == "ok" ]] ||
  fail "restored database integrity check failed"
assert_checksums "$RESTORE_EVIDENCE"
[[ -f "$RESTORE_EVIDENCE/factory.sqlite" ]] ||
  fail "restore did not back up the pre-restore database"

printf 'reset-test-data shell assertions: PASS\n'
