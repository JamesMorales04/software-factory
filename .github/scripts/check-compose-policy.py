#!/usr/bin/env python3
"""Static security policy for the developer-preview Compose topology.

Consumes `docker compose config --format json` so YAML anchors and extensions
are resolved. It does not prove runtime behavior; real Podman acceptance is a
separate manual harness.
"""
import json
import sys

EXPECTED_SERVICES = {
    "init",
    "provider-bundle",
    "runtime",
    "core",
    "supervisor",
    "worker",
    "verifier",
    "reviewer",
}
ALLOWED_RUNTIME_CAPS = {
    "CHOWN",
    "DAC_OVERRIDE",
    "FOWNER",
    "FSETID",
    "KILL",
    "NET_BIND_SERVICE",
    "SETFCAP",
    "SETGID",
    "SETPCAP",
    "SETUID",
    "SYS_CHROOT",
    "SYS_ADMIN",
    "NET_ADMIN",
}
FORBIDDEN_VOLUME_SUBSTRINGS = (
    "podman.sock",
    "/run/user",
    "/var/run/docker.sock",
    "/var/run/podman",
    "docker.sock",
)


def fail(message: str, errors: list[str]) -> None:
    errors.append(message)


def main(path: str) -> int:
    with open(path, encoding="utf-8") as handle:
        config = json.load(handle)
    services = config.get("services", {})
    errors: list[str] = []

    missing = EXPECTED_SERVICES - set(services)
    if missing:
        fail(f"missing services: {sorted(missing)}", errors)

    for name, service in services.items():
        if service.get("privileged"):
            fail(f"{name}: privileged mode is forbidden", errors)
        if service.get("network_mode") == "host":
            fail(f"{name}: host networking is forbidden", errors)
        for volume in service.get("volumes", []) or []:
            source = str(volume.get("source", ""))
            for forbidden in FORBIDDEN_VOLUME_SUBSTRINGS:
                if forbidden in source:
                    fail(f"{name}: forbidden volume source {source!r}", errors)

    runtime = services.get("runtime", {})
    caps = set(runtime.get("cap_add", []) or [])
    unexpected = caps - ALLOWED_RUNTIME_CAPS
    if unexpected:
        fail(f"runtime: unexpected capabilities {sorted(unexpected)}", errors)
    if "SYS_ADMIN" not in caps or "NET_ADMIN" not in caps:
        fail("runtime: required namespaced capabilities are missing", errors)
    if "ALL" not in set(runtime.get("cap_drop", []) or []):
        fail("runtime: capability drop ALL is required", errors)
    if not runtime.get("read_only"):
        fail("runtime: read-only root filesystem is required", errors)

    published = []
    for name, service in services.items():
        for port in service.get("ports", []) or []:
            published.append((name, port))
            if str(port.get("host_ip", "")) != "127.0.0.1":
                fail(f"{name}: published ports must bind 127.0.0.1", errors)
    if [name for name, _ in published] != ["core"]:
        fail(f"only core may publish a port; got {[n for n, _ in published]}", errors)

    for name in ("init", "provider-bundle"):
        service = services.get(name, {})
        if service.get("networks"):
            fail(f"{name}: one-shot initialization must not join a network", errors)

    for name in ("supervisor", "verifier", "reviewer"):
        networks = set((services.get(name, {}).get("networks") or {}).keys())
        if networks != {"factory-control"}:
            fail(f"{name}: must only join the internal control network", errors)

    volumes = config.get("volumes", {})
    for required in (
        "factory-data",
        "factory-projects",
        "factory-provider-sessions",
        "factory-image-bundle",
        "factory-engine",
    ):
        if required not in volumes:
            fail(f"missing durable volume {required}", errors)

    if errors:
        for error in errors:
            print(f"compose-policy: {error}", file=sys.stderr)
        return 1
    print("compose-policy: PASS")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: check-compose-policy.py /tmp/compose.json", file=sys.stderr)
        sys.exit(2)
    sys.exit(main(sys.argv[1]))
