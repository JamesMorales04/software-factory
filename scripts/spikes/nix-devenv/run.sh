#!/usr/bin/env bash
# Host-side wrapper for the Nix/devenv spike.
#
# Only rootless Podman is required on the host. The image contains Nix, Node,
# Cargo and Playwright; `/nix` and `/cache` are Podman named volumes. The
# wrapper rejects host networking and host-path bind mounts, and verifies that
# Podman is rootless before doing any work.
set -euo pipefail

cd "$(dirname "$0")"

die() {
  printf 'run.sh: %s\n' "$*" >&2
  exit 1
}

require_rootless_podman() {
  local rootless
  rootless="$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null)" \
    || die "podman info failed; rootless Podman is required"
  [ "$rootless" = "true" ] \
    || die "rootless Podman is required (podman reports Rootless=$rootless)"
}

validate_volume_name() {
  local variable="$1"
  local value="$2"
  case "$value" in
    "") die "$variable must name a Podman volume" ;;
    */*|.*) die "$variable must be a named Podman volume, not a host path: $value" ;;
    [!A-Za-z0-9]*|*[!A-Za-z0-9_.-]*) die "$variable is not a valid Podman volume name: $value" ;;
  esac
}

IMAGE="${IMAGE:-localhost/wp0-nix-devenv-spike:2026-11-10}"
NIX_VOLUME="${NIX_VOLUME:-wp0-spike-nix}"
CACHE_VOLUME="${CACHE_VOLUME:-wp0-spike-cache}"
NETWORK="${NETWORK:-bridge}"
OFFLINE="${OFFLINE:-0}"
RESET="${RESET:-0}"

case "$NETWORK" in
  bridge|none) ;;
  *) die "NETWORK must be 'bridge' or 'none'; host/container networking is rejected: $NETWORK" ;;
esac

case "$OFFLINE" in
  0|1) ;;
  *) die "OFFLINE must be 0 or 1" ;;
esac

case "$RESET" in
  0|1) ;;
  *) die "RESET must be 0 or 1" ;;
esac

validate_volume_name NIX_VOLUME "$NIX_VOLUME"
validate_volume_name CACHE_VOLUME "$CACHE_VOLUME"
require_rootless_podman

if [ "${CHECK_ONLY:-0}" = "1" ]; then
  printf 'run.sh: configuration valid; rootless Podman detected\n'
  exit 0
fi

if [ "$RESET" = "1" ]; then
  podman volume rm -f "$NIX_VOLUME" "$CACHE_VOLUME" >/dev/null 2>&1 || true
fi

podman build -t "$IMAGE" .
podman volume create "$NIX_VOLUME" >/dev/null 2>&1 || true
podman volume create "$CACHE_VOLUME" >/dev/null 2>&1 || true

exec podman run --rm \
  --network="$NETWORK" \
  --security-opt no-new-privileges \
  -v "$NIX_VOLUME:/nix" \
  -v "$CACHE_VOLUME:/cache" \
  -e "OFFLINE=$OFFLINE" \
  "$IMAGE" "$@"
