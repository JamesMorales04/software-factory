#!/usr/bin/env python3
"""Negative tests for the static Compose security policy."""
import copy
import json
import os
import runpy
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHECKER = HERE / "check-compose-policy.py"


def base_config():
    path = os.environ.get("COMPOSE_CONFIG_JSON", "/tmp/compose.json")
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


class ComposePolicyTests(unittest.TestCase):
    def assert_rejected(self, mutate, expected_fragment):
        config = copy.deepcopy(base_config())
        mutate(config)
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
            json.dump(config, handle)
            path = handle.name
        try:
            result = subprocess.run(
                ["python3", str(CHECKER), path],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        finally:
            os.unlink(path)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(expected_fragment, result.stderr)

    def test_base_configuration_passes(self):
        config = copy.deepcopy(base_config())
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
            json.dump(config, handle)
            path = handle.name
        try:
            result = subprocess.run(
                ["python3", str(CHECKER), path],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        finally:
            os.unlink(path)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_worker_extra_capability(self):
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__("cap_add", ["SYS_ADMIN"]),
            "permanent services must not add capabilities",
        )

    def test_rejects_worker_writable_root(self):
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__("read_only", False),
            "read-only root filesystem",
        )

    def test_rejects_init_bridge_network(self):
        self.assert_rejected(
            lambda c: c["services"]["init"].__setitem__("network_mode", "bridge"),
            "network_mode none",
        )

    def test_rejects_writable_or_privileged_one_shot_services(self):
        self.assert_rejected(
            lambda c: c["services"]["init"].__setitem__("read_only", False),
            "init: read-only root filesystem is required",
        )
        self.assert_rejected(
            lambda c: c["services"]["provider-bundle"].__setitem__(
                "cap_add", ["SYS_ADMIN"]
            ),
            "provider-bundle: unsupported one-shot capabilities",
        )
        self.assert_rejected(
            lambda c: c["services"]["init"].__setitem__(
                "cap_add", ["CHOWN", "DAC_OVERRIDE", "SYS_ADMIN"]
            ),
            "init: unsupported one-shot capabilities",
        )

    def test_rejects_host_namespaces_and_shared_mount_sources(self):
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__("pid", "host"),
            "pid namespace override is forbidden",
        )
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__("ipc", "host"),
            "ipc namespace override is forbidden",
        )
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__(
                "volumes_from", ["core"]
            ),
            "volumes_from is forbidden",
        )
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__(
                "device_cgroup_rules", ["c 1:3 rwm"]
            ),
            "device_cgroup_rules are forbidden",
        )

    def test_rejects_unsafe_security_options(self):
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__(
                "security_opt", ["no-new-privileges:true", "seccomp=unconfined"]
            ),
            "unsupported security_opt",
        )

    def test_rejects_cross_role_config_volume(self):
        def mutate(config):
            volumes = config["services"]["verifier"]["volumes"]
            for volume in volumes:
                if volume.get("source") == "factory-config-verifier":
                    volume["source"] = "factory-config-core"

        self.assert_rejected(mutate, "must mount only factory-config-verifier")

    def test_rejects_extra_published_port(self):
        self.assert_rejected(
            lambda c: c["services"]["worker"].__setitem__(
                "ports", [{"host_ip": "127.0.0.1", "published": "9999", "target": 9999}]
            ),
            "published ports must match mode",
        )

    def test_rejects_privileged_runtime(self):
        self.assert_rejected(
            lambda c: c["services"]["runtime"].__setitem__("privileged", True),
            "privileged mode is forbidden",
        )

    def test_rejects_runtime_unexpected_capability(self):
        def mutate(config):
            config["services"]["runtime"].setdefault("cap_add", []).append("SYS_PTRACE")

        self.assert_rejected(mutate, "unexpected capabilities")

    def test_rejects_runtime_lost_no_new_privileges(self):
        self.assert_rejected(
            lambda c: c["services"]["runtime"].__setitem__("security_opt", []),
            "runtime: no-new-privileges is required",
        )

    def test_rejects_named_volume_driver_bind(self):
        self.assert_rejected(
            lambda c: c["volumes"]["factory-engine"].__setitem__(
                "driver_opts",
                {"type": "none", "device": "/var/lib/factory-host-engine", "o": "bind"},
            ),
            "driver_opts are forbidden",
        )

    def test_rejects_host_bind(self):
        def mutate(config):
            config["services"]["core"]["volumes"].append(
                {"type": "bind", "source": "/tmp", "target": "/tmp"}
            )

        self.assert_rejected(mutate, "host bind")


    def test_tailscale_serve_config_is_private_https_only(self):
        if "tailscale" not in base_config()["services"]:
            self.skipTest("optional Tailscale overlay is not present in this config")
        serve_path = HERE.parents[1] / "deploy" / "tailscale" / "serve.json"
        with serve_path.open(encoding="utf-8") as handle:
            serve = json.load(handle)
        self.assertEqual(serve["TCP"], {"443": {"HTTPS": True}})
        self.assertEqual(
            serve["Web"]["${TS_CERT_DOMAIN}:443"]["Handlers"]["/"],
            {"Proxy": "unix:/office-socket/office.sock"},
        )
        self.assertEqual(serve["AllowFunnel"], {"${TS_CERT_DOMAIN}:443": False})
        self.assertNotIn("Funnel", serve)
        checker = runpy.run_path(str(CHECKER))
        with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as handle:
            serve["Funnel"] = {"443": True}
            json.dump(serve, handle)
            serve_path = Path(handle.name)
        try:
            errors = checker["tailscale_serve_config_errors"](serve_path)
        finally:
            serve_path.unlink()
        self.assertIn("tailscale: Funnel must be explicitly disabled", errors)

    def test_rejects_non_loopback_office_publication_in_tailscale_mode(self):
        if "tailscale" not in base_config()["services"]:
            self.skipTest("optional Tailscale overlay is not present in this config")
        self.assert_rejected(
            lambda c: c["services"]["core"]["ports"][0].__setitem__("host_ip", "0.0.0.0"),
            "published ports must bind 127.0.0.1",
        )

    def test_rejects_unsafe_optional_tailscale_configuration(self):
        if "tailscale" not in base_config()["services"]:
            self.skipTest("optional Tailscale overlay is not present in this config")
        self.assert_rejected(
            lambda c: c["services"]["tailscale"].__setitem__("cap_add", ["NET_ADMIN"]),
            "all capabilities must be dropped",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["environment"].__setitem__("TS_USERSPACE", "false"),
            "userspace networking is required",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["environment"].__setitem__("TS_AUTHKEY", "unexpected"),
            "unsupported environment variable(s)",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["environment"].__setitem__("TS_EXTRA_ARGS", "--ssh"),
            "unsupported environment variable(s)",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["environment"].__setitem__("TS_STATE_DIR", "/tmp/state"),
            "persistent node state path is required",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["environment"].__setitem__(
                "TS_ENABLE_HEALTH_CHECK", "false"
            ),
            "sanitized health endpoint is required",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["environment"].__setitem__(
                "TS_LOCAL_ADDR_PORT", "127.0.0.1:9003"
            ),
            "health endpoint must use the private container port",
        )
        def move_state_volume(config):
            state_volume = next(
                volume
                for volume in config["services"]["tailscale"]["volumes"]
                if volume.get("source") == "factory-tailscale-state"
            )
            state_volume["target"] = "/tmp/state"

        self.assert_rejected(
            move_state_volume,
            "node state must use its persistent writable volume",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"].__setitem__(
                "ports",
                [{"host_ip": "0.0.0.0", "published": "443", "target": 443}],
            ),
            "device and published-port grants are forbidden",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"].__setitem__("networks", {"factory-control": {}}),
            "must not share any Core network",
        )

    def test_rejects_tailscale_before_unix_backend_security_fixes(self):
        if "tailscale" not in base_config()["services"]:
            return
        self.assert_rejected(
            lambda c: c["services"]["tailscale"].__setitem__(
                "image",
                "docker.io/tailscale/tailscale@sha256:"
                "2667499ed87ae29218f292556ba062918402dd5e92e93637af14867e4df12dd3",
            ),
            "official image must remain pinned to the reviewed digest",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["volumes"].append(
                {
                    "type": "volume",
                    "source": "factory-config-core",
                    "target": "/stolen-core-config",
                    "read_only": True,
                }
            ),
            "mounts must be limited to persistent state, the read-only Office socket and Serve config",
        )
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__("FACTORY_TRUSTED_ORIGINS", "https://*.ts.net"),
            "one exact HTTPS MagicDNS origin",
        )
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__(
                "FACTORY_TAILSCALE_ORIGIN", "https://other-host.tail4a7adf.ts.net"
            ),
            "trusted and status origins must match",
        )
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__(
                "FACTORY_OFFICE_ACCESS_BIND", "0.0.0.0:8765"
            ),
            "must not bind a TCP port",
        )
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__(
                "FACTORY_OFFICE_ACCESS_SOCKET", "/tmp/office.sock"
            ),
            "Office socket listener path is required",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["networks"].__setitem__(
                "factory-control", {}
            ),
            "must not share any Core network",
        )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["networks"].__setitem__(
                "unexpected-network", {}
            ),
            "must use only the dedicated uplink network",
        )

    def test_rejects_wildcard_or_misplaced_full_control_listener(self):
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__(
                "FACTORY_BIND", "0.0.0.0:8765"
            ),
            "full control listener must bind",
        )
        if "tailscale" not in base_config()["services"]:
            return
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__(
                "FACTORY_BIND", "core-office:8765"
            ),
            "full control listener must bind",
        )

    def test_rejects_office_tcp_listener(self):
        self.assert_rejected(
            lambda c: c["services"]["core"]["environment"].__setitem__(
                "FACTORY_OFFICE_ACCESS_BIND", "0.0.0.0:8768"
            ),
            "must not bind a TCP port",
        )
        if "tailscale" not in base_config()["services"]:
            return
        self.assert_rejected(
            lambda c: [
                volume.__setitem__("read_only", False)
                for volume in c["services"]["tailscale"]["volumes"]
                if volume.get("source") == "factory-office-socket"
            ],
            "read-only Office socket",
        )

    def test_rejects_core_network_address_overrides(self):
        self.assert_rejected(
            lambda c: c["services"]["core"]["networks"]["factory-control"].__setitem__(
                "ipv4_address", "10.213.5.10"
            ),
            "address overrides are forbidden",
        )

    def test_rejects_tailscale_route_proxy_and_feature_environment(self):
        if "tailscale" not in base_config()["services"]:
            self.skipTest("optional Tailscale overlay is not present in this config")
        for name, value in (
            ("TS_ROUTES", "192.168.0.0/16"),
            ("TS_DEST_IP", "172.20.0.10"),
            ("TS_EXPERIMENTAL_DEST_DNS_NAME", "core"),
            ("TS_SOCKS5_SERVER", ":1055"),
            ("TS_OUTBOUND_HTTP_PROXY_LISTEN", ":8080"),
            ("TS_KUBE_SECRET", "factory"),
        ):
            with self.subTest(environment_key=name):
                self.assert_rejected(
                    lambda c, key=name, item=value: c["services"]["tailscale"][
                        "environment"
                    ].__setitem__(key, item),
                    "unsupported environment variable(s)",
                )

    def test_rejects_tailscale_command_dns_and_network_overrides(self):
        if "tailscale" not in base_config()["services"]:
            self.skipTest("optional Tailscale overlay is not present in this config")
        for name, value in (
            ("command", ["tailscale", "funnel", "443"]),
            ("entrypoint", ["/bin/sh", "-c"]),
            ("extra_hosts", ["core:203.0.113.9"]),
            ("dns", ["203.0.113.53"]),
            ("dns_search", ["attacker.invalid"]),
            ("links", ["core:office"]),
            ("network_mode", "service:core"),
        ):
            with self.subTest(override=name):
                self.assert_rejected(
                    lambda c, key=name, item=value: c["services"]["tailscale"].__setitem__(
                        key, item
                    ),
                    f"{name} overrides are forbidden",
                )
        self.assert_rejected(
            lambda c: c["services"]["tailscale"]["networks"].__setitem__(
                "factory-tailscale-uplink", {"aliases": ["core"]}
            ),
            "tailscale: network aliases are forbidden",
        )


if __name__ == "__main__":
    unittest.main()
