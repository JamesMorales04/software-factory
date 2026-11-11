#!/usr/bin/env bash
# Shared boundary helpers for the Nix/devenv spike.
#
# This file is sourced by run.sh, measure.sh and test-boundaries.sh. It is not
# a product library and grants no Factory authority.

die() {
  printf '%s: %s\n' "${0##*/}" "$*" >&2
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

# Remove a volume if it exists. Exit 1 means "absent" and is ignored; any other
# probe or removal failure is fatal. This prevents RESET=1 from silently
# reusing a store/cache after a failed removal while claiming a cold run.
remove_volume_if_present() {
  local name="$1"
  local status
  set +e
  podman volume exists "$name" >/dev/null 2>&1
  status=$?
  set -e
  case "$status" in
    0)
      podman volume rm -f "$name" >/dev/null \
        || die "failed to remove volume $name"
      ;;
    1)
      return 0
      ;;
    *)
      die "podman volume exists failed for $name (exit $status)"
      ;;
  esac
}
