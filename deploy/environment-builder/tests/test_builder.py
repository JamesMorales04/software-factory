"""Deterministic unit tests for the containerized environment builder.

These tests exercise policy, receipt and SBOM logic without Nix, Podman or
network. Real container evidence is recorded separately and is not implied by
a green unit test.
"""

from __future__ import annotations

import io
import json
import os
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from environment_builder import BUILDER_POLICY_REVISION, LICENSE_POLICY_REVISION  # noqa: E402
import environment_builder.cli as cli  # noqa: E402
from environment_builder.cli import _resolution_file  # noqa: E402
from environment_builder.errors import BuilderError  # noqa: E402
from environment_builder.policy import (  # noqa: E402
    EnvironmentFile,
    core_entries_sha256,
    validate_check,
    validate_fetch_policy,
    validate_license_expression,
    validate_license_inventory,
    validate_network,
    validate_nix_constructs,
    validate_resources,
    validate_sandbox,
    validate_sbom_binding,
    validate_secrets,
)
from environment_builder.nix import BuildLog, closure_manifest_sha256, run_check  # noqa: E402
from environment_builder.receipt import (  # noqa: E402
    _mounts_authorized,
    build_receipt,
    limits_within_plan,
)
from environment_builder.sbom import build_sbom, license_inventory_copy  # noqa: E402

SHA = "a" * 64


def env_file(path: str, content: str) -> EnvironmentFile:
    from environment_builder.util import sha256_text

    return EnvironmentFile(path, sha256_text(content), "GENERATED", content)


def spec() -> dict:
    return {
        "schemaVersion": 1,
        "specId": "spec-1",
        "specSha256": SHA,
        "projectId": "project-1",
        "requestId": "request-1",
        "actionId": "action-1",
        "baseCommit": "b" * 40,
        "planId": "plan-1",
        "planSha256": "c" * 64,
        "conventionsSha256": "d" * 64,
        "reviewId": "review-1",
        "platform": "x86_64-linux",
        "nixVersion": "2.28.3",
        "devenvVersion": "1.11.2",
        "flakeLockSha256": "e" * 64,
        "devenvLockSha256": "",
        "licenseInventorySha256": "f" * 64,
        "sbomSha256": "1" * 64,
        "builderImage": "localhost/software-factory-environment-builder:dev",
        "builderImageDigest": "sha256:" + "2" * 64,
        "status": "FROZEN",
        "frozenAt": "2026-11-10T00:00:00Z",
    }


def license_inventory(packages: list[dict], overrides: list[dict] | None = None) -> dict:
    entries = [
        {
            "name": "nix",
            "version": "pinned by builder receipt",
            "license": "LGPL-2.1-only",
            "source": "https://github.com/NixOS/nix/blob/master/COPYING",
            "sha256": "",
            "unfree": False,
            "ownerOverrideId": None,
            "attribution": "LGPL-2.1 tool",
        },
        {
            "name": "nixpkgs",
            "version": "pinned by the environment plan",
            "license": "MIT",
            "source": "https://github.com/NixOS/nixpkgs/blob/master/COPYING",
            "sha256": "",
            "unfree": False,
            "ownerOverrideId": None,
            "attribution": "MIT",
        },
        {
            "name": "devenv",
            "version": "pinned by builder receipt",
            "license": "Apache-2.0",
            "source": "https://github.com/cachix/devenv/blob/main/LICENSE",
            "sha256": "",
            "unfree": False,
            "ownerOverrideId": None,
            "attribution": "Apache-2.0",
        },
    ]
    for package in packages:
        entries.append(
            {
                "name": package["name"],
                "version": package["version"],
                "license": package["license"],
                "source": package.get("source", "nixpkgs"),
                "sha256": package.get("sha256", ""),
                "unfree": bool(package.get("unfree", False)),
                "ownerOverrideId": "override-1" if package.get("unfree") else None,
                "attribution": package.get("attribution", ""),
            }
        )
    return {
        "schemaVersion": 1,
        "policyRevision": LICENSE_POLICY_REVISION,
        "allowUnfree": False,
        "entries": entries,
        "overrides": overrides or [],
        "blockers": [],
        "inventorySha256": "f" * 64,
        "sbomSha256": "1" * 64,
    }


