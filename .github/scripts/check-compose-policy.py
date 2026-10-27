#!/usr/bin/env python3
"""Static security policy for the developer-preview Compose topology.

Consumes `docker compose config --format json` so YAML anchors and extensions
are resolved. It rejects privileged containers, host networking, host binds,
unexpected capabilities, published ports outside Core loopback and cross-role
configuration volumes. It does not prove runtime behavior; real Podman
acceptance is the separate manual harness.
"""
import json
import sys

PERMANENT = {"core", "supervisor", "worker", "verifier", "reviewer"}
CONFIG_VOLUMES = {
    "runtime": "factory-config-runtime",
    "core": "factory-config-core",
    "supervisor": "factory-config-supervisor",
    "worker": "factory-config-worker",
    "verifier": "factory-config-verifier",
    "reviewer": "factory-config-reviewer",
}
ALL_CONFIG_VOLUMES = set(CONFIG_VOLUMES.values())
EXPECTED_NETWORKS = {
    "core": {"factory-control", "factory-egress"},
    "supervisor": {"factory-control"},
    "worker": {"factory-control", "factory-egress"},
    "verifier": {"factory-control"},
    "reviewer": {"factory-control"},
    "runtime": {"factory-control", "factory-egress"},
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
REQUIRED_RUNTIME_CAPS = {"SYS_ADMIN", "NET_ADMIN"}
FORBIDDEN_VOLUME_SUBSTRINGS = (
    "podman.sock",
    "/run/user",
    "/var/run/docker.sock",
    "/var/run/podman",
    "docker.sock",
)
INIT_ALLOWED_CAPS = {"CHOWN", "DAC_OVERRIDE", "FOWNER"}
HOST_NAMESPACE_KEYS = ("pid", "ipc", "uts", "cgroup")


def as_set(service, key):
    value = service.get(key) or []
    return set(value)


def main(path: str) -> int:
    with open(path, encoding="utf-8") as handle:
        config = json.load(handle)
    services = config.get("services", {})
    named_volumes = set((config.get("volumes") or {}).keys())
    errors: list[str] = []

    def fail(message: str) -> None:
        errors.append(message)

    missing = (PERMANENT | {"init", "provider-bundle", "runtime"}) - set(services)
    if missing:
        fail(f"missing services: {sorted(missing)}")
    if set(services) != PERMANENT | {"init", "provider-bundle", "runtime"}:
        fail(f"unexpected services: {sorted(set(services) - PERMANENT - {'init', 'provider-bundle', 'runtime'})}")

    published = []
    for name, service in services.items():
        if service.get("privileged"):
            fail(f"{name}: privileged mode is forbidden")
        if service.get("network_mode") == "host":
            fail(f"{name}: host networking is forbidden")
        if service.get("userns_mode"):
            fail(f"{name}: userns_mode override is forbidden")
        if service.get("devices"):
            fail(f"{name}: device grants are forbidden in the permanent stack")
        if service.get("sysctls"):
            fail(f"{name}: sysctls are forbidden in the permanent stack")
        for key in HOST_NAMESPACE_KEYS:
            if service.get(key):
                fail(f"{name}: {key} namespace override is forbidden")
        if service.get("volumes_from"):
            fail(f"{name}: volumes_from is forbidden")
        if service.get("device_cgroup_rules"):
            fail(f"{name}: device_cgroup_rules are forbidden")
        for option in service.get("security_opt", []) or []:
            normalized = str(option).replace(" ", "")
            if normalized != "no-new-privileges:true":
                fail(f"{name}: unsupported security_opt {option!r}")

        for volume in service.get("volumes", []) or []:
            source = str(volume.get("source", ""))
            target = str(volume.get("target", ""))
            for forbidden in FORBIDDEN_VOLUME_SUBSTRINGS:
                if forbidden in source:
                    fail(f"{name}: forbidden volume source {source!r}")
            if volume.get("type") == "bind":
                fail(f"{name}: host bind {source!r} -> {target!r} is forbidden in the base stack")
            elif source and source not in named_volumes:
                fail(f"{name}: volume {source!r} is not a declared named volume")

        if name in PERMANENT:
            if service.get("read_only") is not True:
                fail(f"{name}: read-only root filesystem is required")
            if as_set(service, "cap_drop") != {"ALL"}:
                fail(f"{name}: cap_drop must be exactly ALL")
            if as_set(service, "cap_add"):
                fail(f"{name}: permanent services must not add capabilities")
            if not any(
                str(option).replace(" ", "") == "no-new-privileges:true"
                for option in service.get("security_opt", []) or []
            ):
                fail(f"{name}: no-new-privileges is required")
            if str(service.get("user")) != "10001:10001":
                fail(f"{name}: must run as the unprivileged factory UID")
            expected_config = CONFIG_VOLUMES[name]
            config_sources = {
                str(volume.get("source", ""))
                for volume in service.get("volumes", []) or []
                if str(volume.get("source", "")) in ALL_CONFIG_VOLUMES
            }
            if config_sources != {expected_config}:
                fail(
                    f"{name}: must mount only {expected_config}; got {sorted(config_sources)}"
                )
            actual_networks = set((service.get("networks") or {}).keys())
            if actual_networks != EXPECTED_NETWORKS[name]:
                fail(
                    f"{name}: networks {sorted(actual_networks)} do not match {sorted(EXPECTED_NETWORKS[name])}"
                )

        for port in service.get("ports", []) or []:
            published.append((name, port))
            if str(port.get("host_ip", "")) != "127.0.0.1":
                fail(f"{name}: published ports must bind 127.0.0.1")

    if [name for name, _ in published] != ["core"]:
        fail(f"only core may publish a port; got {[name for name, _ in published]}")

    for name in ("init", "provider-bundle"):
        service = services.get(name, {})
        if service.get("network_mode") != "none":
            fail(f"{name}: one-shot initialization must use network_mode none")
        if service.get("networks"):
            fail(f"{name}: one-shot initialization must not join a network")
        if service.get("read_only") is not True:
            fail(f"{name}: read-only root filesystem is required")
        if as_set(service, "cap_drop") != {"ALL"}:
            fail(f"{name}: cap_drop must be exactly ALL")
        if str(service.get("user")) not in {"0", "0:0"}:
            fail(f"{name}: one-shot initialization must run as root only")
        if not any(
            str(option).replace(" ", "") == "no-new-privileges:true"
            for option in service.get("security_opt", []) or []
        ):
            fail(f"{name}: no-new-privileges is required")
        allowed_caps = INIT_ALLOWED_CAPS if name == "init" else {"DAC_OVERRIDE"}
        unexpected = as_set(service, "cap_add") - allowed_caps
        if unexpected:
            fail(f"{name}: unsupported one-shot capabilities {sorted(unexpected)}")

    init_config = {
        str(volume.get("source", ""))
        for volume in services.get("init", {}).get("volumes", []) or []
        if str(volume.get("source", "")) in ALL_CONFIG_VOLUMES
    }
    if init_config != ALL_CONFIG_VOLUMES:
        fail(f"init: must mount every per-role config volume; got {sorted(init_config)}")

    runtime = services.get("runtime", {})
    caps = as_set(runtime, "cap_add")
    unexpected = caps - ALLOWED_RUNTIME_CAPS
    if unexpected:
        fail(f"runtime: unexpected capabilities {sorted(unexpected)}")
    if not REQUIRED_RUNTIME_CAPS.issubset(caps):
        fail("runtime: required namespaced capabilities are missing")
    if as_set(runtime, "cap_drop") != {"ALL"}:
        fail("runtime: cap_drop must be exactly ALL")
    if runtime.get("read_only") is not True:
        fail("runtime: read-only root filesystem is required")
    if not any(
        str(option).replace(" ", "") == "no-new-privileges:true"
        for option in runtime.get("security_opt", []) or []
    ):
        fail("runtime: no-new-privileges is required")
    if str(runtime.get("user")) not in {"None", "", "0", "0:0"}:
        fail("runtime: the dedicated engine must start as root inside its user namespace")
    runtime_config = {
        str(volume.get("source", ""))
        for volume in runtime.get("volumes", []) or []
        if str(volume.get("source", "")) in ALL_CONFIG_VOLUMES
    }
    if runtime_config != {"factory-config-runtime"}:
        fail(f"runtime: must mount only its own config volume; got {sorted(runtime_config)}")

    for name, volume in (config.get("volumes") or {}).items():
        if volume.get("driver_opts"):
            fail(f"named volume {name}: driver_opts are forbidden (host bind risk)")

    for required in (
        "factory-data",
        "factory-projects",
        "factory-provider-sessions",
        "factory-image-bundle",
        "factory-engine",
    ):
        if required not in named_volumes:
            fail(f"missing durable volume {required}")

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
