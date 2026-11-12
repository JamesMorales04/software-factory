"""Deterministic unit tests for the containerized environment builder.

These tests exercise policy, receipt and SBOM logic without Nix, Podman or
network. Real container evidence is recorded separately and is not implied by
a green unit test.
"""

from __future__ import annotations

import io
import json
import os
import subprocess
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
    validate_closure_licenses,
    validate_fetch_policy,
    validate_license_expression,
    validate_license_inventory,
    validate_network,
    validate_nix_constructs,
    validate_package_source,
    validate_resources,
    validate_sandbox,
    validate_sbom_binding,
    validate_secrets,
)
from environment_builder.nix import (  # noqa: E402
    BuildLog,
    child_environment,
    closure_manifest_sha256,
    run_check,
)
from environment_builder.util import last_nonempty_line  # noqa: E402
from environment_builder.receipt import (  # noqa: E402
    _mount_attestation,
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

    def test_every_unfree_policy_form_is_rejected(self) -> None:
        for content in [
            "config.allowUnfree = true;",
            "allowUnfree = (true);",
            "allowUnfreePredicate = _: true;",
            "permittedUnfreePackages = [ \"nixpkgs.hello\" ];",
            "nixpkgs.config.allowUnfree = true;",
            "config.${builtins.getAttr (\"allow\" + \"Unfree\") { allowUnfree = true; }} = true;",
            "let allow = \"allowUn\" + \"freePredicate\"; in import nixpkgs { config = builtins.listToAttrs [ { name = allow; value = _: true; } ]; }",
        ]:
            with self.assertRaises(BuilderError) as raised:
                validate_nix_constructs([env_file("flake.nix", content)])
            self.assertIn(
                raised.exception.code,
                ("ENVIRONMENT_LICENSE_DENIED", "ENVIRONMENT_SANDBOX_DENIED"),
                content,
            )

    def test_builder_metadata_rejects_licence_or_version_lies(self) -> None:
        plan = {
            "packages": [
                {
                    "name": "ripgrep",
                    "version": "14.1.1",
                    "license": "MIT",
                    "source": "nixpkgs",
                    "attr": "ripgrep",
                    "unfree": False,
                }
            ]
        }
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        try:
            for observed, expected_code in [
                (
                    {
                        "name": "ripgrep",
                        "attr": "ripgrep",
                        "storePath": "/nix/store/" + "c" * 32 + "-ripgrep-14.1.1",
                        "version": "14.1.1",
                        "license": "Apache-2.0",
                        "unfree": False,
                    },
                    "ENVIRONMENT_LICENSE_UNKNOWN",
                ),
                (
                    {
                        "name": "ripgrep",
                        "attr": "ripgrep",
                        "storePath": "/nix/store/" + "c" * 32 + "-ripgrep-14.1.1",
                        "version": "0.0.1",
                        "license": "MIT",
                        "unfree": False,
                    },
                    "ENVIRONMENT_PIN_MISSING",
                ),
            ]:
                with mock.patch(
                    "environment_builder.cli.nix_eval_json", return_value=[observed]
                ):
                    with self.assertRaises(BuilderError) as raised:
                        cli._package_metadata(
                            "github:NixOS/nixpkgs/" + "a" * 40,
                            plan,
                            {},
                            30,
                            log,
                        )
                self.assertEqual(raised.exception.code, expected_code)
        finally:
            log.close()

    def test_internal_xgcc_exception_requires_exact_identity(self) -> None:
        pin = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d"
        real_path = "/nix/store/16hvpw4b3r05girazh4rnwbw0jgjkb4l-xgcc-14.3.0-libgcc"
        real_driver = "/nix/store/p8vpplfq13sjvxxvcfswp8p2rajji7yh-xgcc-14.3.0.drv"
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        try:
            with mock.patch("environment_builder.cli.nix_eval_json", return_value=[]):
                entries = cli._closure_license_entries(
                    {"nix": {"nixpkgs": pin}},
                    [{"path": real_path, "deriver": real_driver}],
                    {},
                    30,
                    log,
                )
            self.assertEqual(entries[0]["name"], "xgcc")
            self.assertEqual(entries[0]["license"], "GPL-3.0-or-later")

            fabricated = "/nix/store/" + "f" * 32 + "-xgcc-1.0"
            with mock.patch("environment_builder.cli.nix_eval_json", return_value=[]):
                with self.assertRaises(BuilderError) as raised:
                    cli._closure_license_entries(
                        {"nix": {"nixpkgs": pin}},
                        [{"path": fabricated, "deriver": "/nix/store/" + "e" * 32 + "-xgcc-1.0.drv"}],
                        {},
                        30,
                        log,
                    )
            self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_UNKNOWN")
        finally:
            log.close()

    def test_committed_environment_uses_the_devenv_package_profile(self) -> None:
        workspace = Path(tempfile.mkdtemp())
        (workspace / ".devenv").mkdir()
        target = "/nix/store/" + "a" * 32 + "-devenv-profile"
        (workspace / ".devenv" / "profile").symlink_to(target)
        self.assertEqual(
            cli._environment_profile(workspace, workspace / "run"), target
        )

    def test_committed_substrate_closure_entries_require_exact_reviewed_identities(self) -> None:
        pin = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d"
        reviewed = [
            (
                "/nix/store/aiijyyiz8mrml76mr1kvx0r62aw8ccbn-devenv-1.11.2.drv",
                "/nix/store/mxi4r8qi2y79pybxyfzlqg5i8hl5a910-devenv-1.11.2",
                "devenv",
                "Apache-2.0",
            ),
            (
                "/nix/store/dzwdvpqgiv9ja73r6pfnkf4gdjk34x3n-nix-2.28.5.drv",
                "/nix/store/mxafjxh0amr24d2gb7n3km6hljj79qsj-nix-2.28.5",
                "nix",
                "LGPL-2.1-only",
            ),
            (
                "/nix/store/10jdyhddq82xfyfyrid403bdfsyq2l00-githooks-3.0.4.drv",
                "/nix/store/7s93k2darc8ba3n36j17dafzw7z165ck-githooks-3.0.4",
                "githooks",
                "MIT",
            ),
            ("", "/nix/store/3p306srz83h9z9v0ma9xcxb8y8cdxkxj-source", "nixpkgs", "MIT"),
            (
                "",
                "/nix/store/q8g9dljvn01w9fn5ih0yam13z96lxfa2-source",
                "git-hooks.nix",
                "MIT",
            ),
        ]
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        try:
            for deriver, path, name, license_value in reviewed:
                with mock.patch(
                    "environment_builder.cli.nix_eval_json", return_value=[]
                ):
                    entries = cli._closure_license_entries(
                        {"nix": {"nixpkgs": pin}},
                        [{"path": path, "deriver": deriver}],
                        {},
                        30,
                        log,
                    )
                self.assertEqual(entries[0]["name"], name)
                self.assertEqual(entries[0]["license"], license_value)

            for fabricate in [
                "/nix/store/" + "f" * 32 + "-devenv-1.0",
                "/nix/store/" + "f" * 32 + "-nix-1.0",
                "/nix/store/" + "f" * 32 + "-nixpkgs-1.0",
                "/nix/store/" + "f" * 32 + "-git-hooks-1.0",
            ]:
                with mock.patch(
                    "environment_builder.cli.nix_eval_json", return_value=[]
                ):
                    with self.assertRaises(BuilderError) as raised:
                        cli._closure_license_entries(
                            {"nix": {"nixpkgs": pin}},
                            [{"path": fabricate, "deriver": "/nix/store/fabricated.drv"}],
                            {},
                            30,
                            log,
                        )
                self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_UNKNOWN")
        finally:
            log.close()

    def test_fabricated_sh_package_with_deriver_fails_closed(self) -> None:
        pin = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d"
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        try:
            with mock.patch(
                "environment_builder.cli.nix_eval_json", return_value=[]
            ) as evaluate:
                with self.assertRaises(BuilderError) as raised:
                    cli._closure_license_entries(
                        {"nix": {"nixpkgs": pin}},
                        [
                            {
                                "path": "/nix/store/" + "e" * 32 + "-concealed-package.sh",
                                "deriver": "/nix/store/concealed-package.drv",
                            }
                        ],
                        {},
                        30,
                        log,
                    )
            self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_UNKNOWN")
            evaluate.assert_called_once()
        finally:
            log.close()

    def test_strip_sh_helper_without_deriver_is_not_a_package(self) -> None:
        pin = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d"
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        try:
            with mock.patch(
                "environment_builder.cli.nix_eval_json", return_value=[]
            ) as evaluate:
                entries = cli._closure_license_entries(
                    {"nix": {"nixpkgs": pin}},
                    [
                        {
                            "path": "/nix/store/pilsssjjdxvdphlg2h19p0bfx5q0jzkn-strip.sh",
                            "deriver": None,
                        }
                    ],
                    {},
                    30,
                    log,
                )
            self.assertEqual(entries, [])
            evaluate.assert_not_called()
        finally:
            log.close()

    def test_unreviewed_deriverless_closure_path_fails_closed(self) -> None:
        pin = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d"
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        try:
            with mock.patch(
                "environment_builder.cli.nix_eval_json", return_value=[]
            ) as evaluate:
                with self.assertRaises(BuilderError) as raised:
                    cli._closure_license_entries(
                        {"nix": {"nixpkgs": pin}},
                        [
                            {
                                "path": "/nix/store/" + "f" * 32 + "-unknown-source",
                                "deriver": None,
                            }
                        ],
                        {},
                        30,
                        log,
                    )
            self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_UNKNOWN")
            evaluate.assert_not_called()
        finally:
            log.close()

    def test_closure_audit_excludes_only_the_exact_devshell_wrapper(self) -> None:
        root = "/nix/store/" + "a" * 32 + "-devshell"
        package = "/nix/store/" + "b" * 32 + "-bash-5.2"
        unknown = "/nix/store/" + "c" * 32 + "-mystery-1.0"
        manifest = [
            {"path": root, "deriver": "/nix/store/root.drv"},
            {"path": package, "deriver": "/nix/store/bash.drv"},
        ]
        plan = {"nix": {"nixpkgs": "github:NixOS/nixpkgs/" + "d" * 40}}
        workspace = Path(tempfile.mkdtemp())
        log = BuildLog(workspace / "resolve.log")
        observed = {
            "path": package,
            "name": "bash",
            "version": "5.2",
            "license": "GPL-3.0-or-later",
            "unfree": False,
        }
        try:
            with mock.patch(
                "environment_builder.cli.nix_eval_json", return_value=[observed]
            ):
                entries = cli._closure_license_entries(
                    plan, manifest, {}, 30, log, exclude={root}
                )
            self.assertEqual([entry["path"] for entry in entries], [package])

            # An arbitrary unknown package output is not silently excluded.
            manifest.append({"path": unknown, "deriver": "/nix/store/mystery.drv"})
            with mock.patch(
                "environment_builder.cli.nix_eval_json", return_value=[observed]
            ):
                with self.assertRaises(BuilderError) as raised:
                    cli._closure_license_entries(
                        plan, manifest, {}, 30, log, exclude={root}
                    )
            self.assertEqual(raised.exception.code, "ENVIRONMENT_LICENSE_UNKNOWN")
        finally:
            log.close()

    def test_committed_dynamic_config_ast_is_rejected(self) -> None:
        from environment_builder.util import sha256_text

        # Normalised `nix-instantiate --parse` ASTs: direct, merged, long
        # whitespace and a nested `let` between the merge and the dynamic key.
        rejected_asts = [
            '{ outputs = ({ nixpkgs }: import nixpkgs { config = { "${key}" = (_: true); }; }); }',
            '{ outputs = ({ nixpkgs }: import nixpkgs { config = ({ } // { "${key}" = (_: true); }); }); }',
            '{ outputs = ({ nixpkgs }: import nixpkgs { config = ({ } // (let x = 1; in { "${key}" = (_: true); })); }); }',
            '{ outputs = ({ nixpkgs }: let settings = { "${allow}" = (_: true); }; in import nixpkgs { config = settings; }); }',
        ]
        from environment_builder.policy import (
            reject_computed_nixpkgs_import,
            reject_dynamic_config_ast,
        )

        for ast in rejected_asts:
            with self.assertRaises(BuilderError) as raised:
                reject_dynamic_config_ast(ast, "flake.nix")
            self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")

        rejected_import_asts = [
            '{ outputs = ({ nixpkgs }: let configKey = ("con" + "fig"); settings = { "${allow}" = (_: true); }; in (import nixpkgs { "${configKey}" = settings; })); }',
            '{ outputs = ({ nixpkgs }: let args = { config = { allowUnfree = false; }; }; in (import nixpkgs args)); }',
            '{ outputs = ({ nixpkgs }: import nixpkgs ({} // { "${key}" = (_: true); })); }',
            '{ outputs = ({ nixpkgs }: let f = import; in (f nixpkgs { config = { allowUnfree = false; }; })); }',
        ]
        for ast in rejected_import_asts:
            with self.assertRaises(BuilderError) as raised:
                reject_computed_nixpkgs_import(ast, "flake.nix")
            self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")

        accepted_import_asts = [
            '{ outputs = ({ nixpkgs }: import nixpkgs { config = { allowUnfree = false; }; }); }',
            '{ outputs = ({ nixpkgs }: import nixpkgs { overlays = [ ]; }); }',
            '{ outputs = ({ nixpkgs }: let system = "x86_64-linux"; pkgs = import nixpkgs { inherit system; }; in { devShells = { "${system}" = { default = (pkgs.mkShell { }); }; }; }); }',
        ]
        for ast in accepted_import_asts:
            reject_dynamic_config_ast(ast, "flake.nix")
            reject_computed_nixpkgs_import(ast, "flake.nix")

        conventional_ast = (
            '{ outputs = ({ nixpkgs }: let system = "x86_64-linux"; '
            'pkgs = import nixpkgs { inherit system; }; in '
            '{ devShells = { "${system}" = { default = (pkgs.mkShell { }); }; }; }); }'
        )
        reject_dynamic_config_ast(conventional_ast, "flake.nix")
        reject_computed_nixpkgs_import(conventional_ast, "flake.nix")
        reject_dynamic_config_ast(
            '{ outputs = ({ nixpkgs }: import nixpkgs { config = { allowUnfree = false; }; }); }',
            "flake.nix",
        )

        raw = (
            "let key = \"allowUn\" + \"freePredicate\"; "
            "in import nixpkgs { config = {} // "
            + " " * 4000
            + " (let x = 1; in { ${key} = _: true; }); }"
        )
        file = EnvironmentFile("flake.nix", sha256_text(raw), "PROJECT", raw)
        with mock.patch(
            "environment_builder.cli.nix_parse", return_value=rejected_asts[2]
        ) as parse:
            with self.assertRaises(BuilderError) as raised:
                cli.validate_nix_config_ast([file])
        self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")
        parse.assert_called_once()

        alias_raw = (
            'let allow = "allowUn" + "freePredicate"; '
            'settings = { ${allow} = _: true; }; '
            'in import nixpkgs { config = settings; }'
        )
        alias_file = EnvironmentFile(
            "flake.nix", sha256_text(alias_raw), "PROJECT", alias_raw
        )
        with mock.patch(
            "environment_builder.cli.nix_parse", return_value=rejected_asts[3]
        ):
            with self.assertRaises(BuilderError) as raised:
                cli.validate_nix_config_ast([alias_file])
        self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")

        computed_key_raw = (
            'let configKey = "con" + "fig"; '
            'allow = "allowUn" + "freePredicate"; '
            'settings = { ${allow} = _: true; }; '
            'in import nixpkgs { ${configKey} = settings; }'
        )
        computed_file = EnvironmentFile(
            "flake.nix", sha256_text(computed_key_raw), "PROJECT", computed_key_raw
        )
        with mock.patch(
            "environment_builder.cli.nix_parse",
            return_value=rejected_import_asts[0],
        ):
            with self.assertRaises(BuilderError) as raised:
                cli.validate_nix_config_ast([computed_file])
        self.assertEqual(raised.exception.code, "ENVIRONMENT_SANDBOX_DENIED")

        conventional = (
            "outputs = { self, nixpkgs }: "
            "let system = \"x86_64-linux\"; in "
            "{ devShells.${system}.default = nixpkgs.legacyPackages.${system}.mkShell { }; };"
        )
        validate_nix_constructs(
            [EnvironmentFile("flake.nix", sha256_text(conventional), "PROJECT", conventional)]
        )

    def test_command_check_rejects_shell_builtins_and_accepts_real_binary(self) -> None:
        script = (
            'p=$(command -v "$1") || exit 1; '
            'case "$p" in /*) ;; *) exit 1;; esac; '
            'test -x "$p" && printf "%s" "$p"'
        )
        for executable, expected in [("source", False), ("sh", True)]:
            result = subprocess.run(
                ["sh", "-c", script, "sh", executable],
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode == 0, expected, executable)

    def test_package_metadata_uses_the_plan_pin_not_a_workspace_flake(self) -> None:
        pin = "github:NixOS/nixpkgs/" + "a" * 40
        expression = cli._package_metadata_expression(
            pin, [{"name": "ripgrep", "attr": "ripgrep"}]
        )
        self.assertIn(pin, expression)
        self.assertNotIn("path:", expression)
        closure = cli._closure_metadata_expression(
            pin, ["/nix/store/" + "b" * 32 + "-bash-5.2"]
        )
        self.assertIn(pin, closure)
        self.assertIn("/nix/store/" + "b" * 32 + "-bash-5.2", closure)

    def test_closure_license_policy_rejects_unfree_denied_and_unknown(self) -> None:
        base = {
            "path": "/nix/store/" + "a" * 32 + "-bash-5.2",
            "name": "bash",
            "version": "5.2",
            "license": "GPL-3.0-or-later",
            "unfree": False,
        }
        self.assertEqual(validate_closure_licenses([base])[0]["name"], "bash")
        for change, code in [
            ({"unfree": True}, "ENVIRONMENT_UNFREE_REQUIRES_OWNER"),
            ({"license": "AGPL-3.0-only"}, "ENVIRONMENT_LICENSE_DENIED"),
            ({"license": "NOASSERTION"}, "ENVIRONMENT_LICENSE_UNKNOWN"),
            ({"license": "Unknown-Proprietary"}, "ENVIRONMENT_LICENSE_UNKNOWN"),
        ]:
            entry = {**base, **change}
            with self.assertRaises(BuilderError) as raised:
                validate_closure_licenses([entry])
            self.assertEqual(raised.exception.code, code, change)

    def test_missing_command_executable_is_rejected(self) -> None:
        plan = {"commands": {"test": {"argv": ["cargo", "test"]}}}
        with mock.patch(
            "environment_builder.cli.check_command_available",
            return_value=(False, "not found"),
        ):
            with self.assertRaises(BuilderError) as raised:
                cli._verify_command_executables(
                    Path("/tmp"),
                    Path("/tmp"),
                    plan,
                    offline=True,
                    resources={"timeoutSecs": 30},
                    timeout=30,
                    proxy=None,
                )
        self.assertEqual(raised.exception.code, "ENVIRONMENT_PLAN_INVALID")
        with mock.patch(
            "environment_builder.cli.check_command_available",
            return_value=(True, "/nix/store/cargo"),
        ):
            cli._verify_command_executables(
                Path("/tmp"),
                Path("/tmp"),
                plan,
                offline=True,
                resources={"timeoutSecs": 30},
                timeout=30,
                proxy=None,
            )

    def test_package_source_must_name_the_exact_validated_attribute(self) -> None:
        validate_package_source(
            {"name": "nodejs", "attr": "nodejs_22", "source": "nixpkgs:nodejs_22"}
        )
        validate_package_source({"name": "ripgrep", "source": "nixpkgs"})
        for package in [
            {"name": "nodejs", "attr": "nodejs_22", "source": "nixpkgs:other"},
            {"name": "nodejs", "attr": "nodejs_22", "source": "cargo"},
            {"name": "nodejs", "attr": "bad attr", "source": "nixpkgs"},
        ]:
            with self.assertRaises(BuilderError):
                validate_package_source(package)

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
        package = {
            "name": "proprietary",
            "version": "1.0",
            "license": "MIT",
            "source": "nixpkgs",
            "unfree": True,
        }
        plan = {
            "packages": [package],
            "licenseInventory": license_inventory([package]),
        }
        with self.assertRaises(BuilderError) as raised:
            validate_license_inventory(spec(), plan)
        self.assertEqual(raised.exception.code, "ENVIRONMENT_UNFREE_REQUIRES_OWNER")

    def test_unfree_with_exact_override_is_allowed(self) -> None:
        package = {
            "name": "proprietary",
            "version": "1.0",
            "license": "MIT",
            "source": "nixpkgs",
            "unfree": True,
        }
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
            validate_check(
                {
                    "checkId": "c1",
                    "description": "run the tests",
                    "argv": "npm test",
                    "cwd": ".",
                    "timeoutSecs": 60,
                }
            )
        parsed = validate_check(
            {
                "checkId": "c1",
                "description": "run the tests",
                "argv": ["npm", "test"],
                "cwd": ".",
                "timeoutSecs": 60,
                "expectExit": 0,
            }
        )
        self.assertEqual(parsed["argv"], ["npm", "test"])
        self.assertEqual(parsed["description"], "run the tests")
        with self.assertRaises(BuilderError):
            validate_check(
                {
                    "checkId": "c1",
                    "description": "x" * 501,
                    "argv": ["npm", "test"],
                    "cwd": ".",
                    "timeoutSecs": 60,
                }
            )
        with self.assertRaises(BuilderError):
            validate_check(
                {
                    "checkId": "c1",
                    "description": "   ",
                    "argv": ["npm", "test"],
                    "cwd": ".",
                    "timeoutSecs": 60,
                }
            )


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
    def mount(
        source: str,
        target: str,
        fstype: str = "ext4",
        root: str = "",
        mount_id: int = 100,
        parent_id: int = 1,
    ) -> dict[str, int | str]:
        return {
            "mount_id": mount_id,
            "parent_id": parent_id,
            "root": root,
            "source": source,
            "mount_point": target,
            "fstype": fstype,
        }

    @staticmethod
    def nested_engine_root() -> str:
        return (
            "/@home/dev/.local/share/containers/storage/volumes/engine/_data"
            "/storage/"
        )

    def test_expected_volume_source_is_authorized_at_each_target(self) -> None:
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertTrue(
                _mounts_authorized([self.mount(self.VOLUME, target)]), target
            )

    def test_safe_root_and_system_sources_are_authorized(self) -> None:
        mounts = [
            self.mount("overlay", "/", "overlay"),
            self.mount("proc", "/proc", "proc"),
            self.mount("sysfs", "/sys", "sysfs"),
            self.mount("cgroup2", "/sys/fs/cgroup", "cgroup2"),
            self.mount("devtmpfs", "/dev", "devtmpfs"),
            self.mount("devpts", "/dev/pts", "devpts"),
            self.mount("shm", "/dev/shm", "tmpfs"),
            self.mount("mqueue", "/dev/mqueue", "mqueue"),
            self.mount("tmpfs", "/run", "tmpfs"),
            self.mount("tmpfs", "/tmp", "tmpfs"),
            self.mount("tmpfs", "/home/factory", "tmpfs"),
            self.mount(
                "/var/lib/containers/storage/overlay-containers/abc/userdata/hosts",
                "/etc/hosts",
                "ext4",
            ),
            self.mount(
                "/var/lib/containers/storage/overlay-containers/abc/userdata/hostname",
                "/etc/hostname",
                "ext4",
            ),
            self.mount(
                "/var/lib/containers/storage/overlay-containers/abc/userdata/resolv.conf",
                "/etc/resolv.conf",
                "ext4",
            ),
        ]
        self.assertTrue(_mounts_authorized(mounts))
        self.assertTrue(_mount_attestation(mounts, True))

    def test_container_runtime_file_source_must_match_target(self) -> None:
        prefix = "/var/lib/containers/storage/overlay-containers/abc/userdata"
        self.assertFalse(
            _mounts_authorized([self.mount(f"{prefix}/config.json", "/etc/hosts")])
        )
        self.assertFalse(
            _mounts_authorized([self.mount(f"{prefix}/hostname", "/etc/hosts")])
        )
        self.assertFalse(
            _mounts_authorized([self.mount("/etc/hosts", "/etc/hosts", "ext4")])
        )

    def test_container_runtime_file_source_traversal_is_rejected(self) -> None:
        traversal = (
            "/var/lib/containers/storage/overlay-containers/"
            "../../../../../root/userdata/hosts"
        )
        self.assertFalse(
            _mounts_authorized([self.mount(traversal, "/etc/hosts")])
        )
        self.assertFalse(
            _mounts_authorized(
                [
                    self.mount(
                        "/var/lib/containers/storage/overlay-containers/abc/../userdata/hosts",
                        "/etc/hosts",
                    )
                ]
            )
        )

    def nested_mounts(self, *extra: dict[str, int | str]) -> list[dict[str, int | str]]:
        engine = self.nested_engine_root()
        root = self.mount(
            "/dev/nvme0n1p2", "/", "btrfs", f"{engine}vfs/dir/abc123",
            mount_id=2145, parent_id=2100,
        )
        return [root, *extra]

    def test_nested_vfs_named_volume_root_is_authorized(self) -> None:
        engine = self.nested_engine_root()
        root = f"{engine}volumes/factory-env-test/_data"
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertTrue(
                _mounts_authorized(
                    self.nested_mounts(
                        self.mount(
                            "/dev/nvme0n1p2", target, "btrfs", root,
                            mount_id=2002, parent_id=2145,
                        )
                    )
                ),
                target,
            )
        self.assertFalse(
            _mounts_authorized(
                self.nested_mounts(
                    self.mount(
                        "/dev/nvme0n1p2", "/cache", "btrfs",
                        f"{engine}volumes/../root/_data",
                        mount_id=2002, parent_id=2145,
                    )
                )
            )
        )
        self.assertFalse(
            _mounts_authorized(
                self.nested_mounts(
                    self.mount(
                        "/dev/nvme0n1p2", "/cache", "btrfs", root,
                        mount_id=2002, parent_id=9999,
                    )
                )
            )
        )
        self.assertFalse(
            _mounts_authorized(
                self.nested_mounts(
                    self.mount(
                        "/tmp/factory-cache", "/cache", "btrfs", root,
                        mount_id=2002, parent_id=2145,
                    )
                )
            )
        )

    def test_shaped_host_binds_are_rejected(self) -> None:
        # A host directory that merely looks like a nested volume root is not
        # tied to the observed engine storage prefix and parent root mount.
        shaped_volume = "/host/export/storage/volumes/attacker/_data"
        self.assertFalse(
            _mounts_authorized(
                [self.mount("/dev/hostdisk", "/cache", "btrfs", shaped_volume)]
            )
        )
        shaped_root = "/host/export/storage/vfs/unrelated"
        self.assertFalse(
            _mounts_authorized(
                [self.mount("/dev/hostdisk", "/", "btrfs", shaped_root)]
            )
        )
        # A valid root mount does not authorize a volume outside its storage
        # prefix or a runtime file with an extra host prefix.
        self.assertFalse(
            _mounts_authorized(
                self.nested_mounts(
                    self.mount(
                        "/dev/hostdisk", "/cache", "btrfs", shaped_volume,
                        mount_id=2002, parent_id=2145,
                    )
                )
            )
        )
        self.assertFalse(
            _mounts_authorized(
                self.nested_mounts(
                    self.mount(
                        "tmpfs", "/etc/hosts", "tmpfs",
                        "/host/containers/storage/vfs-containers/abc/userdata/hosts",
                        mount_id=2084, parent_id=2145,
                    )
                )
            )
        )

    def test_nested_vfs_runtime_file_root_is_authorized(self) -> None:
        for target, name in (
            ("/etc/hosts", "hosts"),
            ("/etc/hostname", "hostname"),
            ("/etc/resolv.conf", "resolv.conf"),
        ):
            root = f"/containers/storage/vfs-containers/abc123/userdata/{name}"
            self.assertTrue(
                _mounts_authorized(
                    self.nested_mounts(
                        self.mount(
                            "tmpfs", target, "tmpfs", root,
                            mount_id=2084, parent_id=2145,
                        )
                    )
                ),
                target,
            )
            self.assertFalse(
                _mounts_authorized(
                    self.nested_mounts(
                        self.mount(
                            "tmpfs", target, "tmpfs",
                            "/containers/storage/vfs-containers/abc123/userdata/config.json",
                            mount_id=2084, parent_id=2145,
                        )
                    )
                ),
                target,
            )
            self.assertFalse(
                _mounts_authorized(
                    self.nested_mounts(
                        self.mount(
                            "/tmp/hosts", target, "tmpfs", root,
                            mount_id=2084, parent_id=2145,
                        )
                    )
                ),
                target,
            )

    def test_nested_vfs_root_filesystem_is_authorized(self) -> None:
        engine = self.nested_engine_root()
        root = f"{engine}vfs/dir/abc123"
        self.assertTrue(
            _mounts_authorized(
                self.nested_mounts()
            )
        )
        self.assertFalse(
            _mounts_authorized(
                [self.mount("/tmp/root", "/", "btrfs", root)]
            )
        )
        self.assertFalse(
            _mounts_authorized(
                [self.mount("/dev/nvme0n1p2", "/", "btrfs", "/")]
            )
        )

    def test_missing_or_incomplete_mountinfo_is_not_attested(self) -> None:
        self.assertFalse(_mount_attestation([], False))
        self.assertFalse(_mount_attestation([], True))
        self.assertFalse(
            _mount_attestation([self.mount("overlay", "/", "overlay")], False)
        )

    def test_overlay_at_proc_is_rejected(self) -> None:
        self.assertFalse(
            _mounts_authorized([self.mount("overlay", "/proc", "overlay")])
        )
        self.assertFalse(
            _mounts_authorized([self.mount("overlay", "/proc/sys", "overlay")])
        )

    def test_forbidden_root_source_at_each_allowed_target_is_rejected(self) -> None:
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertFalse(_mounts_authorized([self.mount("/root", target)]), target)

    def test_forbidden_root_source_at_root_and_system_targets_is_rejected(self) -> None:
        for target, fstype in (
            ("/", "ext4"),
            ("/proc", "proc"),
            ("/sys", "sysfs"),
            ("/dev", "devtmpfs"),
            ("/run", "tmpfs"),
            ("/tmp", "tmpfs"),
            ("/home/factory", "tmpfs"),
        ):
            self.assertFalse(
                _mounts_authorized([self.mount("/root", target, fstype)]), target
            )

    def test_host_bind_at_an_allowed_target_is_rejected(self) -> None:
        for target in ("/inputs/workspace", "/cache", "/nix"):
            self.assertFalse(
                _mounts_authorized([self.mount("/tmp/factory-cache", target)]), target
            )
        self.assertFalse(_mounts_authorized([self.mount("/tmp/factory", "/", "ext4")]))
        self.assertFalse(
            _mounts_authorized([self.mount("/tmp/factory", "/proc", "proc")])
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

    def test_named_volume_at_root_or_system_target_is_rejected(self) -> None:
        self.assertFalse(_mounts_authorized([self.mount(self.VOLUME, "/")]))
        self.assertFalse(_mounts_authorized([self.mount(self.VOLUME, "/proc", "proc")]))


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

    def test_check_runs_in_the_requested_working_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            subdir = workspace / "projects" / "node"
            cache = root / "cache"
            subdir.mkdir(parents=True)
            cache.mkdir()
            seen: dict[str, list[str]] = {}
            log = BuildLog(cache / "check.log")
            try:
                def fake_command(
                    _workspace: Path, argv: list[str], *, offline: bool
                ) -> list[str]:
                    self.assertTrue(offline)
                    seen["argv"] = argv
                    return ["true"]

                with (
                    mock.patch(
                        "environment_builder.nix._environment_command",
                        side_effect=fake_command,
                    ),
                    mock.patch(
                        "environment_builder.nix.subprocess.Popen",
                        return_value=self._Process(),
                    ),
                ):
                    run_check(
                        workspace,
                        cache,
                        subdir,
                        ["npm", "test"],
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
            self.assertEqual(
                seen["argv"], ["env", "-C", str(subdir), "npm", "test"]
            )

    def test_check_timeout_is_a_typed_builder_error(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            cache = root / "cache"
            workspace.mkdir()
            cache.mkdir()
            log = BuildLog(cache / "check.log")
            try:
                process = self._Process()

                def timeout(*_args: object, **_kwargs: object) -> int:
                    raise subprocess.TimeoutExpired("npm", 1)

                process.wait = timeout  # type: ignore[method-assign]
                with (
                    mock.patch(
                        "environment_builder.nix._environment_command",
                        return_value=["npm", "test"],
                    ),
                    mock.patch(
                        "environment_builder.nix.subprocess.Popen",
                        return_value=process,
                    ),
                    mock.patch("environment_builder.nix._terminate_process"),
                ):
                    with self.assertRaises(BuilderError) as raised:
                        run_check(
                            workspace,
                            cache,
                            workspace,
                            ["npm", "test"],
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
            self.assertEqual(raised.exception.code, "ENVIRONMENT_BUILD_TIMEOUT")

    def test_offline_check_sets_the_npm_offline_cache(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            cache = root / "cache"
            workspace.mkdir()
            cache.mkdir()
            resources = {
                "cpu": 2,
                "memoryMiB": 4096,
                "diskMiB": 10240,
                "pids": 512,
                "timeoutSecs": 1800,
            }
            offline = child_environment(
                workspace, cache, offline=True, resources=resources
            )
            self.assertEqual(offline["npm_config_offline"], "true")
            self.assertEqual(offline["npm_config_cache"], str(cache / "npm"))
            self.assertEqual(offline["HOME"], str(cache / "home"))
            self.assertIn("substituters =\n", offline["NIX_CONFIG"])
            online = child_environment(
                workspace, cache, offline=False, resources=resources
            )
            self.assertEqual(online["npm_config_offline"], "false")

    def test_check_stderr_tail_is_one_bounded_redacted_line(self) -> None:
        self.assertEqual(
            last_nonempty_line("first\n\n  final line  \n"), "final line"
        )
        self.assertEqual(last_nonempty_line(""), "")
        self.assertEqual(last_nonempty_line("x" * 4_000), "x" * 512)
        redacted = last_nonempty_line("token = abcdefghijklmnop")
        self.assertNotIn("abcdefghijklmnop", redacted)

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
            nix_store = root / "nix"
            nix_store.mkdir()
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
                "description": "the check sees the candidate workspace",
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
                    os.environ,
                    {
                        "FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE": "1",
                        "FACTORY_ENVIRONMENT_NIX_STORE": str(nix_store),
                    },
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
            self.assertEqual(
                result["description"], "the check sees the candidate workspace"
            )
            self.assertEqual(result["stderrTail"], "")


if __name__ == "__main__":
    unittest.main()