class PolicyTests(unittest.TestCase):
    def test_secret_shaped_file_is_rejected(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_secrets([env_file("devenv.nix", 'env.GITHUB_TOKEN = "ghp_' + "a" * 30 + '";')], {})
        self.assertEqual(raised.exception.code, "ENVIRONMENT_SECRET_DETECTED")

    def test_builtins_exec_is_rejected(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_nix_constructs([env_file("devenv.nix", "{ builtins.exec [ \"true\" ]; }")])
        self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")

    def test_unpinned_fetch_is_rejected(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_nix_constructs(
                [env_file("flake.nix", "inputs.x.url = \"https://example.invalid/source.tar.gz\";")]
            )
        self.assertEqual(raised.exception.code, "ENVIRONMENT_PIN_MISSING")

    def test_pinned_fetch_is_allowed(self) -> None:
        validate_nix_constructs(
            [
                env_file(
                    "flake.nix",
                    "inputs.x.url = \"github:NixOS/nixpkgs/" + "a" * 40 + "\";\n"
                    "{ x = builtins.fetchTarball { url = \"https://example.invalid/x\"; sha256 = \"sha256-"
                    + "A" * 44
                    + "\"; }; }",
                )
            ]
        )

    def test_unknown_license_is_rejected(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_license_expression("NOASSERTION")
        self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_UNKNOWN")

    def test_denied_license_is_rejected(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_license_expression("AGPL-3.0-only")
        self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_DENIED")

    def test_unfree_without_override_is_rejected(self) -> None:
        package = {"name": "proprietary", "version": "1.0", "license": "MIT", "unfree": True}
        plan = {
            "packages": [package],
            "licenseInventory": license_inventory([package]),
        }
        with self.assertRaises(BuilderError) as raised:
            validate_license_inventory(spec(), plan)
        self.assertEqual(raised.exception.code, "ENVIRONMENT_UNFREE_REQUIRES_OWNER")

    def test_unfree_with_exact_override_is_allowed(self) -> None:
        package = {"name": "proprietary", "version": "1.0", "license": "MIT", "unfree": True}
        override = {
            "id": "override-1",
            "package": "proprietary",
            "version": "1.0",
            "license": "MIT",
            "planSha256": "c" * 64,
            "reason": "owner approved exact package",
            "decisionId": "decision-1",
            "expiresAt": "2030-01-01T00:00:00Z",
        }
        plan = {
            "packages": [package],
            "licenseInventory": license_inventory([package], [override]),
        }
        validated = validate_license_inventory(spec(), plan)
        self.assertEqual(validated["allowUnfree"], False)

    def test_broker_environment_egress_network_is_accepted_but_arbitrary_names_are_not(self) -> None:
        plan = {
            "network": {
                "build": "NONE",
                "fetch": "BROKER",
                "fetchAllowlist": ["https://cache.nixos.org"],
            }
        }
        request = {
            "fetchNetwork": "factory-env-egress-1",
            "fetchProxy": "http://10.91.1.1:18080",
            "fetchPolicy": {
                "network": "factory-env-egress-1",
                "proxy": "http://10.91.1.1:18080",
                "allowlist": ["https://cache.nixos.org"],
                "enforcedBy": "broker",
            },
        }
        validate_fetch_policy(request, plan, "resolve")
        for invalid in ("host", "container", "factory-env-egress-0", "factory-env-egress-999", "factory-env-egress-"):
            request["fetchNetwork"] = invalid
            with self.assertRaises(BuilderError) as raised:
                validate_fetch_policy(request, plan, "resolve")
            self.assertEqual(raised.exception.code, "ENVIRONMENT_NETWORK_DENIED")

    def test_resolution_file_is_typed_and_hash_bound(self) -> None:
        import tempfile
        from pathlib import Path as FilePath

        with tempfile.TemporaryDirectory() as directory:
            workspace = FilePath(directory)
            content = '{"nodes":{"root":{}},"root":"root","version":7}\n'
            (workspace / "flake.lock").write_text(content, encoding="utf-8")
            file = _resolution_file(workspace, "flake.lock", "PROJECT")
            self.assertEqual(file["path"], "flake.lock")
            self.assertEqual(file["origin"], "PROJECT")
            self.assertEqual(file["sha256"], __import__("hashlib").sha256(content.encode()).hexdigest())
            (workspace / "flake.lock").write_text("not json", encoding="utf-8")
            with self.assertRaises(BuilderError) as raised:
                _resolution_file(workspace, "flake.lock", "PROJECT")
            self.assertEqual(raised.exception.code, "ENVIRONMENT_PIN_MISSING")

    def test_network_build_must_be_none(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_network({"build": "BROKER", "fetch": "BROKER", "fetchAllowlist": []})
        self.assertEqual(raised.exception.code, "ENVIRONMENT_NETWORK_DENIED")

    def test_network_allowlist_rejects_wildcards(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_network({"build": "NONE", "fetch": "BROKER", "fetchAllowlist": ["https://*"]})
        self.assertEqual(raised.exception.code, "ENVIRONMENT_NETWORK_DENIED")

    def test_sandbox_must_be_enabled(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_sandbox(
                {
                    "sandbox": False,
                    "impure": False,
                    "hostMounts": [],
                    "privileged": False,
                    "hostNetwork": False,
                    "hostSockets": [],
                }
            )
        self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")

    def test_resources_are_bounded(self) -> None:
        with self.assertRaises(BuilderError) as raised:
            validate_resources(
                {"cpu": 64, "memoryMiB": 4096, "diskMiB": 10240, "pids": 512, "timeoutSecs": 1800}
            )
        self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")

    def test_check_argv_is_typed(self) -> None:
        with self.assertRaises(BuilderError):
            validate_check({"checkId": "c1", "argv": "npm test", "cwd": ".", "timeoutSecs": 60})
        parsed = validate_check(
            {"checkId": "c1", "argv": ["npm", "test"], "cwd": ".", "timeoutSecs": 60, "expectExit": 0}
        )
        self.assertEqual(parsed["argv"], ["npm", "test"])


    def test_fetch_policy_rejects_host_and_unenforced_proxy(self) -> None:
        plan = {"network": {"fetchAllowlist": ["https://cache.nixos.org"]}}
        with self.assertRaises(BuilderError) as raised:
            validate_fetch_policy({"fetchNetwork": "host"}, plan, "fetch")
        self.assertEqual(raised.exception.code, "ENVIRONMENT_NETWORK_DENIED")
        with self.assertRaises(BuilderError):
            validate_fetch_policy(
                {"fetchNetwork": "factory-egress-internal"},
                plan,
                "fetch",
            )
        with self.assertRaises(BuilderError):
            validate_fetch_policy(
                {
                    "fetchNetwork": "factory-egress-internal",
                    "fetchProxy": "http://proxy:18080",
                    "fetchPolicy": {"enforcedBy": "broker", "allowlist": ["https://other.invalid"]},
                },
                plan,
                "fetch",
            )
        validate_fetch_policy(
            {
                "fetchNetwork": "factory-egress-internal",
                "fetchProxy": "http://proxy:18080",
                "fetchPolicy": {"enforcedBy": "broker", "allowlist": ["https://cache.nixos.org"]},
            },
            plan,
            "fetch",
        )
        with self.assertRaises(BuilderError):
            validate_fetch_policy(
                {"fetchNetwork": "factory-egress-internal"},
                plan,
                "check",
            )


class ReceiptTests(unittest.TestCase):
    def test_receipt_binds_the_exact_spec_and_sandbox(self) -> None:
        receipt = build_receipt(
            spec=spec(),
            status="SUCCEEDED",
            store_path="/nix/store/" + "a" * 32 + "-devenv-shell-env",
            output_sha256="3" * 64,
            log_ref="build.log",
            attestation={
                "nonRoot": True,
                "noSecrets": True,
                "noHostNetwork": True,
                "noHostSocket": True,
                "noHostMount": True,
                "sandbox": True,
                "impure": False,
                "boundedResources": True,
            },
            blockers=[],
            duration_ms=1,
        )
        self.assertEqual(receipt["specSha256"], SHA)
        self.assertEqual(receipt["builderPolicyRevision"], BUILDER_POLICY_REVISION)
        self.assertEqual(receipt["status"], "SUCCEEDED")
        self.assertTrue(receipt["sandboxAttestation"]["sandbox"])
        self.assertFalse(receipt["sandboxAttestation"]["impure"])
        self.assertEqual(len(receipt["outputSha256"]), 64)

    def test_cgroup_limits_must_not_exceed_plan(self) -> None:
        resources = {"cpu": 2, "memoryMiB": 4096, "pids": 512}
        self.assertTrue(
            limits_within_plan(
                cpu_quota=200_000,
                cpu_period=100_000,
                memory_limit=4096 * 1024 * 1024,
                pids_limit=512,
                resources=resources,
            )
        )
        self.assertFalse(
            limits_within_plan(
                cpu_quota=800_000,
                cpu_period=100_000,
                memory_limit=4096 * 1024 * 1024,
                pids_limit=512,
                resources=resources,
            )
        )
        self.assertFalse(
            limits_within_plan(
                cpu_quota=200_000,
                cpu_period=100_000,
                memory_limit=4097 * 1024 * 1024,
                pids_limit=512,
                resources=resources,
            )
        )
        self.assertFalse(
            limits_within_plan(
                cpu_quota=200_000,
                cpu_period=100_000,
                memory_limit=4096 * 1024 * 1024,
                pids_limit=513,
                resources=resources,
            )
        )

    def test_failed_receipt_carries_a_typed_blocker(self) -> None:
        receipt = build_receipt(
            spec=spec(),
            status="FAILED",
            store_path="",
            output_sha256="",
            log_ref="build.log",
            attestation={},
            blockers=[{"code": "ENVIRONMENT_NETWORK_DENIED", "message": "offline miss"}],
            duration_ms=1,
        )
        self.assertEqual(receipt["blockers"][0]["code"], "ENVIRONMENT_NETWORK_DENIED")
        self.assertEqual(receipt["outputSha256"], "")


class SbomTests(unittest.TestCase):
    def test_sbom_is_deterministic_and_marks_transitive_entries(self) -> None:
        plan = {
            "packages": [{"name": "nodejs", "version": "22.20.0", "license": "MIT", "source": "nixpkgs"}],
            "browsers": [{"name": "chromium", "version": "1.64.1", "source": "playwright-driver"}],
            "licenseInventory": license_inventory(
                [
                    {"name": "nodejs", "version": "22.20.0", "license": "MIT", "source": "nixpkgs"},
                    {"name": "chromium", "version": "1.64.1", "license": "Apache-2.0", "source": "nixpkgs"},
                ]
            ),
        }
        manifest = [
            {"path": "/nix/store/" + "a" * 32 + "-glibc-2.39", "narHash": "sha256-x", "narSize": 1}
        ]
        first = build_sbom(spec=spec(), plan=plan, manifest=manifest, nix_version="2.28.3", devenv_version="1.11.2")
        second = build_sbom(spec=spec(), plan=plan, manifest=manifest, nix_version="2.28.3", devenv_version="1.11.2")
        self.assertEqual(first, second)
        transitive = [component for component in first["components"] if component["name"] == "glibc"]
        self.assertEqual(transitive[0]["scope"], "excluded")
        self.assertEqual(transitive[0]["licenses"], [])
        selected = [component for component in first["components"] if component["name"] == "nodejs"]
        self.assertEqual(selected[0]["licenses"][0]["license"]["id"], "MIT")

    def test_sbom_binding_requires_core_entry_hash(self) -> None:
        plan = {"licenseInventory": license_inventory([])}
        expected = core_entries_sha256(plan["licenseInventory"]["entries"])
        frozen = dict(spec())
        frozen["sbomSha256"] = expected
        plan["licenseInventory"]["sbomSha256"] = expected
        self.assertEqual(validate_sbom_binding(frozen, plan), expected)
        frozen["sbomSha256"] = "0" * 64
        with self.assertRaises(BuilderError) as raised:
            validate_sbom_binding(frozen, plan)
        self.assertEqual(raised.exception.code, "ENVIRONMENT_HASH_MISMATCH")

    def test_closure_manifest_hash_is_not_the_environment_digest(self) -> None:
        manifest = [{"path": "/nix/store/" + "a" * 32 + "-x", "narHash": "sha256-x", "narSize": 1}]
        value = closure_manifest_sha256(manifest)
        self.assertEqual(len(value), 64)
        self.assertEqual(value, closure_manifest_sha256(manifest))
        self.assertNotEqual(value, "194a9ca32c4006d453e46828512f87678f9310d2750a4903335a5367592cbf69")

    def test_inventory_copy_keeps_the_core_shape(self) -> None:
        plan = {"licenseInventory": license_inventory([])}
        copied = license_inventory_copy(plan)
        self.assertEqual(
            set(copied),
            {
                "schemaVersion",
                "policyRevision",
                "allowUnfree",
                "entries",
                "overrides",
                "blockers",
                "inventorySha256",
                "sbomSha256",
            },
        )


class AttestationTests(unittest.TestCase):
    VOLUME = "/var/lib/containers/storage/volumes/factory-env-test/_data"

    @staticmethod
    def mount(source: str, target: str) -> dict[str, str]:
        return {"source": source, "mount_point": target, "fstype": "ext4"}

    def test_expected_volume_source_is_authorized_at_each_target(self) -> None:
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertTrue(
                _mounts_authorized([self.mount(self.VOLUME, target)]), target
            )

    def test_forbidden_root_source_at_each_allowed_target_is_rejected(self) -> None:
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertFalse(_mounts_authorized([self.mount("/root", target)]), target)

    def test_host_bind_at_an_allowed_target_is_rejected(self) -> None:
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertFalse(
                _mounts_authorized([self.mount("/tmp/factory-cache", target)]), target
            )

    def test_path_traversal_volume_source_is_rejected(self) -> None:
        self.assertFalse(
            _mounts_authorized(
                [
                    self.mount(
                        "/var/lib/containers/storage/volumes/../root/_data",
                        "/cache",
                    )
                ]
            )
        )


class CheckExecutionTests(unittest.TestCase):
    class _Process:
        returncode = 101
        pid = 4242

        def __init__(self) -> None:
            self.stdout = io.BytesIO(b"compile failed\n")
            self.stderr = io.BytesIO(b"compiler error\n")

        def wait(self, timeout: int | None = None) -> int:
            return self.returncode

        def poll(self) -> int:
            return self.returncode

    def test_failed_check_preserves_exit_code_and_streams(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            cache = root / "cache"
            workspace.mkdir()
            cache.mkdir()
            log = BuildLog(cache / "check.log")
            try:
                with (
                    mock.patch(
                        "environment_builder.nix._environment_command",
                        return_value=["false"],
                    ),
                    mock.patch(
                        "environment_builder.nix.subprocess.Popen",
                        return_value=self._Process(),
                    ),
                ):
                    code, stdout, stderr, stdout_truncated, stderr_truncated = run_check(
                        workspace,
                        cache,
                        workspace,
                        ["false"],
                        timeout=30,
                        resources={
                            "cpu": 2,
                            "memoryMiB": 4096,
                            "diskMiB": 10240,
                            "pids": 512,
                            "timeoutSecs": 1800,
                        },
                        log=log,
                        expect_exit=0,
                    )
            finally:
                log.close()
        self.assertEqual(code, 101)
        self.assertEqual(stdout, "compile failed\n")
        self.assertEqual(stderr, "compiler error\n")
        self.assertFalse(stdout_truncated)
        self.assertFalse(stderr_truncated)

    def test_check_uses_the_candidate_workspace_not_the_build_cache(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            candidate = root / "candidate"
            candidate.mkdir()
            (candidate / "changed.txt").write_text("candidate\n", encoding="utf-8")
            run = root / "run"
            build_workspace = run / "workspace"
            (build_workspace / ".devenv").mkdir(parents=True)
            (build_workspace / "stale.txt").write_text("build\n", encoding="utf-8")
            output = root / "output"
            output.mkdir()
            digest = "sha256:" + "1" * 64
            spec = {
                "specSha256": SHA,
                "platform": "x86_64-linux",
                "builderImage": "localhost/builder:dev",
                "builderImageDigest": digest,
                "sbomSha256": SHA,
            }
            plan = {"network": {"fetchAllowlist": []}, "files": []}
            receipt = {
                "status": "SUCCEEDED",
                "specSha256": SHA,
                "platform": "x86_64-linux",
                "builderImage": "localhost/builder:dev",
                "builderImageDigest": digest,
                "sandboxAttestation": {
                    "nonRoot": True,
                    "noSecrets": True,
                    "noHostNetwork": True,
                    "noHostSocket": True,
                    "noHostMount": True,
                    "sandbox": True,
                    "impure": False,
                    "boundedResources": True,
                },
                "outputSha256": "5" * 64,
            }
            resources = {
                "cpu": 2,
                "memoryMiB": 4096,
                "diskMiB": 10240,
                "pids": 512,
                "timeoutSecs": 1800,
            }
            check = {
                "checkId": "candidate-visible",
                "argv": ["true"],
                "cwd": ".",
                "timeoutSecs": 30,
                "expectExit": 0,
            }
            request = {
                "spec": spec,
                "plan": plan,
                "receipt": receipt,
                "check": check,
                "runDir": str(output),
            }
            seen: dict[str, Path] = {}

            def fake_realize(workspace: Path, *_args: object, **_kwargs: object) -> str:
                seen["realize"] = workspace
                return "/nix/store/frozen-environment"

            def fake_run_check(
                workspace: Path,
                _cache: Path,
                _cwd: Path,
                _argv: list[str],
                **_kwargs: object,
            ) -> tuple[int, str, str, bool, bool]:
                seen["workspace"] = workspace
                self.assertEqual(
                    (workspace / "changed.txt").read_text(encoding="utf-8"),
                    "candidate\n",
                )
                self.assertFalse((workspace / "stale.txt").exists())
                return 0, "ok\n", "", False, False

            args = types.SimpleNamespace(
                request=str(root / "request.json"),
                workspace=str(candidate),
                output=str(output),
                check_id=None,
            )
            with (
                mock.patch.object(cli, "_load_request", return_value=request),
                mock.patch.object(
                    cli,
                    "_validate_request",
                    return_value=(spec, plan, [], resources, "2.28.3", "digest", SHA),
                ),
                mock.patch.object(
                    cli,
                    "realize_environment",
                    side_effect=fake_realize,
                ),
                mock.patch.object(cli, "closure_manifest", return_value=[]),
                mock.patch.object(
                    cli,
                    "environment_digest",
                    return_value=receipt["outputSha256"],
                ),
                mock.patch.object(cli, "run_check", side_effect=fake_run_check),
                mock.patch.dict(
                    os.environ, {"FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE": "1"}
                ),
            ):
                self.assertEqual(cli.check(args), 0)

            self.assertEqual(seen["workspace"], output / "check-workspace")
            self.assertEqual(seen["realize"], output / "check-workspace")
            self.assertTrue((build_workspace / "stale.txt").exists())
            result = json.loads(
                (output / "check-result.json").read_text(encoding="utf-8")
            )
            self.assertEqual(result["status"], "PASSED")


if __name__ == "__main__":
    unittest.main()
