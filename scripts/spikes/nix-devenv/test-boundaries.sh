#!/usr/bin/env bash
# Boundary tests for the spike wrapper. No Podman daemon is touched: a fake
# `podman` function simulates the volume probe/removal exit codes.
set -euo pipefail

cd "$(dirname "$0")"
# shellcheck source=scripts/spikes/nix-devenv/lib.sh
. ./lib.sh

failures=0
pass() { printf 'PASS %s\n' "$*"; }
fail() { printf 'FAIL %s\n' "$*" >&2; failures=$((failures + 1)); }

FAKE_ROOTLESS=true
FAKE_EXISTS=1
FAKE_RM=0
FAKE_RM_CALLED=0
podman() {
  case "$1 $2" in
    "info --format") printf '%s\n' "$FAKE_ROOTLESS" ;;
    "volume exists") return "$FAKE_EXISTS" ;;
    "volume rm")
      FAKE_RM_CALLED=1
      return "$FAKE_RM"
      ;;
    *) return 0 ;;
  esac
}

for name in wp0-spike-nix cache_1 a.b-c; do
  if validate_volume_name TEST "$name"; then
    pass "valid volume name: $name"
  else
    fail "valid volume name rejected: $name"
  fi
done

for name in /tmp/foo ../foo bad:name foo/bar .hidden ""; do
  if (validate_volume_name TEST "$name") >/dev/null 2>&1; then
    fail "invalid volume name accepted: '$name'"
  else
    pass "invalid volume name rejected: '$name'"
  fi
done

FAKE_ROOTLESS=true
if require_rootless_podman; then
  pass "rootless Podman accepted"
else
  fail "rootless Podman rejected"
fi

FAKE_ROOTLESS=false
if (require_rootless_podman) >/dev/null 2>&1; then
  fail "non-rootless Podman accepted"
else
  pass "non-rootless Podman rejected"
fi
FAKE_ROOTLESS=true

FAKE_EXISTS=1
FAKE_RM=0
FAKE_RM_CALLED=0
if remove_volume_if_present absent-volume && [ "$FAKE_RM_CALLED" = "0" ]; then
  pass "absent volume is ignored (no removal attempted)"
else
  fail "absent volume handling"
fi

FAKE_EXISTS=0
FAKE_RM=0
FAKE_RM_CALLED=0
if remove_volume_if_present present-volume && [ "$FAKE_RM_CALLED" = "1" ]; then
  pass "present volume is removed"
else
  fail "present volume removal"
fi

FAKE_EXISTS=0
FAKE_RM=1
if (remove_volume_if_present present-volume) >/dev/null 2>&1; then
  fail "removal failure was ignored"
else
  pass "removal failure is fatal"
fi

FAKE_EXISTS=125
FAKE_RM=0
if (remove_volume_if_present probe-error) >/dev/null 2>&1; then
  fail "volume probe error was ignored"
else
  pass "volume probe error is fatal"
fi

if [ "$failures" -ne 0 ]; then
  printf '%d boundary test(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'all boundary tests passed\n'
