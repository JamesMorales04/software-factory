#!/usr/bin/env bash
# Host-side wrapper for the Nix/devenv spike.
#
# Only rootless Podman is required on the host. The image contains Nix, Node,
# Cargo and Playwright; `/nix` and `/cache` are Podman named volumes. There is
# no host bind mount, host networking, privileged mode or host socket.
set -euo pipefail

cd "$(dirname "$0")"

IMAGE="${IMAGE:-localhost/wp0-nix-devenv-spike:2026-11-10}"
NIX_VOLUME="${NIX_VOLUME:-wp0-spike-nix}"
CACHE_VOLUME="${CACHE_VOLUME:-wp0-spike-cache}"
NETWORK="${NETWORK:-bridge}"
OFFLINE="${OFFLINE:-0}"
RESET="${RESET:-0}"

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
