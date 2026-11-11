"""Disk-budget and bounded-output tests for the environment builder runner."""

from __future__ import annotations

import io
import gc
import os
import subprocess
import sys
import tempfile
import time
import unittest
import warnings
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from environment_builder.disk import (  # noqa: E402
    DU_TIMEOUT_SECONDS,
    MAX_DU_STDERR_BYTES,
    DiskBudget,
    _parse_du,
    directory_usage_bytes,
)
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
            (root / "payload.bin").write_bytes(b"x" * (2 * 1024 * 1024))
            used, complete = directory_usage_bytes([root])
            self.assertGreaterEqual(used, 2 * 1024 * 1024)
            self.assertTrue(complete)
            budget = DiskBudget(1, [root])
            with self.assertRaises(BuilderError) as raised:
                budget.check()
            self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
            record = budget.failure_record("check", used, complete)
            self.assertFalse(record["exact"])
            self.assertTrue(record["exceeded"])

    def test_du_requires_total_and_every_expected_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = str(Path(temporary))
            used, complete = _parse_du(
                f"2048\t{root}\n2048\ttotal\n", [root]
            )
            self.assertEqual(used, 2 * 1024 * 1024)
            self.assertTrue(complete)
            self.assertFalse(_parse_du("1\t/cache\n", ["/cache", "/nix", "/workspace"])[1])
            self.assertFalse(
                _parse_du(
                    "1\t/cache\n1\t/nix\n1\t/workspace\n",
                    ["/cache", "/nix", "/workspace"],
                )[1]
            )
            self.assertFalse(
                _parse_du(
                    f"1\t{root}\n1\t/other\n1\ttotal\n", [root]
                )[1]
            )
            self.assertFalse(_parse_du("not-a-size\n", [root])[1])
            self.assertFalse(_parse_du("", [root])[1])

    def test_noisy_du_stderr_is_bounded(self) -> None:
        class FakeDu:
            returncode = 0
            pid = 4244

            def __init__(self, stdout: bytes, stderr: bytes) -> None:
                self.stdout = io.BytesIO(stdout)
                self.stderr = io.BytesIO(stderr)

            def wait(self, timeout: float | None = None) -> int:
                return 0

            def poll(self) -> int:
                return 0

        with tempfile.TemporaryDirectory() as temporary:
            root = str(Path(temporary))
            process = FakeDu(
                f"1\t{root}\n1\ttotal\n".encode(),
                b"e" * (MAX_DU_STDERR_BYTES * 8),
            )
            with mock.patch(
                "environment_builder.disk.subprocess.Popen",
                return_value=process,
            ):
                used, complete = directory_usage_bytes([root])
            self.assertEqual(used, 1024)
            self.assertTrue(complete)
            self.assertTrue(process.stdout.closed)
            self.assertTrue(process.stderr.closed)

    def test_slow_du_within_timeout_succeeds(self) -> None:
        class SlowDu:
            returncode = 0
            pid = 4246

            def __init__(self, stdout: bytes) -> None:
                self.stdout = io.BytesIO(stdout)
                self.stderr = io.BytesIO(b"")
                self.timeout_seen: float | None = None

            def wait(self, timeout: float | None = None) -> int:
                self.timeout_seen = timeout
                time.sleep(0.05)
                return 0

            def poll(self) -> int:
                return 0

        with tempfile.TemporaryDirectory() as temporary:
            root = str(Path(temporary))
            process = SlowDu(f"1\t{root}\n1\ttotal\n".encode())
            with mock.patch(
                "environment_builder.disk.subprocess.Popen",
                return_value=process,
            ):
                used, complete = directory_usage_bytes([root])
            self.assertEqual(used, 1024)
            self.assertTrue(complete)
            self.assertGreaterEqual(process.timeout_seen or 0, DU_TIMEOUT_SECONDS)

    def test_du_timeout_beyond_bound_fails_closed(self) -> None:
        class TimeoutDu:
            returncode = None
            pid = 4247

            def __init__(self) -> None:
                self.stdout = io.BytesIO(b"")
                self.stderr = io.BytesIO(b"")

            def wait(self, timeout: float | None = None) -> int:
                raise subprocess.TimeoutExpired("du", timeout or 0)

            def poll(self) -> None:
                return None

        with tempfile.TemporaryDirectory() as temporary:
            root = str(Path(temporary))
            with (
                mock.patch(
                    "environment_builder.disk.subprocess.Popen",
                    return_value=TimeoutDu(),
                ),
                mock.patch("environment_builder.disk.os.killpg"),
            ):
                used, complete = directory_usage_bytes([root])
            self.assertEqual(used, 0)
            self.assertFalse(complete)

    def test_check_retries_transient_incomplete_then_succeeds(self) -> None:
        budget = DiskBudget(1024, [], retry_backoff=0)
        calls = [(0, False), (0, False), (1024, True)]
        with mock.patch.object(budget, "usage", side_effect=calls):
            budget.check()

    def test_check_fails_after_three_incomplete_attempts(self) -> None:
        budget = DiskBudget(1024, [], retry_backoff=0)
        with mock.patch.object(
            budget, "usage", side_effect=[(0, False), (0, False), (0, False)]
        ) as usage:
            with self.assertRaises(BuilderError) as raised:
                budget.check()
        self.assertEqual(usage.call_count, 3)
        self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
        self.assertIn("3 attempts", raised.exception.message)

    def test_monitor_requires_three_consecutive_incomplete_samples(self) -> None:
        class Process:
            pid = 4248

            def poll(self) -> None:
                return None

        budget = DiskBudget(1, [], interval=0.05, timeout=0.1)
        process = Process()
        killed: list[int] = []
        with (
            mock.patch.object(budget, "usage", return_value=(0, False)),
            mock.patch(
                "environment_builder.disk.os.killpg",
                side_effect=lambda pid, _sig: killed.append(pid),
            ),
        ):
            monitor = budget.monitor(process)
            monitor.start()
            time.sleep(0.35)
            monitor.stop()
        self.assertTrue(monitor.exceeded)
        self.assertTrue(monitor.incomplete)
        self.assertEqual(killed, [process.pid])

    def test_monitor_transient_failures_do_not_kill(self) -> None:
        class Process:
            pid = 4249

            def poll(self) -> None:
                return None

        budget = DiskBudget(1, [], interval=0.05, timeout=0.1)
        process = Process()
        calls = [(0, False), (0, False), (1024, True)]

        def usage() -> tuple[int, bool]:
            return calls.pop(0) if calls else (1024, True)

        killed: list[int] = []
        with (
            mock.patch.object(budget, "usage", side_effect=usage),
            mock.patch(
                "environment_builder.disk.os.killpg",
                side_effect=lambda pid, _sig: killed.append(pid),
            ),
        ):
            monitor = budget.monitor(process)
            monitor.start()
            time.sleep(0.35)
            monitor.stop()
        self.assertFalse(monitor.exceeded)
        self.assertFalse(monitor.incomplete)
        self.assertEqual(killed, [])

    def test_unreadable_scan_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            missing = root / "missing"
            budget = DiskBudget(10240, [missing], retry_backoff=0)
            used, complete = directory_usage_bytes([missing])
            self.assertFalse(complete)
            self.assertEqual(used, 0)
            with self.assertRaises(BuilderError) as raised:
                budget.check()
            self.assertEqual(raised.exception.code, "ENVIRONMENT_RESOURCE_LIMIT")
            self.assertIn("could not complete", raised.exception.message)

    @unittest.skipIf(os.geteuid() == 0, "root can read chmod 000 directories")
    def test_unreadable_directory_chmod_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            blocked = root / "blocked"
            blocked.mkdir()
            (blocked / "payload.bin").write_bytes(b"x" * 4096)
            os.chmod(blocked, 0)
            try:
                used, complete = directory_usage_bytes([blocked])
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
                interval=0.5,
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
                interval=0.5,
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
