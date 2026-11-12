#!/usr/bin/env bash
# Independent, read-only candidate verification.
#
# Exports the exact git object to a disposable directory, removes write
# permission from the candidate, runs the Rust checks with a target directory
# outside the candidate, and strips the host environment except for the
# toolchain/cache paths. This is development infrastructure, not Factory
# acceptance or isolation certification.
#
# Usage: scripts/ci/verify-candidate.sh <sha> [--skip-native] [--office]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SHA="${1:?usage: verify-candidate.sh <sha> [--skip-native] [--office]}"
shift
SKIP_NATIVE=0
OFFICE=0
for argument in "$@"; do
  case "$argument" in
    --skip-native) SKIP_NATIVE=1 ;;
    --office) OFFICE=1 ;;
    *) printf 'unknown argument: %s\n' "$argument" >&2; exit 2 ;;
  esac
done

WORK="$(mktemp -d /var/tmp/factory-verify-candidate.XXXXXXXX)"
TARGET="$(mktemp -d /var/tmp/factory-verify-target.XXXXXXXX)"
SCRATCH="$(mktemp -d /var/tmp/factory-verify-scratch.XXXXXXXX)"
cleanup() {
  chmod -R u+w "$WORK" 2>/dev/null || true
  rm -rf "$WORK" "$TARGET" "$SCRATCH"
}
trap cleanup EXIT

cd "$ROOT"
git cat-file -e "${SHA}^{commit}"
git archive "$SHA" | tar -x -C "$WORK"
mkdir -p "$SCRATCH/home" "$SCRATCH/tmp"
if ((OFFICE == 1)); then
  ln -s "$ROOT/office/node_modules" "$WORK/office/node_modules"
fi
chmod -R a-w "$WORK"

run() {
  env -i \
    PATH="$PATH" \
    HOME="$SCRATCH/home" \
    TMPDIR="$SCRATCH/tmp" \
    RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
    CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
    CARGO_TARGET_DIR="$TARGET" \
    CARGO_TERM_COLOR=never \
    NO_COLOR=1 \
    "$@"
}

cd "$WORK"
printf 'verify candidate %s in %s\n' "$SHA" "$WORK"
run cargo test -p factory-cli --locked
run cargo test -p xtask --locked
if ((SKIP_NATIVE == 0)); then
  run cargo test -p factory-native --lib --locked
fi
if ((OFFICE == 1)); then
  run npm --prefix "$WORK/office" run typecheck
  run npm --prefix "$WORK/office" run lint
  run npm --prefix "$WORK/office" run lint:typed
  run npm --prefix "$WORK/office" test -- --run
fi
printf 'verify candidate PASS\n'
