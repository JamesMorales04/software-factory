#!/bin/sh
set -eu

if [ "$(id -u)" != "0" ]; then
  echo "factory-runtime: the dedicated engine must start as root inside its rootless container" >&2
  exit 78
fi

until [ -f /config/.ready ]; do
  sleep 1
done
until [ -f /bundles/provider-rootfs.tar.ready ]; do
  sleep 1
done

if [ ! -f /sys/fs/cgroup/cgroup.controllers ]; then
  echo "factory-runtime: cgroup v2 is required for bounded child execution" >&2
  exit 78
fi

# Podman mounts /sys/fs/cgroup read-only by default. The dedicated engine owns
# only its own delegated subtree; remounting cgroup2 here does not grant host
# cgroup authority.
mount -t cgroup2 none /sys/fs/cgroup 2>/dev/null || true
mkdir -p /sys/fs/cgroup/runtime
for pid in $(cat /sys/fs/cgroup/cgroup.procs); do
  echo "$pid" > /sys/fs/cgroup/runtime/cgroup.procs 2>/dev/null || true
done
if ! echo "+cpu +memory +pids" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null; then
  echo "factory-runtime: cannot enable cgroup controllers for child execution" >&2
  exit 78
fi
for controller in cpu memory pids; do
  if ! grep -qw "$controller" /sys/fs/cgroup/cgroup.subtree_control; then
    echo "factory-runtime: cgroup controller $controller is unavailable" >&2
    exit 78
  fi
done

mkdir -p /var/lib/containers/tmp
export TMPDIR=/var/lib/containers/tmp
exec /usr/local/bin/factory-runtime-broker
