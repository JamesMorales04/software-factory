#!/usr/bin/env bash
# Shell assertions for the WP12 acceptance harness control flow.
#
# This is intentionally lightweight: it does not run the Rust, Office or
# Compose acceptance suite. It proves that required skips are PARTIAL (not
# PASS), that failures remain FAIL, and that --no-install fails closed for
# missing and stale Office dependencies.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="$ROOT/scripts/quality-wave12-acceptance.sh"

help="$("$SCRIPT" --help)"
grep -Fq 'RESULT: PARTIAL' <<<"$help"
grep -Fq -- '--no-install' <<<"$help"
grep -Fq -- '--self-test' <<<"$help"

"$SCRIPT" --self-test

# Exercise the real CLI control flow with harmless command shims. The shims
# return success immediately, so the assertions are about result semantics and
# the Office dependency gate, not about Rust/Office/Compose behavior.
tmp="$(mktemp -d "${TMPDIR:-/tmp}/factory-wave12-cli-test.XXXXXXXX")"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/target" "$tmp/logs" "$tmp/tmp" "$tmp/office"
printf '{"lockfileVersion":3}\n' >"$tmp/office/package-lock.json"
for command in cargo rustc node npm python3 podman; do
  printf '#!/bin/sh\nexit 0\n' >"$tmp/bin/$command"
  chmod +x "$tmp/bin/$command"
done

run_shimmed() {
  local output="$1"
  shift
  set +e
  PATH="$tmp/bin:$PATH" \
    FACTORY_WAVE12_TARGET_DIR="$tmp/target" \
    FACTORY_WAVE12_LOG_DIR="$tmp/logs" \
    FACTORY_WAVE12_TMP_DIR="$tmp/tmp" \
    FACTORY_WAVE12_OFFICE_DIR="$tmp/office" \
    FACTORY_WAVE12_SKIP_SELF_TEST=1 \
    "$SCRIPT" "$@" >"$output" 2>&1
  local status=$?
  set -e
  printf '%s\n' "$status"
}

status="$(run_shimmed "$tmp/partial.log" --skip-office --skip-compose)"
[[ "$status" == "3" ]]
grep -Fq 'RESULT: PARTIAL' "$tmp/partial.log"

status="$(run_shimmed "$tmp/missing.log" --no-install --skip-compose)"
[[ "$status" == "1" ]]
grep -Fq -- '--no-install was set but Office dependencies are missing' "$tmp/missing.log"

mkdir -p "$tmp/office/node_modules"
printf 'stale\n' >"$tmp/office/node_modules/.factory-wave12-lock"
status="$(run_shimmed "$tmp/stale.log" --no-install --skip-compose)"
[[ "$status" == "1" ]]
grep -Fq -- '--no-install was set but Office dependencies are stale' "$tmp/stale.log"

printf 'quality-wave12 acceptance control-flow assertions: PASS\n'
