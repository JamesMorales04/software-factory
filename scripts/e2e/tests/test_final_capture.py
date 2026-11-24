"""Ordering and failure tests for the final pre-cleanup evidence capture.

No Podman is required. The fake adapter records the capture→cleanup order and
the scenarios required after the 04 run lost its cgroup evidence on cleanup.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
E2E = HERE.parent
if str(E2E) not in sys.path:
    sys.path.insert(0, str(E2E))

import common  # noqa: E402


class FinalizeOrderTests(unittest.TestCase):
    def test_capture_runs_before_cleanup_on_success(self) -> None:
        order = []
        capture_result, cleanup_result, capture_error = common.finalize_with_capture(
            lambda: order.append("capture") or "CAPTURED",
            lambda: order.append("cleanup") or "CLEANED",
        )
        self.assertEqual(order, ["capture", "cleanup"])
        self.assertEqual(capture_result, "CAPTURED")
        self.assertEqual(cleanup_result, "CLEANED")
        self.assertIsNone(capture_error)

    def test_capture_failure_still_runs_cleanup_and_keeps_original_error(self) -> None:
        order = []
        run_error = RuntimeError("original run failure")

        def capture():
            order.append("capture")
            raise OSError("cgroup unavailable")

        def cleanup():
            order.append("cleanup")
            return "CLEANED"

        caught = None
        try:
            raise run_error
        except RuntimeError as error:
            caught = error
            _, cleanup_result, capture_error = common.finalize_with_capture(
                capture, cleanup
            )
        self.assertEqual(order[:2], ["capture", "cleanup"])
        self.assertEqual(cleanup_result, "CLEANED")
        self.assertIsInstance(capture_error, OSError)
        self.assertIs(caught, run_error)

    def test_cancellation_capture_runs_cleanup_and_propagates_after(self) -> None:
        order = []

        def capture():
            order.append("capture")
            raise KeyboardInterrupt()

        def cleanup():
            order.append("cleanup")
            return "CLEANED"

        _, cleanup_result, capture_error = common.finalize_with_capture(capture, cleanup)
        self.assertEqual(order[:2], ["capture", "cleanup"])
        self.assertEqual(cleanup_result, "CLEANED")
        self.assertIsInstance(capture_error, KeyboardInterrupt)
        # The caller re-raises the original cancellation after finalization.

    def test_cleanup_failure_is_not_hidden(self) -> None:
        def cleanup():
            raise RuntimeError("cleanup failed")

        with self.assertRaises(RuntimeError):
            common.finalize_with_capture(lambda: "CAPTURED", cleanup)


class CaptureBestEffortTests(unittest.TestCase):
    def test_missing_podman_records_not_captured_without_blocking(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = SimpleNamespace(
                root=root,
                artifacts_dir=root / "artifacts",
                project="software-factory-e2e-test",
                environment=lambda: {},
            )
            compose = SimpleNamespace(service_container=lambda name: None)
            original = common._bounded_run

            def fail(*args, **kwargs):
                raise common.ScenarioError("podman is unavailable")

            common._bounded_run = fail
            try:
                summary = common.capture_final_evidence(
                    config, compose, common.State(path=root / "state.json"), None,
                    reason="failure:test",
                )
            finally:
                common._bounded_run = original
            capture = json.loads(
                (config.artifacts_dir / "final-capture" / "capture.json").read_text()
            )
        self.assertEqual(summary["reason"], "failure:test")
        self.assertEqual(capture["artifacts"]["containers.txt"]["status"], "NOT_CAPTURED")
        self.assertEqual(capture["artifacts"]["health.txt"]["status"], "NOT_CAPTURED")
        self.assertEqual(capture["artifacts"]["fixture-counters.json"]["status"], "NOT_CAPTURED")

    def test_no_prompt_or_cookie_body_is_written(self) -> None:
        self.assertEqual(
            common.redact_text("Cookie: session=abc\nprompt: secret body"),
            "Cookie=[REDACTED]\nprompt: secret body",
        )


if __name__ == "__main__":
    unittest.main()
