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
from pathlib import Path
from urllib.parse import urlsplit

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


def tailscale_serve_config_errors(path: Path) -> list[str]:
    try:
        with path.open(encoding="utf-8") as handle:
            serve = json.load(handle)
    except (OSError, json.JSONDecodeError) as error:
        return [f"tailscale: cannot read Serve config: {error}"]

    errors = []
    if set(serve) != {"TCP", "Web", "AllowFunnel"}:
        errors.append("tailscale: Serve config must contain only the approved endpoints")
    domain = "${TS_CERT_DOMAIN}:443"
    if serve.get("TCP") != {"443": {"HTTPS": True}}:
        errors.append("tailscale: Serve must expose HTTPS 443 only")
    expected_web = {domain: {"Handlers": {"/": {"Proxy": "http://core:8768"}}}}
    if serve.get("Web") != expected_web:
        errors.append("tailscale: Serve must proxy only to Core on its private network")
    if serve.get("AllowFunnel") != {domain: False} or "Funnel" in serve:
        errors.append("tailscale: Funnel must be explicitly disabled")
    return errors


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
    has_tailscale = "tailscale" in services
    expected_services = PERMANENT | {"init", "provider-bundle", "runtime"}
    if has_tailscale:
        expected_services.add("tailscale")
    if set(services) != expected_services:
        fail(f"unexpected services: {sorted(set(services) - expected_services)}")

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
                expected_serve_path = (
                    Path(__file__).resolve().parents[2]
                    / "deploy"
                    / "tailscale"
                    / "serve.json"
                ).resolve()
                try:
                    resolved_source = Path(source).resolve(strict=True)
                except (OSError, RuntimeError):
                    resolved_source = None
                allowed_tailscale_config = (
                    has_tailscale
                    and name == "tailscale"
                    and resolved_source == expected_serve_path
                    and target == "/etc/factory/serve.json"
                    and volume.get("read_only") is True
                )
                if not allowed_tailscale_config:
                    fail(f"{name}: host bind {source!r} -> {target!r} is forbidden")
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
            expected_networks = EXPECTED_NETWORKS[name]
            if has_tailscale and name == "core":
                expected_networks = expected_networks | {"factory-office-private"}
            if actual_networks != expected_networks:
                fail(
                    f"{name}: networks {sorted(actual_networks)} do not match {sorted(expected_networks)}"
                )

        for port in service.get("ports", []) or []:
            published.append((name, port))
            if str(port.get("host_ip", "")) != "127.0.0.1":
                fail(f"{name}: published ports must bind 127.0.0.1")

    expected_published = ["core"]
    if [name for name, _ in published] != expected_published:
        fail(f"published ports must match mode; got {[name for name, _ in published]}")

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

    if has_tailscale:
        tailscale = services["tailscale"]
        expected_image = (
            "docker.io/tailscale/tailscale@sha256:"
            "2667499ed87ae29218f292556ba062918402dd5e92e93637af14867e4df12dd3"
        )
        if tailscale.get("image") != expected_image:
            fail("tailscale: official image must remain pinned to the reviewed digest")
        if tailscale.get("privileged") or tailscale.get("network_mode") == "host":
            fail("tailscale: privileged mode and host networking are forbidden")
        for override in (
            "command",
            "entrypoint",
            "extra_hosts",
            "dns",
            "dns_search",
            "links",
            "network_mode",
        ):
            if tailscale.get(override):
                fail(f"tailscale: {override} overrides are forbidden")
        if tailscale.get("devices") or tailscale.get("ports"):
            fail("tailscale: device and published-port grants are forbidden")
        if as_set(tailscale, "cap_drop") != {"ALL"} or as_set(tailscale, "cap_add"):
            fail("tailscale: all capabilities must be dropped")
        if tailscale.get("read_only") is not True:
            fail("tailscale: read-only root filesystem is required")
        if not any(
            str(option).replace(" ", "") == "no-new-privileges:true"
            for option in tailscale.get("security_opt", []) or []
        ):
            fail("tailscale: no-new-privileges is required")
        environment = tailscale.get("environment") or {}
        allowed_environment = {
            "TS_ACCEPT_DNS",
            "TS_AUTH_ONCE",
            "TS_ENABLE_HEALTH_CHECK",
            "TS_HOSTNAME",
            "TS_LOCAL_ADDR_PORT",
            "TS_SERVE_CONFIG",
            "TS_STATE_DIR",
            "TS_USERSPACE",
        }
        unexpected_environment = set(environment) - allowed_environment
        if unexpected_environment:
            fail(
                "tailscale: unsupported environment variable(s): "
                f"{sorted(unexpected_environment)}"
            )
        if environment.get("TS_USERSPACE") != "true":
            fail("tailscale: userspace networking is required")
        if environment.get("TS_AUTH_ONCE") != "true":
            fail("tailscale: persistent one-time login is required")
        if environment.get("TS_SERVE_CONFIG") != "/etc/factory/serve.json":
            fail("tailscale: Serve config path is required")
        if environment.get("TS_STATE_DIR") != "/var/lib/tailscale":
            fail("tailscale: persistent node state path is required")
        if environment.get("TS_ENABLE_HEALTH_CHECK") != "true":
            fail("tailscale: the sanitized health endpoint is required")
        if environment.get("TS_LOCAL_ADDR_PORT") != "0.0.0.0:9002":
            fail("tailscale: health endpoint must use the private container port")
        state_mounts = [
            volume for volume in tailscale.get("volumes", []) or []
            if volume.get("source") == "factory-tailscale-state"
            and volume.get("target") == "/var/lib/tailscale"
            and volume.get("type") == "volume"
            and volume.get("read_only") is not True
        ]
        if not state_mounts:
            fail("tailscale: node state must use its persistent writable volume")
        expected_serve_path = (
            Path(__file__).resolve().parents[2] / "deploy" / "tailscale" / "serve.json"
        ).resolve()
        errors.extend(tailscale_serve_config_errors(expected_serve_path))
        allowed_tailscale_mounts = {
            ("factory-tailscale-state", "/var/lib/tailscale", "volume", False),
            (str(expected_serve_path), "/etc/factory/serve.json", "bind", True),
        }
        actual_tailscale_mounts = {
            (
                str(volume.get("source", "")),
                str(volume.get("target", "")),
                str(volume.get("type", "")),
                volume.get("read_only", False) is True,
            )
            for volume in tailscale.get("volumes", []) or []
        }
        if actual_tailscale_mounts != allowed_tailscale_mounts:
            fail("tailscale: mounts must be limited to persistent state and read-only Serve config")
        if any(
            environment.get(name)
            for name in ("TS_AUTHKEY", "TS_CLIENT_SECRET", "TS_ID_TOKEN")
        ):
            fail("tailscale: authentication material must not be in Compose environment")
        if environment.get("TS_EXTRA_ARGS") or environment.get("TS_TAILSCALED_EXTRA_ARGS"):
            fail("tailscale: extra Tailscale features and flags are forbidden")
        if (
            tailscale.get("cpus") != 0.5
            or tailscale.get("mem_limit") != "268435456"
            or tailscale.get("pids_limit") != 128
        ):
            fail("tailscale: CPU, memory and PID bounds are required")
        if set(tailscale.get("networks") or {}) != {
            "factory-office-private",
            "factory-tailscale-uplink",
        }:
            fail("tailscale: must use only the isolated Office and uplink networks")
        if any(value is not None for value in (tailscale.get("networks") or {}).values()):
            fail("tailscale: network aliases and address overrides are forbidden")
        expected_core_networks = EXPECTED_NETWORKS["core"] | {"factory-office-private"}
        if set(services["core"].get("networks", {})) != expected_core_networks:
            fail("core: Tailscale mode must expose only its dedicated Office network")
        if services["core"].get("environment", {}).get("FACTORY_OFFICE_ACCESS_BIND") != "0.0.0.0:8768":
            fail("core: Tailscale mode must enable the restricted Office listener")
        if any(
            value is not None
            for value in (services["core"].get("networks") or {}).values()
        ):
            fail("core: network aliases and address overrides are forbidden in Tailscale mode")
        office_members = {
            service_name for service_name, service in services.items()
            if "factory-office-private" in (service.get("networks") or {})
        }
        uplink_members = {
            service_name for service_name, service in services.items()
            if "factory-tailscale-uplink" in (service.get("networks") or {})
        }
        if office_members != {"core", "tailscale"}:
            fail("factory-office-private: only Core and Tailscale may be members")
        if uplink_members != {"tailscale"}:
            fail("factory-tailscale-uplink: only Tailscale may be a member")
        trusted_origin = str(
            services["core"].get("environment", {}).get("FACTORY_TRUSTED_ORIGINS", "")
        )
        tailscale_origin = str(
            services["core"].get("environment", {}).get("FACTORY_TAILSCALE_ORIGIN", "")
        )
        parsed_origin = urlsplit(trusted_origin)
        hostname = str(environment.get("TS_HOSTNAME", ""))
        try:
            origin_port = parsed_origin.port
        except ValueError:
            origin_port = -1
        valid_origin = (
            parsed_origin.scheme == "https"
            and bool(parsed_origin.hostname)
            and parsed_origin.hostname.endswith(".ts.net")
            and origin_port is None
            and parsed_origin.path in {"", "/"}
            and not parsed_origin.query
            and not parsed_origin.fragment
            and not parsed_origin.username
            and not parsed_origin.password
            and "," not in trusted_origin
            and "*" not in trusted_origin
            and parsed_origin.hostname.split(".")[0] == hostname
        )
        if not valid_origin or tailscale_origin != trusted_origin:
            fail(
                "core: trusted and status origins must match one exact HTTPS "
                "MagicDNS origin for TS_HOSTNAME"
            )
        if "factory-tailscale-state" not in named_volumes:
            fail("tailscale: persistent node state volume is required")
        if (config.get("networks", {}).get("factory-office-private", {}).get("internal") is not True):
            fail("factory-office-private: network must be internal")
        if (config.get("networks", {}).get("factory-tailscale-uplink", {}).get("internal") is True):
            fail("factory-tailscale-uplink: outbound connectivity is required")

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
