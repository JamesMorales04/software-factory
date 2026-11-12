"""Wrapper-level tests using a fake rootless Podman executable.

These exercise the real shell wrapper without a container engine: a fake
``podman`` can return success/rootless metadata, write SUCCEEDED/PASSED
artifacts into the mounted cache, then exit non-zero. The wrapper must purge
those artifacts and return a typed failure.
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
WRAPPER = ROOT / "scripts" / "environment-builder.sh"
REQUEST = ROOT / "docs" / "evidence" / "environment-builder-2026-11-10" / "spec-plan-request.json"

FAKE_PODMAN = """#!/usr/bin/env python3
import os, sys
from pathlib import Path

args = sys.argv[1:]
rootless = os.environ.get("FAKE_ROOTLESS", "true")
if args[:1] == ["info"]:
    print(rootless)
    raise SystemExit(0)
if args[:1] == ["image"]:
    sub = args[1] if len(args) > 1 else ""
    if sub == "exists":
        raise SystemExit(0)
    if sub == "inspect":
        fmt = " ".join(args)
        print("sha256:" + ("a" * 64) if "{{.Id}}" in fmt else "sha256:" + ("b" * 64))
        raise SystemExit(0)
if args[:1] == ["network"]:
    print("true")
    raise SystemExit(0)
if args[:1] == ["run"]:
    marker = os.environ.get("FAKE_RUN_MARKER")
    if marker:
        Path(marker).write_text("run", encoding="utf-8")
    source = None
    for index, value in enumerate(args):
        if value == "--mount" and index + 1 < len(args):
            mount = args[index + 1]
            if "dst=/cache" in mount:
                for part in mount.split(","):
                    if part.startswith("src="):
                        source = part[4:]
    if source:
        target = Path(source)
        target.mkdir(parents=True, exist_ok=True)
        if os.environ.get("FAKE_OVER_BUDGET") == "1":
            (target / "large.bin").write_bytes(b"x" * (2 * 1024 * 1024))
        (target / "result.json").write_text('{"status":"SUCCEEDED"}', encoding="utf-8")
        (target / "builder-evidence.json").write_text('{"status":"SUCCEEDED"}', encoding="utf-8")
        (target / "check-result.json").write_text('{"status":"PASSED"}', encoding="utf-8")
        (target / "sbom.cdx.json").write_text("{}", encoding="utf-8")
        (target / "closure.json").write_text("{}", encoding="utf-8")
        (target / "license-inventory.json").write_text("{}", encoding="utf-8")
    raise SystemExit(int(os.environ.get("FAKE_RUN_EXIT", "1")))
raise SystemExit(0)
"""


class WrapperTests(unittest.TestCase):
    def _run(
        self,
        rootless: str = "true",
        *,
        over_budget: bool = False,
        disk_mib: int = 10240,
    ) -> tuple[subprocess.CompletedProcess[str], Path, Path, Path, Path]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        base = Path(temporary.name)
        cache_base = base / "cache-base"
        cache = cache_base / "cache"
        store = cache / "nix"
        run_dir = cache / "runs" / "run-1"
        output = base / "output"
        workspace = base / "workspace"
        fake_bin = base / "bin"
        for directory in (cache_base, store, run_dir, output, workspace, fake_bin):
            directory.mkdir(parents=True, exist_ok=True)
        (fake_bin / "podman").write_text(FAKE_PODMAN, encoding="utf-8")
        (fake_bin / "podman").chmod(0o755)
        (store / ".seeded").write_text("seeded", encoding="utf-8")
        (workspace / "README").write_text("fixture", encoding="utf-8")
        request_path = REQUEST
        if disk_mib != 10240:
            request = json.loads(REQUEST.read_text(encoding="utf-8"))
            request["plan"]["resources"]["diskMiB"] = disk_mib
            request["spec"]["resourcePolicy"]["diskMiB"] = disk_mib
            request_path = base / "request.json"
            request_path.write_text(json.dumps(request), encoding="utf-8")
        marker = base / "podman-run-marker"
        environment = {
            **os.environ,
            "PATH": f"{fake_bin}:{os.environ['PATH']}",
            "FAKE_ROOTLESS": rootless,
            "FAKE_RUN_EXIT": "1",
            "FAKE_RUN_MARKER": str(marker),
            "FAKE_OVER_BUDGET": "1" if over_budget else "0",
            "FACTORY_ENVIRONMENT_BUILDER_IMAGE": "localhost/test-builder:test",
            "FACTORY_ENVIRONMENT_BUILDER_DIGEST": "sha256:" + ("a" * 64),
            "FACTORY_ENVIRONMENT_BUILDER_CACHE_BASE": str(cache_base),
            "FACTORY_ENVIRONMENT_BUILDER_CACHE": str(cache),
            "FACTORY_ENVIRONMENT_BUILDER_NIX_STORE": str(store),
        }
        completed = subprocess.run(
            [
                "bash",
                str(WRAPPER),
                "build",
                "--run-dir",
                str(run_dir),
                "--request",
                str(request_path),
                "--workspace",
                str(workspace),
                "--output",
                str(output),
            ],
            capture_output=True,
            text=True,
            env=environment,
            timeout=60,
        )
        return completed, base, run_dir, output, marker

    def test_nonzero_container_purges_stale_success(self) -> None:
        completed, _base, run_dir, output, marker = self._run(rootless="true")
        self.assertEqual(completed.returncode, 2, completed.stderr)
        self.assertTrue(marker.exists(), "the fake container must have run")
        self.assertIn("ENVIRONMENT_BUILD_FAILED", completed.stdout)
        for base in (run_dir, output):
            for name in (
                "result.json",
                "builder-evidence.json",
                "check-result.json",
                "sbom.cdx.json",
                "closure.json",
                "license-inventory.json",
            ):
                self.assertFalse((base / name).exists(), f"{base}/{name} must be purged")
            self.assertTrue((base / "failure.json").exists())
            self.assertTrue((base / "disk-usage.json").exists())

    def test_nonzero_over_budget_is_resource_limit_and_purges_success(self) -> None:
        completed, _base, run_dir, output, marker = self._run(
            rootless="true", over_budget=True, disk_mib=1
        )
        self.assertEqual(completed.returncode, 2, completed.stderr)
        self.assertTrue(marker.exists())
        self.assertIn("ENVIRONMENT_RESOURCE_LIMIT", completed.stdout)
        for base in (run_dir, output):
            for name in (
                "result.json",
                "builder-evidence.json",
                "check-result.json",
                "sbom.cdx.json",
                "closure.json",
                "license-inventory.json",
            ):
                self.assertFalse((base / name).exists(), f"{base}/{name} must be purged")
            self.assertTrue((base / "disk-failure.json").exists())
            self.assertTrue((base / "disk-usage.json").exists())
            self.assertFalse((base / "failure.json").exists())

    def test_rootful_podman_is_rejected_before_run(self) -> None:
        completed, _base, run_dir, output, marker = self._run(rootless="false")
        self.assertEqual(completed.returncode, 2, completed.stderr)
        self.assertIn("ENVIRONMENT_SANDBOX_DENIED", completed.stdout)
        self.assertFalse(marker.exists(), "no container may run under rootful Podman")
        self.assertFalse((output / "result.json").exists())


if __name__ == "__main__":
    unittest.main()
