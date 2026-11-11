"""Disk-budget and bounded-output tests for the environment builder runner."""

from __future__ import annotations

import io
import gc
import os
import sys
import tempfile
import time
import unittest
import warnings
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from environment_builder.disk import DiskBudget, directory_usage_bytes  # noqa: E402
from environment_builder.errors import BuilderError  # noqa: E402
from environment_builder.nix import (  # noqa: E402
    MAX_CHECK_STREAM_BYTES,
    BuildLog,
    run_check,
    run_logged,
)

RESOURCES = {"cpu": 2, "memoryMiB": 4096, "diskMiB": 1, "pids": 512, "timeoutSecs": 30}


class DiskBudgetTests(unittest.TestCase):
    def test_bounded_usage_and_over_budget_rejection(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            payload = root / "payload.bin"
            payload.write_bytes(b"x" * (2 * 1024 * 1024))
            used, complete = directory_usage_bytes([root])
            self.assertGreaterEqual(used, 2 * 1024 * 1024)
            self.assertTrue(complete)
            limited, complete = directory_usage_bytes(
                [root], max_entries=0, max_seconds=0.1
            )
            self.assertFalse(complete)
            self.assertLessEqual(limited, used)
            budget = DiskBudget(1, [root], max_entries=100, max_seconds=1.0)
            with self.assertRaises(BuilderError) as raised:
                budget.check()
            self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
            record = budget.failure_record("check", used, complete)
            self.assertFalse(record["exact"])
            self.assertTrue(record["exceeded"])

    def test_unreadable_scan_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "payload.bin").write_bytes(b"x" * 4096)
            with mock.patch(
                "environment_builder.disk.os.scandir",
                side_effect=PermissionError("denied"),
            ):
                used, complete = directory_usage_bytes([root])
            self.assertFalse(complete)
            self.assertEqual(used, 0)
            budget = DiskBudget(10240, [root])
            with mock.patch(
                "environment_builder.disk.os.scandir",
                side_effect=PermissionError("denied"),
            ):
                with self.assertRaises(BuilderError) as raised:
                    budget.check()
            self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
            self.assertIn("cannot complete", raised.exception.message)

    @unittest.skipIf(os.geteuid() == 0, "root can read chmod 000 directories")
    def test_unreadable_directory_chmod_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            blocked = root / "blocked"
            blocked.mkdir()
            (blocked / "payload.bin").write_bytes(b"x" * 4096)
            os.chmod(blocked, 0)
            try:
                used, complete = directory_usage_bytes([root])
            finally:
                os.chmod(blocked, 0o700)
        self.assertFalse(complete)
        self.assertEqual(used, 0)

    def test_run_check_kills_an_over_budget_child(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            cache = root / "cache"
            workspace.mkdir()
            cache.mkdir()
            log = BuildLog(cache / "check.log")
            budget = DiskBudget(
                1,
                [workspace, cache],
                interval=0.05,
                max_seconds=0.5,
            )
            started = time.monotonic()
            try:
                with mock.patch(
                    "environment_builder.nix._environment_command",
                    return_value=[
                        "/bin/sh",
                        "-c",
                        "dd if=/dev/zero of=big bs=1M count=3 2>/dev/null; sleep 10",
                    ],
                ):
                    with self.assertRaises(BuilderError) as raised:
                        run_check(
                            workspace,
                            cache,
                            workspace,
                            ["true"],
                            timeout=30,
                            resources=RESOURCES,
                            log=log,
                            expect_exit=0,
                            disk_budget=budget,
                        )
            finally:
                log.close()
            self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
            self.assertLess(time.monotonic() - started, 8.0)

    def test_run_logged_kills_an_over_budget_child(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            workspace.mkdir()
            log = BuildLog(root / "build.log")
            budget = DiskBudget(
                1,
                [root],
                interval=0.05,
                max_seconds=0.5,
            )
            started = time.monotonic()
            try:
                with self.assertRaises(BuilderError) as raised:
                    run_logged(
                        [
                            "/bin/sh",
                            "-c",
                            "dd if=/dev/zero of=big bs=1M count=3 2>/dev/null; sleep 10",
                        ],
                        cwd=workspace,
                        env={"PATH": "/usr/bin:/bin"},
                        timeout=30,
                        log=log,
                        disk_budget=budget,
                    )
            finally:
                log.close()
            self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
            self.assertLess(time.monotonic() - started, 8.0)

    def test_noisy_check_output_is_retained_only_up_to_the_bound(self) -> None:
        class NoisyProcess:
            returncode = 0
            pid = 4243

            def __init__(self) -> None:
                self.stdout = io.BytesIO(b"o" * (MAX_CHECK_STREAM_BYTES * 4))
                self.stderr = io.BytesIO(b"e" * (MAX_CHECK_STREAM_BYTES * 4))

            def wait(self, timeout: int | None = None) -> int:
                return 0

            def poll(self) -> int:
                return 0

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workspace = root / "workspace"
            cache = root / "cache"
            workspace.mkdir()
            cache.mkdir()
            log = BuildLog(cache / "check.log")
            process = NoisyProcess()
            try:
                with (
                    mock.patch(
                        "environment_builder.nix._environment_command",
                        return_value=["true"],
                    ),
                    mock.patch(
                        "environment_builder.nix.subprocess.Popen",
                        return_value=process,
                    ),
                ):
                    _code, stdout, stderr, out_trunc, err_trunc = run_check(
                        workspace,
                        cache,
                        workspace,
                        ["true"],
                        timeout=30,
                        resources=RESOURCES,
                        log=log,
                    )
                self.assertLessEqual(log.path.stat().st_size, 1024 * 1024 + 128)
            finally:
                log.close()
        self.assertEqual(len(stdout), MAX_CHECK_STREAM_BYTES)
        self.assertEqual(len(stderr), MAX_CHECK_STREAM_BYTES)
        self.assertTrue(out_trunc)
        self.assertTrue(err_trunc)
        self.assertTrue(process.stdout.closed)
        self.assertTrue(process.stderr.closed)
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            gc.collect()
        self.assertFalse(
            [warning for warning in caught if issubclass(warning.category, ResourceWarning)]
        )


if __name__ == "__main__":
    unittest.main()
