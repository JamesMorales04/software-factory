#!/usr/bin/env bash
# Bounded test of the real self-test cleanup evaluator.
#
# It invokes scripts/identity-facts-real-selftest.sh in cleanup-check-only mode
# with fixture inventories. Unrelated retained volumes/containers are always
# present to prove they cannot affect the verdict.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SCRIPT="$ROOT/scripts/identity-facts-real-selftest.sh"
WORK=$(mktemp -d /tmp/opencode/identity-facts-cleanup-check.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

cat > "$WORK/volumes-unrelated" <<'EOF'
factory-core-data
factory-e2e-recovery-run7-fc137502_factory-data
factory-env-cache-abcdef
run10-coverage-nix
unrelated-volume
EOF
cat > "$WORK/containers-unrelated" <<'EOF'
factory-core-test
factory-e2e-recovery-run7-fc137502_core_1
software-factory-uat-ui6_init_1
EOF

run_case() {
  name=$1
  expected_rc=$2
  expected_text=$3
  volumes=$4
  containers=$5
  target=$6
  keep=$7
  set +e
  output=$(FACTORY_IDF_CLEANUP_CHECK_ONLY=1 \
    FACTORY_IDF_CLEANUP_VOLUMES="$volumes" \
    FACTORY_IDF_CLEANUP_CONTAINERS="$containers" \
    FACTORY_IDF_CLEANUP_TARGET="$target" \
    FACTORY_IDF_CLEANUP_KEEP="$keep" \
    bash "$SCRIPT" 2>&1)
  status=$?
  set -e
  if [ "$status" -ne "$expected_rc" ]; then
    echo "FAIL $name: exit $status, expected $expected_rc: $output" >&2
    return 1
  fi
  if ! printf '%s' "$output" | grep -q "$expected_text"; then
    echo "FAIL $name: output '$output' lacks '$expected_text'" >&2
    return 1
  fi
  echo "PASS $name (exit $status): $output"
}

TARGET=factory-idf-selftest-target-abc
cat "$WORK/volumes-unrelated" > "$WORK/volumes-none"
cat "$WORK/volumes-unrelated" > "$WORK/volumes-target"
printf '%s\n' "$TARGET" >> "$WORK/volumes-target"
cat "$WORK/volumes-unrelated" > "$WORK/volumes-extra"
printf '%s\n%s\n' "$TARGET" "factory-idf-selftest-other" >> "$WORK/volumes-extra"
cat "$WORK/containers-unrelated" > "$WORK/containers-none"
cat "$WORK/containers-unrelated" > "$WORK/containers-extra"
printf '%s\n' "factory-idf-selftest-container" >> "$WORK/containers-extra"

run_case "no-own-resources" 0 "cleanup=confirmed" \
  "$WORK/volumes-none" "$WORK/containers-none" "$TARGET" 0
run_case "retained-target-keep1" 0 "cleanup=confirmed_with_retained_target=$TARGET" \
  "$WORK/volumes-target" "$WORK/containers-none" "$TARGET" 1
run_case "target-without-keep" 1 "ERROR: disposable selftest volumes remain" \
  "$WORK/volumes-target" "$WORK/containers-none" "$TARGET" 0
run_case "extra-own-volume" 1 "ERROR: disposable selftest volumes remain" \
  "$WORK/volumes-extra" "$WORK/containers-none" "$TARGET" 1
run_case "extra-own-container" 1 "ERROR: unexpected factory-idf resources remain" \
  "$WORK/volumes-target" "$WORK/containers-extra" "$TARGET" 1

echo "cleanup-evaluator=PASS"
