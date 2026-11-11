"""Tests for disk-overflow cleanup of success artifacts."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
DISK = HERE.parents[2] / "scripts" / "environment-builder-disk-policy.py"


def load_disk_policy():
    spec = importlib.util.spec_from_file_location("builder_disk_policy", DISK)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


DISK_MODULE = load_disk_policy()


class DiskPolicyTests(unittest.TestCase):
    def _seed(self, run: Path, output: Path) -> None:
        for base in (run, output):
            base.mkdir(parents=True, exist_ok=True)
            (base / "result.json").write_text('{"status":"SUCCEEDED"}', encoding="utf-8")
            (base / "builder-evidence.json").write_text('{"status":"SUCCEEDED"}', encoding="utf-8")
            (base / "check-result.json").write_text('{"status":"PASSED"}', encoding="utf-8")
            (base / "sbom.cdx.json").write_text("{}", encoding="utf-8")
            (base / "closure.json").write_text("{}", encoding="utf-8")
        (run / "disk-usage.json").write_text(
            json.dumps({"exceeded": True, "planDiskMiB": 1, "observedKiB": 2048}),
            encoding="utf-8",
        )

    def test_precheck_overflow_purges_existing_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "run"
            output = Path(directory) / "output"
            self._seed(run, output)
            DISK_MODULE.purge_success(run, output)
            DISK_MODULE.write_failure(run, output, "pre-build")
            for base in (run, output):
                self.assertFalse((base / "result.json").exists())
                self.assertFalse((base / "builder-evidence.json").exists())
                self.assertFalse((base / "check-result.json").exists())
                self.assertFalse((base / "sbom.cdx.json").exists())
                failure = json.loads((base / "disk-failure.json").read_text(encoding="utf-8"))
                self.assertEqual(failure["status"], "FAILED")
                self.assertEqual(failure["blocker"]["code"], "ENVIRONMENT_RESOURCE_LIMIT")
            self.assertTrue((run / "disk-usage.json").exists())

    def test_check_overflow_leaves_no_passed_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory) / "run"
            output = Path(directory) / "output"
            self._seed(run, output)
            # Simulate a PASSED check result produced just before the post-check.
            (run / "check-result.json").write_text('{"status":"PASSED"}', encoding="utf-8")
            (output / "check-result.json").write_text('{"status":"PASSED"}', encoding="utf-8")
            DISK_MODULE.purge_success(run, output)
            DISK_MODULE.write_failure(run, output, "post-check")
            for base in (run, output):
                self.assertFalse((base / "check-result.json").exists())
                self.assertFalse((base / "result.json").exists())
                self.assertTrue((base / "disk-failure.json").exists())


if __name__ == "__main__":
    unittest.main()
