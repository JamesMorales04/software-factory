"""Lifecycle supervision tests for the Compose scenario harness.

These tests use only short local processes. They do not start Podman, build
images or touch any Factory stack. They pin the contract introduced after the
2026-11-15 start timeout: silent legitimate work is not killed by a universal
clock, our process group is reaped on explicit cancellation, streaming output
never deadlocks, and build/start are not bound to the short operation timeout.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
E2E = HERE.parent
if str(E2E) not in sys.path:
    sys.path.insert(0, str(E2E))

import common  # noqa: E402


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


class RunSupervisedTests(unittest.TestCase):
    def test_silent_legitimate_process_is_not_killed_by_a_clock(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "silent.log"
            started = time.monotonic()
            result = common.run_supervised(
                ["bash", "-c", "sleep 1.2; echo finished"],
                cwd=Path(directory),
                env=os.environ.copy(),
                log_path=log_path,
                timeout=None,
                check=False,
            )
            elapsed = time.monotonic() - started
            logged = log_path.read_text(encoding="utf-8")
        self.assertEqual(result.returncode, 0)
        self.assertGreater(elapsed, 1.0)
        self.assertIn("finished", logged)

    def test_cancellation_reaps_child_and_grandchild(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            log_path = base / "tree.log"
            child_pid = base / "child.pid"
            grandchild_pid = base / "grandchild.pid"
            script = (
                f"sleep 60 & echo $! > {grandchild_pid}; "
                f"echo $$ > {child_pid}; wait"
            )
            result = common.run_supervised(
                ["bash", "-c", script],
                cwd=base,
                env=os.environ.copy(),
                log_path=log_path,
                timeout=1,
                check=False,
            )
            self.assertTrue(result.timed_out)
            time.sleep(0.5)
            for path in (child_pid, grandchild_pid):
                self.assertTrue(path.is_file(), path)
                pid = int(path.read_text(encoding="utf-8").strip())
                self.assertFalse(pid_alive(pid), f"process {pid} survived cancellation")

    def test_continuous_output_does_not_deadlock(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "stream.log"
            result = common.run_supervised(
                ["bash", "-c", "for i in $(seq 1 2000); do echo line-$i; done"],
                cwd=Path(directory),
                env=os.environ.copy(),
                log_path=log_path,
                timeout=None,
                check=False,
            )
            text = log_path.read_text(encoding="utf-8")
        self.assertEqual(result.returncode, 0)
        self.assertIn("line-2000", text)

    def test_real_error_preserves_exit_and_logs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "error.log"
            result = common.run_supervised(
                ["bash", "-c", "echo boom >&2; exit 7"],
                cwd=Path(directory),
                env=os.environ.copy(),
                log_path=log_path,
                timeout=None,
                check=False,
            )
            text = log_path.read_text(encoding="utf-8")
        self.assertEqual(result.returncode, 7)
        self.assertIn("boom", text)
        self.assertIn("exit=7", text)


class BuildBindingTests(unittest.TestCase):
    def make_compose(self, root: Path):
        config = SimpleNamespace(
            root=root,
            run_dir=root / "run",
            artifacts_dir=root / "artifacts",
            project="software-factory-e2e-test",
            operation_timeout=30,
            readiness_timeout=60,
            run_timeout=None,
            environment=lambda: os.environ.copy(),
        )
        config.run_dir.mkdir(parents=True, exist_ok=True)
        return common.Compose(config)

    def test_no_build_refuses_without_a_prebuild_record(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            compose = self.make_compose(Path(directory))
            with self.assertRaises(common.ScenarioError):
                compose._verify_prebuilt_binding()

    def test_no_build_refuses_a_record_from_another_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            compose = self.make_compose(root)
            (compose.config.run_dir / "prebuilt.json").write_text(
                json.dumps(
                    {
                        "schema": "factory-e2e-prebuild-v1",
                        "candidateSha": "not-the-current-sha",
                        "images": {},
                    }
                ),
                encoding="utf-8",
            )
            class FakeCompose(type(compose)):
                def _candidate_sha(self) -> str:
                    return "current-sha"

            fake = FakeCompose(compose.config)
            with self.assertRaises(common.ScenarioError) as raised:
                fake._verify_prebuilt_binding()
            self.assertIn("not bound to the current candidate", str(raised.exception))

    def test_sequential_prebuild_uses_run_timeout_not_operation_timeout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            compose = self.make_compose(root)
            calls = []

            class FakeCompose(type(compose)):
                def _candidate_sha(self) -> str:
                    return "sha-1"

                def _compose_services(self):
                    return ["a", "b"]

                def _compose_images(self):
                    return ["img-a:dev", "img-b:dev"]

            fake = FakeCompose(compose.config)
            original = common.run_supervised

            def fake_run(command, *, cwd, env, log_path, timeout, check, label=""):
                calls.append((command, timeout))
                return common.SupervisedResult(0, "", "", log_path, timed_out=False)

            original_run = common.subprocess.run

            def fake_listing(command, **kwargs):
                class Result:
                    returncode = 0
                    stdout = "img-a:dev id-a\nimg-b:dev id-b\n"
                    stderr = ""

                return Result()

            common.run_supervised = fake_run
            common.subprocess.run = fake_listing
            try:
                binding = fake.prebuild_sequential()
            finally:
                common.run_supervised = original
                common.subprocess.run = original_run
            self.assertEqual(binding["candidateSha"], "sha-1")
            self.assertEqual([entry[1] for entry in calls], [None, None])
            self.assertEqual(len(calls), 2)
            self.assertIn("build", calls[0][0])


class CliContractTests(unittest.TestCase):
    def test_cli_separates_operation_readiness_and_run_timeouts(self) -> None:
        import scenario

        args = scenario._parse_args([])
        self.assertEqual(args.operation_timeout, 120)
        self.assertEqual(args.readiness_timeout, 300)
        self.assertIsNone(args.run_timeout)
        self.assertIsNone(args.timeout)

    def test_deprecated_timeout_flag_is_not_the_build_deadline(self) -> None:
        import scenario

        args = scenario._parse_args(["--timeout", "45"])
        self.assertEqual(args.timeout, 45)
        self.assertIsNone(args.run_timeout)


if __name__ == "__main__":
    unittest.main()
