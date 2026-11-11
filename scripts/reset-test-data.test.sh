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

# The Python helper is independently guarded; direct invocation must not
# bypass the shell wrapper's environment, project, confirmation or /data rules.
assert_helper_failure "FACTORY_TEST_DATA_RESET=1" \
  env -u FACTORY_TEST_DATA_RESET python3 "$HELPER" \
  --database "$MASTER" --evidence-dir "$TMP/direct-no-env" \
  --project wp6-test --confirmed-project wp6-test
assert_helper_failure "not a disposable test/dev project" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$MASTER" --evidence-dir "$TMP/direct-production" \
  --project software-factory --confirmed-project software-factory
assert_helper_failure "explicit test/dev/e2e/uat/wp marker" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$MASTER" --evidence-dir "$TMP/direct-marker" \
  --project quality-wave12 --confirmed-project quality-wave12
assert_helper_failure "confirmed-project" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$MASTER" --evidence-dir "$TMP/direct-confirm-missing" \
  --project wp6-test
assert_helper_failure "must exactly match" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$MASTER" --evidence-dir "$TMP/direct-confirm-wrong" \
  --project wp6-test --confirmed-project another-project
assert_helper_failure "may not point inside the live /data volume" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database /data/factory.sqlite --evidence-dir "$TMP/direct-data" \
  --project wp6-test --confirmed-project wp6-test
assert_helper_failure "evidence directory must be empty" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$MASTER" --evidence-dir "$TMP/nonempty" \
  --project wp6-test --confirmed-project wp6-test

# A symlinked database or sidecar must not become a symlinked backup.
SYMLINK_REAL="$TMP/symlink-real.sqlite"
SYMLINK_DB="$TMP/symlink-db.sqlite"
cp "$MASTER" "$SYMLINK_REAL"
ln -s "$SYMLINK_REAL" "$SYMLINK_DB"
assert_helper_failure "database may not be a symlink" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$SYMLINK_DB" --evidence-dir "$TMP/symlink-db-evidence" \
  --project wp6-test --confirmed-project wp6-test
assert_count "$SYMLINK_REAL" campaigns 1
[[ ! -f "$TMP/symlink-db-evidence/factory.sqlite" ]] ||
  fail "symlinked database produced a backup"

SIDECAR_DB="$TMP/sidecar.sqlite"
SIDECAR_TARGET="$TMP/sidecar-wal-target"
cp "$MASTER" "$SIDECAR_DB"
printf 'not a wal\n' >"$SIDECAR_TARGET"
ln -s "$SIDECAR_TARGET" "$SIDECAR_DB-wal"
assert_helper_failure "database sidecar -wal may not be a symlink" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$SIDECAR_DB" --evidence-dir "$TMP/sidecar-evidence" \
  --project wp6-test --confirmed-project wp6-test
rm "$SIDECAR_DB-wal"
assert_count "$SIDECAR_DB" campaigns 1
[[ ! -f "$TMP/sidecar-evidence/factory.sqlite" ]] ||
  fail "symlinked database sidecar produced a backup"

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

