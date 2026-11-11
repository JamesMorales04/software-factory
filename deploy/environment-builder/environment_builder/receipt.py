"""EnvironmentBuildReceipt and sandbox attestation construction."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from . import BUILDER_POLICY_REVISION, SCHEMA_VERSION
from .util import is_sha256, now_rfc3339, receipt_id

ALLOWED_MOUNT_PREFIXES = (
    "/",
    "/proc",
    "/sys",
    "/dev",
    "/etc/hosts",
    "/etc/hostname",
    "/etc/resolv.conf",
    "/tmp",
    "/var/tmp",
    "/run",
    "/home/factory",
    "/inputs/workspace",
    "/cache",
    "/nix",
    "/output",
)
AUTHORIZED_PROJECT_OR_CACHE_MOUNTS = ("/inputs/workspace", "/cache", "/nix")
PODMAN_VOLUME_ROOT = "/var/lib/containers/storage/volumes/"
CONTAINER_RUNTIME_FILE_PREFIX = "/var/lib/containers/storage/overlay-containers/"
CONTAINER_RUNTIME_FILES = {
    "/etc/hosts": "hosts",
    "/etc/hostname": "hostname",
    "/etc/resolv.conf": "resolv.conf",
}
# The canonical Compose runtime uses a private nested Podman engine with VFS
# storage. Inside that engine, a named volume is a directory on a real
# filesystem and its mountinfo `root` is
# `.../storage/volumes/<name>/_data`; the `source` is the backing block
# device. The outer engine's overlay paths remain accepted for the direct
# builder path. Both forms identify a runtime-managed mount, never an
# arbitrary host directory bind.
NESTED_VOLUME_FILESYSTEMS = {"btrfs", "xfs", "ext4", "overlay", "squashfs"}


def _mountinfo() -> tuple[list[dict[str, str]], bool]:
    entries: list[dict[str, str]] = []
    path = Path("/proc/self/mountinfo")
    if not path.exists():
        return entries, False
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return entries, False
    complete = bool(lines)
    for line in lines:
        if " - " not in line:
            complete = False
            continue
        before, after = line.split(" - ", 1)
        fields = before.split(" ")
        post = after.split(" ")
        if len(fields) < 5 or len(post) < 2:
            complete = False
            continue
        entries.append(
            {
                "root": fields[3],
                "mount_point": fields[4],
                "source": post[1],
                "fstype": post[0],
            }
        )
    return entries, complete and bool(entries)


def _podman_volume_source(source: str) -> bool:
    if not source.startswith(PODMAN_VOLUME_ROOT):
        return False
    parts = source[len(PODMAN_VOLUME_ROOT) :].strip("/").split("/")
    if len(parts) not in (1, 2) or (len(parts) == 2 and parts[1] != "_data"):
        return False
    name = parts[0]
    return bool(name) and name not in (".", "..") and "\\" not in name and "\0" not in name


def _nested_volume_root(root: str) -> bool:
    """Match ``.../storage/volumes/<name>/_data`` from a nested VFS engine."""
    parts = Path(root).parts
    return (
        len(parts) >= 4
        and parts[-4] == "storage"
        and parts[-3] == "volumes"
        and bool(parts[-2])
        and parts[-2] not in (".", "..")
        and parts[-1] == "_data"
    )


def _authorized_volume_source(source: str, root: str, fstype: str) -> bool:
    """True only for a runtime-managed named-volume mount.

    A host bind path at ``/cache``, ``/nix`` or ``/inputs/workspace`` is not
    attested as a no-host-mount builder. The brokered product path always uses
    named volumes; the host wrapper is a development harness. The nested VFS
    engine presents a named volume as a directory root on its backing block
    device, so the mountinfo ``root`` and a real-filesystem source are the
    runtime-managed identity.
    """
    if _podman_volume_source(source):
        return True
    return (
        source.startswith("/dev/")
        and fstype in NESTED_VOLUME_FILESYSTEMS
        and _nested_volume_root(root)
    )


def _overlay_container_runtime_file_source(source: str, expected: str) -> bool:
    """Require the exact normalized overlay container-runtime file source."""
    parts = Path(source).parts
    if any(part in (".", "..") for part in parts):
        return False
    prefix = Path(CONTAINER_RUNTIME_FILE_PREFIX).parts
    if parts[: len(prefix)] != prefix:
        return False
    remainder = parts[len(prefix) :]
    return (
        len(remainder) == 3
        and bool(remainder[0])
        and remainder[1] == "userdata"
        and remainder[2] == expected
    )


def _nested_container_runtime_file_root(root: str, expected: str) -> bool:
    """Match ``.../storage/vfs-containers/<id>/userdata/<file>``."""
    parts = Path(root).parts
    return (
        len(parts) >= 5
        and parts[-5] == "storage"
        and parts[-4] == "vfs-containers"
        and bool(parts[-3])
        and parts[-3] not in (".", "..")
        and parts[-2] == "userdata"
        and parts[-1] == expected
    )


def _container_runtime_file_source(
    source: str, root: str, fstype: str, expected: str
) -> bool:
    if _overlay_container_runtime_file_source(source, expected):
        return True
    return (
        source == "tmpfs"
        and fstype == "tmpfs"
        and _nested_container_runtime_file_root(root, expected)
    )


def _nested_container_root(root: str) -> bool:
    parts = Path(root).parts
    return any(
        parts[index] == "storage" and parts[index + 1] in {"vfs", "overlay"}
        for index in range(len(parts) - 1)
    )


def _safe_system_mount(target: str, source: str, fstype: str, root: str) -> bool:
    expected_runtime_file = CONTAINER_RUNTIME_FILES.get(target)
    if expected_runtime_file is not None:
        return _container_runtime_file_source(source, root, fstype, expected_runtime_file)
    if target == "/":
        if fstype in {"overlay", "rootfs", "squashfs"} and (
            source in {"overlay", "rootfs", "/dev/root"} or source.startswith("/dev/")
        ):
            return True
        return (
            source.startswith("/dev/")
            and fstype in NESTED_VOLUME_FILESYSTEMS
            and _nested_container_root(root)
        )
    if target == "/proc" or target.startswith("/proc/"):
        return source == "proc" and fstype == "proc"
    if target == "/sys/fs/cgroup":
        return source in {"cgroup", "cgroup2"} and fstype in {"cgroup", "cgroup2"}
    if target == "/sys" or target.startswith("/sys/"):
        return source == "sysfs" and fstype == "sysfs"
    if target == "/dev":
        return source in {"tmpfs", "devtmpfs", "udev"} and fstype in {
            "tmpfs",
            "devtmpfs",
        }
    if target == "/dev/pts":
        return source == "devpts" and fstype == "devpts"
    if target == "/dev/shm":
        return source in {"shm", "tmpfs"} and fstype == "tmpfs"
    if target == "/dev/mqueue":
        return source == "mqueue" and fstype == "mqueue"
    if target == "/run" or target.startswith("/run/"):
        return source == "tmpfs" and fstype == "tmpfs"
    if target == "/tmp" or target.startswith("/tmp/"):
        return _authorized_volume_source(source, root, fstype) or (
            source == "tmpfs" and fstype == "tmpfs"
        )
    if target == "/var/tmp" or target.startswith("/var/tmp/"):
        return source == "tmpfs" and fstype == "tmpfs"
    if target == "/home/factory" or target.startswith("/home/factory/"):
        return source == "tmpfs" and fstype == "tmpfs"
    if target == "/output" or target.startswith("/output/"):
        return _authorized_volume_source(source, root, fstype) or (
            source == "tmpfs" and fstype == "tmpfs"
        )
    return False


def _mount_attestation(mounts: list[dict[str, str]], complete: bool) -> bool:
    return complete and bool(mounts) and _mounts_authorized(mounts)


def _mounts_authorized(mounts: list[dict[str, str]]) -> bool:
    for mount in mounts:
        source = mount["source"]
        target = mount["mount_point"]
        fstype = mount.get("fstype", "")
        root = mount.get("root", "")
        # Validate the source before accepting any target, including "/" and
        # system mounts. A forbidden host path at /cache, /nix,
        # /inputs/workspace, / or /proc must never set noHostMount=true.
        if target in AUTHORIZED_PROJECT_OR_CACHE_MOUNTS or target.startswith(
            "/inputs/workspace/"
        ):
            if not _authorized_volume_source(source, root, fstype):
                return False
            continue
        if target == "/" or any(
            target == prefix or target.startswith(prefix + "/")
            for prefix in ALLOWED_MOUNT_PREFIXES
        ):
            if not _safe_system_mount(target, source, fstype, root):
                return False
            continue
        return False
    return True


def observe_attestation(
    *,
    resources: dict[str, int],
    sandbox_observed: bool,
    no_secrets: bool,
) -> dict[str, Any]:
    """Observe the builder's own runtime controls.

    The host wrapper is responsible for applying the container limits. This
    function observes what is actually visible inside the container and records
    truthful booleans; a missing cgroup bound is not assumed to be safe.
    """
    uid = os.geteuid()
    mounts, mountinfo_complete = _mountinfo()
    no_host_mount = _mount_attestation(mounts, mountinfo_complete)
    no_host_socket = not any(
        Path(socket).exists()
        for socket in (
            "/var/run/docker.sock",
            "/run/podman/podman.sock",
            "/run/containers/storage/overlay-containers",
        )
    )
    default_route = False
    for route in ("/proc/net/route", "/proc/net/ipv6_route"):
        path = Path(route)
        if not path.exists():
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        lines = text.splitlines()
        if route == "/proc/net/route":
            for line in lines[1:]:
                fields = line.split()
                # Iface Destination ...; a default route on a non-loopback
                # interface is host/network egress.
                if len(fields) >= 2 and fields[1] == "00000000" and fields[0] != "lo":
                    default_route = True
        if route == "/proc/net/ipv6_route":
            for line in lines:
                fields = line.split()
                # The last field is the interface. A default route on lo is
                # the disabled-network loopback route, not egress.
                if (
                    len(fields) >= 10
                    and fields[0] == "00000000000000000000000000000000"
                    and fields[-1] != "lo"
                ):
                    default_route = True
    bounded = _bounded_cgroup(resources)
    attestation = {
        "nonRoot": uid != 0,
        "noSecrets": no_secrets,
        "noHostNetwork": not default_route,
        "noHostSocket": no_host_socket,
        "noHostMount": no_host_mount,
        "sandbox": sandbox_observed,
        "impure": False,
        "boundedResources": bounded,
    }
    return attestation


def _read_cgroup(name: str) -> str | None:
    for base in ("/sys/fs/cgroup", "/sys/fs/cgroup/unified"):
        path = Path(base) / name
        if path.exists():
            try:
                return path.read_text(encoding="utf-8", errors="replace").strip()
            except OSError:
                continue
    return None


def limits_within_plan(
    cpu_quota: int,
    cpu_period: int,
    memory_limit: int,
    pids_limit: int,
    resources: dict[str, int],
) -> bool:
    """Return true only when every observed cgroup bound is at or below plan."""
    memory_bound = 0 < memory_limit <= resources["memoryMiB"] * 1024 * 1024
    pids_bound = 0 < pids_limit <= resources["pids"]
    # Exact plan bound: no multiplier, no "close enough" tolerance.
    cpu_bound = cpu_quota > 0 and cpu_period > 0 and (cpu_quota / cpu_period) <= resources["cpu"]
    return memory_bound and pids_bound and cpu_bound


def _bounded_cgroup(resources: dict[str, int]) -> bool:
    memory = _read_cgroup("memory.max")
    pids = _read_cgroup("pids.max")
    cpu = _read_cgroup("cpu.max")
    if memory in (None, "max") or pids in (None, "max") or cpu in (None, "max"):
        return False
    try:
        memory_limit = int(memory)
        pids_limit = int(pids)
        cpu_quota, cpu_period = (int(part) for part in cpu.split()[:2])
    except (TypeError, ValueError):
        return False
    return limits_within_plan(cpu_quota, cpu_period, memory_limit, pids_limit, resources)


def build_receipt(
    *,
    spec: dict[str, Any],
    status: str,
    store_path: str,
    output_sha256: str,
    log_ref: str,
    attestation: dict[str, Any],
    blockers: list[dict[str, str]],
    duration_ms: int,
    sbom_sha256: str = "",
) -> dict[str, Any]:
    bound_sbom = sbom_sha256 if is_sha256(sbom_sha256) else str(spec.get("sbomSha256", ""))
    return {
        "schemaVersion": SCHEMA_VERSION,
        "receiptId": receipt_id(),
        "specSha256": str(spec.get("specSha256", "")),
        "platform": str(spec.get("platform", "")),
        "builderImage": str(spec.get("builderImage", "")),
        "builderImageDigest": str(spec.get("builderImageDigest", "")),
        "builderPolicyRevision": BUILDER_POLICY_REVISION,
        "status": status,
        "storePath": store_path,
        "outputSha256": output_sha256 if is_sha256(output_sha256) else "",
        "sbomSha256": bound_sbom,
        "logRef": log_ref,
        "sandboxAttestation": attestation,
        "blockers": blockers,
        "durationMs": int(duration_ms),
        "createdAt": now_rfc3339(),
    }
