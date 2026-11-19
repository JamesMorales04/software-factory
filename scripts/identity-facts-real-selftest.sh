#!/usr/bin/env bash
# Real identity-facts self-test.
#
# Compiles the opt-in integration test in the pinned container-native dev image,
# then runs it on the host with ordinary rootless Podman and the existing pinned
# environment-builder image. The test seeds a disposable store volume from that
# image, runs the real broker producer with its fixed Nix argv, verifies the
# signed receipt through the Core client and consumes it through SQLite,
# refusing a replay.
#
# It never starts the product stack, a paid model or a host Nix/Rust install.
# All Podman resources it creates are named `factory-idf-selftest-*` and are
# removed on exit unless KEEP=1.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEV_IMAGE=${FACTORY_IDF_DEV_IMAGE:-localhost/software-factory-dev:local}
BUILDER_IMAGE=${FACTORY_IDENTITY_FACTS_REAL_IMAGE:-localhost/software-factory-environment-builder:dev}
TARGET_VOLUME=${FACTORY_IDF_TARGET_VOLUME:-factory-idf-selftest-target}
PODMAN=${FACTORY_PODMAN_BIN:-$(command -v podman)}
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
WORK=${FACTORY_IDF_WORK:-/tmp/opencode/identity-facts-selftest-$STAMP}
KEEP=${KEEP:-0}

mkdir -p "$WORK"
chmod 700 "$WORK"
chmod 700 "$(dirname "$WORK")" 2>/dev/null || true

cleanup() {
  if [ "$KEEP" = "1" ]; then
    echo "KEEP=1; leaving $WORK and $TARGET_VOLUME" >&2
    return 0
  fi
  "$PODMAN" volume rm --force "$TARGET_VOLUME" >/dev/null 2>&1 || true
  return 0
}
trap cleanup EXIT

echo "work=$WORK"
echo "dev_image=$DEV_IMAGE"
echo "builder_image=$BUILDER_IMAGE"

"$PODMAN" volume create "$TARGET_VOLUME" >/dev/null 2>&1 || true
"$PODMAN" image inspect "$DEV_IMAGE" --format '{{.Id}} {{.Digest}}' > "$WORK/dev-image.txt"
"$PODMAN" image inspect "$BUILDER_IMAGE" --format '{{.Id}} {{.Digest}}' > "$WORK/builder-image.txt"

# Compile the real self-test in the container-native development image. The
# binary is executed on the host because that is where rootless Podman lives.
"$PODMAN" run --rm --userns=keep-id \
  -v "$ROOT:/workspaces/software-factory:rw" \
  -v "$TARGET_VOLUME:/target" \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_HOME=/home/factory/.cargo \
  -w /workspaces/software-factory \
  "$DEV_IMAGE" \
  sh -c 'cargo test -p factory-native --features test-support --test identity_facts_real --no-run --message-format=json' \
  > "$WORK/cargo.json" 2> "$WORK/cargo.stderr"
BIN=$(jq -r 'select(.reason=="compiler-artifact" and (.target.kind|index("test")) and .target.name=="identity_facts_real") | .executable // empty' "$WORK/cargo.json" | tail -1)
if [ -z "$BIN" ] || [ "$BIN" = "null" ]; then
  echo "could not resolve the identity_facts_real test binary" >&2
  exit 1
fi

CONTAINER="factory-idf-selftest-copy-$$"
"$PODMAN" create --name "$CONTAINER" -v "$TARGET_VOLUME:/target:ro" "$DEV_IMAGE" true >/dev/null
"$PODMAN" cp "$CONTAINER:$BIN" "$WORK/identity_facts_real"
"$PODMAN" rm --force "$CONTAINER" >/dev/null
chmod 0700 "$WORK/identity_facts_real"

set +e
FACTORY_IDENTITY_FACTS_REAL_TEST=1 \
FACTORY_IDENTITY_FACTS_REAL_IMAGE="$BUILDER_IMAGE" \
FACTORY_PODMAN_BIN="$PODMAN" \
  "$WORK/identity_facts_real" --ignored --nocapture \
  > "$WORK/selftest.log" 2>&1
SELFTEST_STATUS=$?
set -e
cat "$WORK/selftest.log"
if [ "$SELFTEST_STATUS" -ne 0 ]; then
  echo "selftest failed; work retained at $WORK" >&2
  exit "$SELFTEST_STATUS"
fi
( cd "$WORK" && sha256sum cargo.json selftest.log dev-image.txt builder-image.txt identity_facts_real > SHA256SUMS )
echo "selftest_work=$WORK"

# Confirm no disposable volumes remain.
if "$PODMAN" volume ls --format '{{.Name}}' | grep -q '^factory-idf-selftest-'; then
  echo "ERROR: disposable volumes remain" >&2
  "$PODMAN" volume ls --format '{{.Name}}' | grep '^factory-idf-selftest-' >&2
  exit 1
fi
echo "cleanup=confirmed"