# --- restore path traversal is refused -------------------------------------
for entry in "../escape" "/tmp/absolute-escape" "factory.sqlite-extra"; do
  case "$entry" in
    ../*) label=relative ;;
    /*) label=absolute ;;
    *) label=unknown-sidecar ;;
  esac
  TRAVERSAL="$TMP/traversal-$label"
  cp -a "$RUN_EVIDENCE" "$TRAVERSAL"
  printf '%064d  %s\n' 0 "$entry" >"$TRAVERSAL/SHA256SUMS"
  assert_helper_failure "invalid backup file name" \
    env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
    --database "$TMP/traversal-target-$label.sqlite" \
    --evidence-dir "$TMP/traversal-evidence-$label" \
    --project wp6-test --confirmed-project wp6-test \
    --restore-from "$TRAVERSAL"
  [[ ! -e "$TMP/traversal-target-$label.sqlite" ]] ||
    fail "path traversal created a target database: $entry"
done

# Duplicate checksum entries and symlinked backup files are refused too.
DUPLICATE="$TMP/duplicate-evidence"
cp -a "$RUN_EVIDENCE" "$DUPLICATE"
cat "$RUN_EVIDENCE/SHA256SUMS" >>"$DUPLICATE/SHA256SUMS"
assert_helper_failure "duplicate backup file entry" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$TMP/duplicate-target.sqlite" \
  --evidence-dir "$TMP/duplicate-restore-evidence" \
  --project wp6-test --confirmed-project wp6-test \
  --restore-from "$DUPLICATE"
[[ ! -e "$TMP/duplicate-target.sqlite" ]] ||
  fail "duplicate checksum entry created a target database"

SYMLINK_BACKUP="$TMP/symlink-backup-evidence"
cp -a "$RUN_EVIDENCE" "$SYMLINK_BACKUP"
mv "$SYMLINK_BACKUP/factory.sqlite" "$SYMLINK_BACKUP/factory-real.sqlite"
ln -s factory-real.sqlite "$SYMLINK_BACKUP/factory.sqlite"
assert_helper_failure "backup file may not be a symlink" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$TMP/symlink-backup-target.sqlite" \
  --evidence-dir "$TMP/symlink-backup-restore-evidence" \
  --project wp6-test --confirmed-project wp6-test \
  --restore-from "$SYMLINK_BACKUP"
[[ ! -e "$TMP/symlink-backup-target.sqlite" ]] ||
  fail "symlinked backup entry created a target database"

# A planted restore temp symlink must not overwrite its target.
RESTORE_TMP_TARGET="$TMP/restore-tmp-target.sqlite"
RESTORE_TMP_VICTIM="$TMP/restore-tmp-victim"
printf 'sentinel\n' >"$RESTORE_TMP_VICTIM"
ln -s "$RESTORE_TMP_VICTIM" "$RESTORE_TMP_TARGET.restore-tmp"
assert_helper_failure "restore temporary path may not be a symlink" \
  env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$RESTORE_TMP_TARGET" \
  --evidence-dir "$TMP/restore-tmp-evidence" \
  --project wp6-test --confirmed-project wp6-test \
  --restore-from "$RUN_EVIDENCE"
[[ "$(cat "$RESTORE_TMP_VICTIM")" == "sentinel" ]] ||
  fail "restore temp symlink overwrote its target"
[[ ! -e "$RESTORE_TMP_TARGET" ]] ||
  fail "restore temp symlink created the destination database"

# A stale regular restore temp file is removed safely and restore proceeds.
STALE_TEMP_TARGET="$TMP/stale-temp-target.sqlite"
printf 'stale temp\n' >"$STALE_TEMP_TARGET.restore-tmp"
env FACTORY_TEST_DATA_RESET=1 python3 "$HELPER" \
  --database "$STALE_TEMP_TARGET" \
  --evidence-dir "$TMP/stale-temp-evidence" \
  --project wp6-test --confirmed-project wp6-test \
  --restore-from "$RUN_EVIDENCE" >/dev/null
assert_count "$STALE_TEMP_TARGET" campaigns 1
[[ ! -e "$STALE_TEMP_TARGET.restore-tmp" ]] ||
  fail "stale restore temp file was not cleaned up"

# --- post-commit integrity failure auto-restores ---------------------------
INTEGRITY="$TMP/integrity.sqlite"
cp "$MASTER" "$INTEGRITY"
INTEGRITY_EVIDENCE="$TMP/integrity-evidence"
assert_helper_failure "pre-reset backup restored" \
  env FACTORY_TEST_DATA_RESET=1 FACTORY_TEST_DATA_RESET_FAIL_AFTER_COMMIT=1 \
  python3 "$HELPER" \
  --database "$INTEGRITY" --evidence-dir "$INTEGRITY_EVIDENCE" \
  --project wp6-test --confirmed-project wp6-test
assert_count "$INTEGRITY" campaigns 1
assert_count "$INTEGRITY" projects 1
assert_count "$INTEGRITY" model_call_reservations 1
[[ "$(sql "$INTEGRITY" 'PRAGMA integrity_check;')" == "ok" ]] ||
  fail "auto-restored database integrity check failed"
grep -Fq '"outcome": "RESTORED_PRE_RESET"' \
  "$INTEGRITY_EVIDENCE/recovery-report.json" ||
  fail "integrity failure did not report RESTORED_PRE_RESET"
[[ -f "$INTEGRITY_EVIDENCE/recovery-before-restore/factory.sqlite" ]] ||
  fail "integrity recovery did not back up the failed reset database"
assert_checksums "$INTEGRITY_EVIDENCE"
assert_checksums "$INTEGRITY_EVIDENCE/recovery-before-restore"
[[ ! -f "$INTEGRITY_EVIDENCE/reset-report.json" ]] ||
  fail "failed reset must not write a successful reset report"

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

# --- Core health failure restores the pre-reset backup ----------------------
FAKE_BIN="$TMP/fake-bin"
mkdir -p "$FAKE_BIN"
cat >"$FAKE_BIN/podman" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
command_name=${1:-}
(($#)) && shift
case "$command_name" in
  image)
    exit 0
    ;;
  volume)
    if [[ "${1:-}" == "ls" ]]; then
      printf '%s\n' "$FAKE_VOLUME"
    fi
    exit 0
    ;;
  compose)
    exit 0
    ;;
  ps)
    if [[ "$*" == *"com.docker.compose.service=core"* ]]; then
      printf '%s\n' fake-core
    fi
    exit 0
    ;;
  exec)
    counter="$FAKE_HEALTH_COUNTER"
    count=0
    [[ -f "$counter" ]] && count=$(<"$counter")
    count=$((count + 1))
    printf '%s\n' "$count" >"$counter"
    if ((count <= FAKE_HEALTH_FAILURES)); then
      exit 1
    fi
    printf '%s\n' '{"healthy":true}'
    exit 0
    ;;
  run)
    mounts=()
    helper_args=()
    while (($#)); do
      case "$1" in
        -v)
          mounts+=("$2")
          shift 2
          ;;
        --database|--evidence-dir|--project|--confirmed-project|--restore-from|--chown)
          helper_args+=("$1" "$2")
          shift 2
          ;;
        --user|--security-opt|--entrypoint)
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    evidence_host=""
    backup_host=""
    mapped_mounts=()
    for mount in "${mounts[@]}"; do
      host=${mount%%:*}
      rest=${mount#*:}
      case "$rest" in
        /factory-data:*)
          mapped_mounts+=(-v "$FAKE_VOLUME_DIR:/factory-data:rw")
          ;;
        /evidence:*)
          evidence_host="$host"
          mapped_mounts+=(-v "$host:/evidence:rw")
          ;;
        /backup:*)
          backup_host="$host"
          mapped_mounts+=(-v "$host:/backup:ro")
          ;;
        /usr/local/bin/reset-test-data.py:*)
          mapped_mounts+=(-v "$REAL_HELPER:/usr/local/bin/reset-test-data.py:ro")
          ;;
        *)
          mapped_mounts+=(-v "$mount")
          ;;
      esac
    done
    mapped=()
    index=0
    while ((index < ${#helper_args[@]})); do
      flag=${helper_args[$index]}
      value=${helper_args[$((index + 1))]}
      case "$flag" in
        --database)
          value="$FAKE_VOLUME_DIR/${value#/factory-data/}"
          ;;
        --evidence-dir)
          value="$evidence_host"
          ;;
        --restore-from)
          value="$backup_host"
          ;;
        --chown)
          index=$((index + 2))
          continue
          ;;
      esac
      mapped+=("$flag" "$value")
      index=$((index + 2))
    done
    exec python3 "$REAL_HELPER" "${mapped[@]}"
    ;;
esac
exit 0
SH
chmod +x "$FAKE_BIN/podman"

HEALTH_VOLUME_DIR="$TMP/health-volume"
mkdir -p "$HEALTH_VOLUME_DIR"
cp "$MASTER" "$HEALTH_VOLUME_DIR/factory.sqlite"
HEALTH_EVIDENCE="$TMP/health-evidence"
HEALTH_COUNTER="$TMP/health-counter"
set +e
PATH="$FAKE_BIN:$PATH" \
  FAKE_VOLUME="wp6-health-test_factory-data" \
  FAKE_VOLUME_DIR="$HEALTH_VOLUME_DIR" \
  FAKE_HEALTH_COUNTER="$HEALTH_COUNTER" \
  FAKE_HEALTH_FAILURES=1 \
  REAL_HELPER="$HELPER" \
  FACTORY_TEST_DATA_RESET=1 \
  FACTORY_TEST_DATA_HEALTH_ATTEMPTS=1 \
  FACTORY_TEST_DATA_HEALTH_INTERVAL=0 \
  "$SCRIPT" --project wp6-health-test --evidence-dir "$HEALTH_EVIDENCE" \
  --yes --confirm-project wp6-health-test >"$TMP/health.log" 2>&1
health_status=$?
set -e
[[ "$health_status" -ne 0 ]] || fail "health failure did not fail the reset"
grep -Fq "pre-reset backup restored" "$TMP/health.log" ||
  fail "health failure did not report automatic recovery: $(cat "$TMP/health.log")"
grep -Fq "NOT complete" "$TMP/health.log" ||
  fail "health failure incorrectly reported the reset as complete"
if grep -Fq "reset complete;" "$TMP/health.log"; then
  fail "health failure printed the success line"
fi
assert_count "$HEALTH_VOLUME_DIR/factory.sqlite" campaigns 1
assert_count "$HEALTH_VOLUME_DIR/factory.sqlite" projects 1
assert_count "$HEALTH_VOLUME_DIR/factory.sqlite" model_call_reservations 1
[[ "$(sql "$HEALTH_VOLUME_DIR/factory.sqlite" 'PRAGMA integrity_check;')" == "ok" ]] ||
  fail "health recovery database integrity check failed"
RECOVERY_DIR="$(find "$TMP" -maxdepth 1 -type d -name 'health-evidence-recovery.*' | head -n1)"
[[ -n "$RECOVERY_DIR" ]] || fail "health recovery evidence directory is missing"
[[ -f "$RECOVERY_DIR/factory.sqlite" ]] ||
  fail "health recovery did not back up the failed reset database"
grep -Fq '"result":"ROLLED_BACK_AFTER_HEALTH_FAILURE"' \
  "$HEALTH_EVIDENCE/reset-report.json" ||
  fail "health recovery did not supersede the reset report"
grep -Fq '"outcome":"RESTORED_PRE_RESET"' \
  "$HEALTH_EVIDENCE/recovery-report.json" ||
  fail "health recovery did not write a recovery report"
[[ -f "$HEALTH_EVIDENCE/reset-report.committed.json" ]] ||
  fail "health recovery did not preserve the committed reset report as evidence"
assert_checksums "$HEALTH_EVIDENCE"

printf 'reset-test-data shell assertions: PASS\n'
