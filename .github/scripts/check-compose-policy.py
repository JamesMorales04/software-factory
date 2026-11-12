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

REVIEWER_SERVICES = {
    "reviewer-functional",
    "reviewer-architecture",
    "reviewer-code-quality",
    "reviewer-security",
    "reviewer-testing",
    "reviewer-operability",
}
# Core's full control listener binds only these factory-control/factory-egress
# aliases. The plain `core` service name is attached to both networks and can
# resolve to an address where the control listener is not bound.
CANONICAL_CORE_CONTROL_URL = "http://core-control:8765"
LEGACY_CORE_CONTROL_HOST = "core"
LEGACY_CORE_CONTROL_PORT = 8765
PERMANENT = {"core", "supervisor", "worker", "verifier"} | REVIEWER_SERVICES
CONFIG_VOLUMES = {
    "runtime": "factory-config-runtime",
    "core": "factory-config-core",
    "supervisor": "factory-config-supervisor",
    "worker": "factory-config-worker",
    "verifier": "factory-config-verifier",
    "reviewer-functional": "factory-config-reviewer-functional",
    "reviewer-architecture": "factory-config-reviewer-architecture",
    "reviewer-code-quality": "factory-config-reviewer-code-quality",
    "reviewer-security": "factory-config-reviewer-security",
    "reviewer-testing": "factory-config-reviewer-testing",
    "reviewer-operability": "factory-config-reviewer-operability",
}
# Profile-gated one-shot jobs are not part of the steady-state stack, but they
# still need explicit credential, mount, network and privilege rules.
OPTIONAL_CONFIG_VOLUMES = {
    "openspec-validate": "factory-config-openspec",
}
ALL_CONFIG_VOLUMES = set(CONFIG_VOLUMES.values()) | set(
    OPTIONAL_CONFIG_VOLUMES.values()
)
EXPECTED_NETWORKS = {
    "core": {"factory-control", "factory-egress"},
    "supervisor": {"factory-control"},
    "worker": {"factory-control", "factory-egress"},
    "verifier": {"factory-control"},
    # Reviewers own dedicated Codex ChatGPT credentials and need outbound
    # access to the official App Server auth/review endpoints.
    "reviewer-functional": {"factory-control", "factory-egress"},
    "reviewer-architecture": {"factory-control", "factory-egress"},
    "reviewer-code-quality": {"factory-control", "factory-egress"},
    "reviewer-security": {"factory-control", "factory-egress"},
    "reviewer-testing": {"factory-control", "factory-egress"},
    "reviewer-operability": {"factory-control", "factory-egress"},
    "runtime": {"factory-control", "factory-egress"},
}
REVIEWER_IDENTITIES = {
    "reviewer-functional": (
        "/config/reviewer-functional.token",
        "reviewer-functional-process",
        "FUNCTIONAL",
    ),
    "reviewer-architecture": (
        "/config/reviewer-architecture.token",
        "reviewer-architecture-process",
        "ARCHITECTURE",
    ),
    "reviewer-code-quality": (
        "/config/reviewer-code-quality.token",
        "reviewer-code-quality-process",
        "CODE_QUALITY",
    ),
    "reviewer-security": (
        "/config/reviewer-security.token",
        "reviewer-security-process",
        "SECURITY",
    ),
    "reviewer-testing": (
        "/config/reviewer-testing.token",
        "reviewer-testing-process",
        "TESTING",
    ),
    "reviewer-operability": (
        "/config/reviewer-operability.token",
        "reviewer-operability-process",
        "OPERABILITY",
    ),
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


def network_names(service):
    networks = service.get("networks") or {}
    if isinstance(networks, dict):
        return set(networks.keys())
    return set(networks)


def bind_entries(value):
    return [entry.strip() for entry in str(value or "").split(",") if entry.strip()]


def targets_legacy_core_control(value):
    try:
        parsed = urlsplit(value)
        return (
            parsed.hostname == LEGACY_CORE_CONTROL_HOST
            and parsed.port == LEGACY_CORE_CONTROL_PORT
        )
    except ValueError:
        return False


def core_alias_networks(services):
    aliases = {}
    for network, config in (services.get("core", {}).get("networks") or {}).items():
        for alias in (config or {}).get("aliases") or []:
            aliases[alias] = network
    return aliases


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
    expected_web = {
        domain: {"Handlers": {"/": {"Proxy": "unix:/office-socket/office.sock"}}}
    }
    if serve.get("Web") != expected_web:
        errors.append("tailscale: Serve must proxy only to Core on its private network")
    if serve.get("AllowFunnel") != {domain: False} or "Funnel" in serve:
        errors.append("tailscale: Funnel must be explicitly disabled")
    return errors


def office_socket_volume_sources(config: dict) -> set[str]:
    """Resolve Compose volume keys that refer to the protected socket volume."""
    volumes = config.get("volumes") or {}
    protected_names = {"factory-office-socket"}
    socket_definition = volumes.get("factory-office-socket") or {}
    explicit_socket_name = socket_definition.get("name")
    if explicit_socket_name:
        protected_names.add(str(explicit_socket_name))
    elif config.get("name"):
        protected_names.add(f"{config['name']}_factory-office-socket")

    protected_sources = set()
    for source, definition in volumes.items():
        definition = definition or {}
        effective_name = definition.get("name")
        external = definition.get("external")
        if isinstance(external, dict):
            effective_name = external.get("name") or effective_name or source
        elif external is True and not effective_name:
            # Compose uses the source key as the physical name for an implicit
            # external volume. Treat it as an identity before checking aliases.
            effective_name = source
        if source == "factory-office-socket" or effective_name in protected_names:
            protected_sources.add(source)
    return protected_sources


def main(path: str) -> int:
    with open(path, encoding="utf-8") as handle:
        config = json.load(handle)
    services = config.get("services", {})
    volume_definitions = config.get("volumes") or {}
    named_volumes = set(volume_definitions.keys())
    office_socket_sources = office_socket_volume_sources(config)
    errors: list[str] = []

    def fail(message: str) -> None:
        errors.append(message)

    missing = (
        PERMANENT
        | {
            "init",
            "provider-bundle",
            "dependency-bundle",
            "environment-builder-bundle",
            "runtime",
        }
    ) - set(services)
    if missing:
        fail(f"missing services: {sorted(missing)}")
    has_tailscale = "tailscale" in services
    expected_services = PERMANENT | {
        "init",
        "provider-bundle",
        "dependency-bundle",
        "environment-builder-bundle",
        "runtime",
    }
    if has_tailscale:
        expected_services.add("tailscale")
    if "openspec-validate" in services:
        expected_services.add("openspec-validate")
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

        environment = service.get("environment") or {}
        for key, value in environment.items():
            if isinstance(value, str) and targets_legacy_core_control(value):
                fail(
                    f"{name}: {key} must use the canonical core-control control host, "
                    "not the ambiguous core service name"
                )
        if "FACTORY_CORE_URL" in environment:
            if environment["FACTORY_CORE_URL"] != CANONICAL_CORE_CONTROL_URL:
                fail(
                    f"{name}: FACTORY_CORE_URL must be "
                    f"{CANONICAL_CORE_CONTROL_URL} on the control network"
                )
            if "factory-control" not in network_names(service):
                fail(f"{name}: FACTORY_CORE_URL requires the factory-control network")

        for volume in service.get("volumes", []) or []:
            source = str(volume.get("source", ""))
            target = str(volume.get("target", ""))
            if source in office_socket_sources:
                if name not in {"init", "core", "tailscale"}:
                    fail(
                        f"{name}: Office socket volume is restricted to init, core, and tailscale"
                    )
                else:
                    expected_read_only = name == "tailscale"
                    if target != "/office-socket":
                        fail(f"{name}: Office socket volume must mount at /office-socket")
                    if (volume.get("read_only") is True) != expected_read_only:
                        access = "read-only" if expected_read_only else "writable"
                        fail(f"{name}: Office socket volume must be {access}")
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
            actual_networks = network_names(service)
            expected_networks = EXPECTED_NETWORKS[name]
            if actual_networks != expected_networks:
                fail(
                    f"{name}: networks {sorted(actual_networks)} do not match {sorted(expected_networks)}"
                )
            if name in REVIEWER_IDENTITIES:
                token_file, principal, role = REVIEWER_IDENTITIES[name]
                environment = service.get("environment") or {}
                if environment.get("FACTORY_REVIEWER_TOKEN_FILE") != token_file:
                    fail(f"{name}: must read its own reviewer token file")
                if environment.get("FACTORY_REVIEWER_PRINCIPAL") != principal:
                    fail(f"{name}: must use its own reviewer principal")
                if environment.get("FACTORY_REVIEWER_ROLE") != role:
                    fail(f"{name}: must use its assigned reviewer role")
                if environment.get("FACTORY_REVIEW_MECHANISM") not in {"AUTO", "MANUAL"}:
                    fail(f"{name}: reviewer mechanism must be explicit")
                if any(
                    key.upper().endswith(("TOKEN", "KEY", "SECRET"))
                    and str(value or "").strip()
                    for key, value in environment.items()
                ):
                    fail(f"{name}: reviewer secrets must be file-backed, not environment values")
                state_volume = f"factory-{name}-state"
                state_mounts = {
                    str(volume.get("source", ""))
                    for volume in service.get("volumes", []) or []
                    if str(volume.get("target", "")) == "/state"
                }
                if state_mounts != {state_volume}:
                    fail(f"{name}: must mount only its own reviewer state volume")

        if "openspec-validate" in services:
            job = services["openspec-validate"]
            if job.get("read_only") is not True:
                fail("openspec-validate: read-only root filesystem is required")
            if as_set(job, "cap_drop") != {"ALL"}:
                fail("openspec-validate: cap_drop must be exactly ALL")
            if as_set(job, "cap_add"):
                fail("openspec-validate: the validation job must not add capabilities")
            if not any(
                str(option).replace(" ", "") == "no-new-privileges:true"
                for option in job.get("security_opt", []) or []
            ):
                fail("openspec-validate: no-new-privileges is required")
            if str(job.get("user")) != "10001:10001":
                fail("openspec-validate: must run as the unprivileged factory UID")
            if job.get("profiles") != ["openspec-validation"]:
                fail("openspec-validate: only the openspec-validation profile is allowed")
            if job.get("restart") != "no":
                fail("openspec-validate: a one-shot job must not restart")
            if job.get("entrypoint") != ["node", "/opt/openspec/validate.mjs"]:
                fail("openspec-validate: the image-baked validation entrypoint is required")
            if job.get("image") != "localhost/software-factory-openspec:1.16.1":
                fail("openspec-validate: the pinned OpenSpec image is required")
            expected_config = OPTIONAL_CONFIG_VOLUMES["openspec-validate"]
            volumes = job.get("volumes") or []
            volume_sources = [str(volume.get("source", "")) for volume in volumes]
            if len(volumes) != 1 or volume_sources != [expected_config]:
                fail(
                    "openspec-validate: exactly one volume "
                    f"{expected_config} is allowed; got {volume_sources}"
                )
            else:
                volume = volumes[0]
                if str(volume.get("type", "")) != "volume":
                    fail("openspec-validate: the config mount must be a named volume")
                if (
                    str(volume.get("target", "")) != "/config"
                    or volume.get("read_only") is not True
                ):
                    fail("openspec-validate: its config volume must be read-only at /config")
            actual_networks = network_names(job)
            if actual_networks != {"factory-control"}:
                fail(
                    "openspec-validate: must use only the private control network; got "
                    f"{sorted(actual_networks)}"
                )
            environment = job.get("environment") or {}
            if environment.get("FACTORY_CORE_URL") != CANONICAL_CORE_CONTROL_URL:
                fail(
                    "openspec-validate: FACTORY_CORE_URL must target Core on the "
                    "control network via core-control"
                )
            if environment.get("FACTORY_OPERATOR_TOKEN_FILE") != "/config/operator.token":
                fail("openspec-validate: the operator token must be read from its file")
            if environment.get("FACTORY_OPENSPEC_WORKSPACE") != "/workspace":
                fail("openspec-validate: the isolated workspace path is required")
            if "FACTORY_OPENSPEC_REQUEST_ID" not in environment:
                fail("openspec-validate: the request id must be operator-supplied")
            if any(
                key.upper().endswith("TOKEN") and str(value or "").strip()
                for key, value in environment.items()
            ):
                fail("openspec-validate: secrets must not be passed in the environment")
            if not any(
                str(entry).startswith("/workspace:")
                for entry in job.get("tmpfs", []) or []
            ):
                fail("openspec-validate: the isolated workspace tmpfs is required")

        for port in service.get("ports", []) or []:
            published.append((name, port))
            if str(port.get("host_ip", "")) != "127.0.0.1":
                fail(f"{name}: published ports must bind 127.0.0.1")

    expected_published = ["core"]
    if [name for name, _ in published] != expected_published:
        fail(f"published ports must match mode; got {[name for name, _ in published]}")

    core = services.get("core", {})
    core_environment = core.get("environment") or {}
    aliases = core_alias_networks(services)
    allowed_core_networks = {
        "factory-control": {"core-control"},
        "factory-egress": {"core-egress"},
    }
    if set(core.get("networks") or {}) != set(EXPECTED_NETWORKS["core"]):
        fail("core: unexpected network membership")
    for network, network_config in (core.get("networks") or {}).items():
        network_config = network_config or {}
        if set(network_config) - {"aliases"}:
            fail(f"core: network address overrides are forbidden on {network}")
        if set(network_config.get("aliases") or []) != allowed_core_networks.get(
            network, set()
        ):
            fail(f"core: {network} aliases must match the reviewed listener hosts")

    def bind_networks(environment_key):
        resolved = []
        for entry in bind_entries(core_environment.get(environment_key)):
            host, separator, port = entry.rpartition(":")
            if not separator or not port.isdigit() or not host:
                fail(f"core: {environment_key} entries must be HOST:PORT")
                continue
            if aliases.get(host) is None:
                fail(f"core: {environment_key} host {host!r} must be a core network alias")
                continue
            resolved.append((host, aliases[host], int(port)))
        return resolved

    control_binds = bind_networks("FACTORY_BIND")
    if {(host, port) for host, _, port in control_binds} != {
        ("core-control", 8765),
        ("core-egress", 8765),
    }:
        fail("core: the full control listener must bind core-control and core-egress only")
    if {"factory-control", "factory-egress"} & set(
        (services.get("tailscale") or {}).get("networks") or {}
    ):
        fail("core: a control listener is reachable from the Tailscale network")
    if has_tailscale:
        if core_environment.get("FACTORY_OFFICE_ACCESS_SOCKET") != (
            "/office-socket/office.sock"
        ):
            fail("core: the Office socket listener path is required")
        if core_environment.get("FACTORY_OFFICE_ACCESS_BIND") is not None:
            fail("core: the Office listener must not bind a TCP port")
        socket_mounts = [
            volume
            for volume in core.get("volumes", []) or []
            if volume.get("source") == "factory-office-socket"
            and volume.get("target") == "/office-socket"
            and volume.get("read_only") is not True
        ]
        if not socket_mounts:
            fail("core: the Office socket volume must be mounted writable")
        for host, network, _ in control_binds:
            if network in ((services.get("tailscale") or {}).get("networks") or {}):
                fail(f"core: control listener host {host!r} is on the Tailscale network")
    else:
        if core_environment.get("FACTORY_OFFICE_ACCESS_SOCKET") is not None:
            fail("core: the Office listener requires the Tailscale overlay")
        if core_environment.get("FACTORY_OFFICE_ACCESS_BIND") is not None:
            fail("core: the Office listener must not bind a TCP port")

    for name in (
        "init",
        "provider-bundle",
        "dependency-bundle",
        "environment-builder-bundle",
    ):
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
        if name in {
            "provider-bundle",
            "dependency-bundle",
            "environment-builder-bundle",
        }:
            volumes = service.get("volumes") or []
            if len(volumes) != 1:
                fail(f"{name}: exactly one image bundle mount is allowed")
            else:
                volume = volumes[0]
                if (
                    str(volume.get("source", "")) != "factory-image-bundle"
                    or str(volume.get("target", "")) != "/bundle"
                    or str(volume.get("type", "")) != "volume"
                    or volume.get("read_only") is True
                ):
                    fail(f"{name}: must write only the shared image-bundle volume at /bundle")

    init_socket = [
        volume
        for volume in services.get("init", {}).get("volumes", []) or []
        if volume.get("source") == "factory-office-socket"
        and volume.get("target") == "/office-socket"
    ]
    if not init_socket:
        fail("init: must prepare the shared Office socket directory")
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
    runtime_environment = runtime.get("environment") or {}
    if runtime_environment.get("FACTORY_RUNTIME_BROKER_TOKEN_DIR") != "/config":
        fail("runtime: owner tokens must be read from its private config volume")
    if runtime_environment.get("FACTORY_RUNTIME_DEPENDENCY_IMAGE") != (
        "localhost/software-factory-dependency:dev"
    ):
        fail("runtime: the pinned dependency image is required")
    if runtime_environment.get("FACTORY_RUNTIME_DEPENDENCY_BUNDLE") != (
        "/bundles/dependency-rootfs.tar"
    ):
        fail("runtime: the dependency image bundle is required")
    if runtime_environment.get("FACTORY_RUNTIME_DEPENDENCY_EGRESS_NETWORK") != (
        "factory-deps-egress"
    ):
        fail("runtime: dependency acquisition must use its dedicated internal network")
    if runtime_environment.get("FACTORY_RUNTIME_DEPENDENCY_EGRESS_SUBNET") != "10.90.0.0/24":
        fail("runtime: dependency acquisition must use a fixed private subnet")
    if runtime_environment.get("FACTORY_RUNTIME_DEPENDENCY_EGRESS_GATEWAY") != "10.90.0.1":
        fail("runtime: dependency acquisition must use its fixed proxy gateway")
    if any(
        key.upper().endswith(("TOKEN", "KEY", "SECRET"))
        and str(value or "").strip()
        for key, value in runtime_environment.items()
    ):
        fail("runtime: broker secrets must be file-backed, not environment values")

    if has_tailscale:
        tailscale = services["tailscale"]
        expected_image = (
            "docker.io/tailscale/tailscale@sha256:"
            "ed6480b33c6eb67c9cbd203ed42b9fb57e361986725b6856def71b7a3d9e276c"
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
            ("factory-office-socket", "/office-socket", "volume", True),
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
            fail(
                "tailscale: mounts must be limited to persistent state, the read-only Office socket and Serve config"
            )
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
        if set(tailscale.get("networks") or {}) != {"factory-tailscale-uplink"}:
            fail("tailscale: must use only the dedicated uplink network")
        for network_config in (tailscale.get("networks") or {}).values():
            if set(network_config or {}) - {"aliases"}:
                fail("tailscale: network address overrides are forbidden")
            if (network_config or {}).get("aliases"):
                fail("tailscale: network aliases are forbidden")
        uplink_members = {
            service_name for service_name, service in services.items()
            if "factory-tailscale-uplink" in (service.get("networks") or {})
        }
        if uplink_members != {"tailscale"}:
            fail("factory-tailscale-uplink: only Tailscale may be a member")
        if set(tailscale.get("networks") or {}) & set(core.get("networks") or {}):
            fail("tailscale: must not share any Core network")
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
        if "factory-office-socket" not in named_volumes:
            fail("tailscale: the shared Office socket volume is required")

        if (config.get("networks", {}).get("factory-tailscale-uplink", {}).get("internal") is True):
            fail("factory-tailscale-uplink: outbound connectivity is required")

    for network_name, network in (config.get("networks") or {}).items():
        driver_opts = (network or {}).get("driver_opts") or {}
        if str(driver_opts.get("isolate")).lower() != "true":
            fail(f"network {network_name}: driver_opts.isolate must be 'true'")
    if (config.get("networks", {}).get("factory-control", {}).get("internal") is True):
        fail("factory-control: internal networks are not bridge-isolated by Netavark")
    if (
        (config.get("networks", {}).get("factory-control", {}).get("driver_opts") or {}).get(
            "no_default_route"
        )
        != "true"
    ):
        fail("factory-control: no_default_route=true is required to deny egress")
    for network_name in ("factory-egress", "factory-tailscale-uplink"):
        if (config.get("networks", {}).get(network_name, {}).get("internal") is True):
            fail(f"{network_name}: outbound connectivity is required")

    for name, volume in (config.get("volumes") or {}).items():
        if volume.get("driver_opts"):
            fail(f"named volume {name}: driver_opts are forbidden (host bind risk)")

    for required in (
        "factory-data",
        "factory-projects",
        "factory-provider-sessions",
        "factory-image-bundle",
        "factory-engine",
        "factory-office-socket",
        "factory-codex-home",
        "factory-supervisor-state",
        "factory-verifier-state",
    ):
        if required not in named_volumes:
            fail(f"missing durable volume {required}")
    for service in REVIEWER_SERVICES:
        if f"factory-{service}-state" not in named_volumes:
            fail(f"missing reviewer state volume for {service}")

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
