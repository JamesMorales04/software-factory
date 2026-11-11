#!/usr/bin/env bash
# Quick, auditable measurements for the Nix/devenv spike.
#
# Requires a completed cold run and the default named volumes. It performs no
# network access and no long downloads; the only build is a cached Containerfile
# rebuild used to record the image rebuild time.
set -euo pipefail

cd "$(dirname "$0")"

die() {
  printf 'measure.sh: %s\n' "$*" >&2
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
OUTPUT="${MEASUREMENTS_FILE:-evidence/measurements.log}"

validate_volume_name NIX_VOLUME "$NIX_VOLUME"
validate_volume_name CACHE_VOLUME "$CACHE_VOLUME"
require_rootless_podman

nix_mount="$(podman volume inspect "$NIX_VOLUME" --format '{{.Mountpoint}}' 2>/dev/null)" \
  || die "volume $NIX_VOLUME does not exist; run the cold spike first"
cache_mount="$(podman volume inspect "$CACHE_VOLUME" --format '{{.Mountpoint}}' 2>/dev/null)" \
  || die "volume $CACHE_VOLUME does not exist; run the cold spike first"

mkdir -p "$(dirname "$OUTPUT")"
{
  printf 'timestamp_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'image=%s\n' "$IMAGE"
  printf 'nix_volume=%s\n' "$NIX_VOLUME"
  printf 'cache_volume=%s\n' "$CACHE_VOLUME"

  image_size="$(podman image inspect "$IMAGE" --format '{{.Size}}' 2>/dev/null)" \
    || die "image $IMAGE does not exist; run the spike first"
  printf 'image_size_bytes=%s\n' "$image_size"
  printf 'image_size_human=%s\n' "$(numfmt --to=iec-i --suffix=B "$image_size" 2>/dev/null || printf '%s bytes' "$image_size")"

  printf 'nix_store_du_bytes=%s\n' "$(du -sb "$nix_mount" | awk '{print $1}')"
  printf 'nix_store_du_human=%s\n' "$(du -sh "$nix_mount" | awk '{print $1}')"
  printf 'cache_du_bytes=%s\n' "$(du -sb "$cache_mount" | awk '{print $1}')"
  printf 'cache_du_human=%s\n' "$(du -sh "$cache_mount" | awk '{print $1}')"

  printf 'closure_measurements:\n'
  podman run --rm --network=none --security-opt no-new-privileges \
    -v "$NIX_VOLUME:/nix" -v "$CACHE_VOLUME:/cache" \
    --entrypoint sh "$IMAGE" -c '
      set -eu
      cd /spike
      nix build .#devShells.x86_64-linux.node --no-link >/dev/null
      nix build .#devShells.x86_64-linux.rust --no-link >/dev/null
      nix path-info -Sh .#devShells.x86_64-linux.node
      nix path-info -Sh .#devShells.x86_64-linux.rust
    '
  printf 'closure_measurements_end\n'

  start="$(date +%s%N)"
  podman build -t "$IMAGE" . >/dev/null
  end="$(date +%s%N)"
  printf 'cached_image_rebuild_ms=%s\n' "$(( (end - start) / 1000000 ))"
} 2>&1 | tee "$OUTPUT"
