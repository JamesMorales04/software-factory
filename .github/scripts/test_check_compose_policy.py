#!/usr/bin/env python3
"""Negative tests for the static Compose security policy."""
import copy
import json
import os
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
            "only core may publish a port",
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


if __name__ == "__main__":
    unittest.main()
