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
    "/inputs/workspace",
    "/cache",
    "/output",
)
FORBIDDEN_MOUNT_SOURCES = (
    "/run/user",
    "/var/run",
    "/var/lib/containers",
    "podman.sock",
    "docker.sock",
    "/home/james",
    "/root",
)


def _mountinfo() -> list[dict[str, str]]:
    entries: list[dict[str, str]] = []
    path = Path("/proc/self/mountinfo")
    if not path.exists():
        return entries
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if " - " not in line:
            continue
        before, after = line.split(" - ", 1)
        fields = before.split(" ")
        post = after.split(" ")
        if len(fields) < 5 or len(post) < 2:
            continue
        entries.append(
            {
                "mount_point": fields[4],
                "source": post[1],
                "fstype": post[0],
            }
        )
    return entries


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
    mounts = _mountinfo()
    no_host_mount = True
    for mount in mounts:
        source = mount["source"]
        target = mount["mount_point"]
        if any(fragment in source for fragment in FORBIDDEN_MOUNT_SOURCES):
            no_host_mount = False
            break
        if target.startswith("/inputs/workspace") or target == "/cache" or target == "/":
            continue
        if any(target == prefix or target.startswith(prefix + "/") for prefix in ALLOWED_MOUNT_PREFIXES):
            continue
        no_host_mount = False
        break
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
        if route.endswith("route") and any(
            line.split() and line.split()[1] == "00000000" for line in text.splitlines()[1:]
        ):
            default_route = True
        if "ipv6_route" in route and any(
            line.split() and line.split()[0] == "00000000000000000000000000000000" for line in text.splitlines()
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
    memory_bound = 0 < memory_limit <= resources["memoryMiB"] * 1024 * 1024
    pids_bound = 0 < pids_limit <= resources["pids"]
    cpu_bound = cpu_quota > 0 and cpu_period > 0 and cpu_quota / cpu_period <= resources["cpu"] * 4
    return memory_bound and pids_bound and cpu_bound


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
) -> dict[str, Any]:
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
        "logRef": log_ref,
        "sandboxAttestation": attestation,
        "blockers": blockers,
        "durationMs": int(duration_ms),
        "createdAt": now_rfc3339(),
    }
