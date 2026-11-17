"""Focal contracts for the start path and the campaign consumer helpers.

These cover the consumer code that broke in the CR-17/18/20/21 chain, with the
real adapter return types (`CompletedProcess` for `podman ps`, plain strings for
`exec`) and explicit health negatives. Syntax and import checks alone missed
every one of those defects.
"""

from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
E2E = ROOT / "scripts" / "e2e"
for entry in (str(ROOT), str(E2E)):
    if entry not in sys.path:
        sys.path.insert(0, entry)

import common  # noqa: E402
import compose_intake  # noqa: E402
from common import Compose, Config, ScenarioError  # noqa: E402


def config() -> Config:
    root = Path("/tmp/factory-start-contract-test")
    return Config(
        root=root,
        run_id="run-1",
        run_dir=root / "run",
        artifacts_dir=root / "artifacts",
        project="software-factory-e2e-run-1",
        port=12345,
        provider="fixture",
        github="fake",
        cassette=None,
        browser="none",
        from_stage="prepare",
        to_stage="summary",
        keep=False,
        snapshot=False,
        restore=False,
        snapshot_dir=root / "snaps",
        no_build=False,
        operation_timeout=5,
        readiness_timeout=5,
        run_timeout=5,
        scenario="autonomous-intake",
    )


class ServiceLookupTests(unittest.TestCase):
    """CR-21: the canonical lookup parses a CompletedProcess, not a string."""

    def test_service_container_parses_completed_process(self) -> None:
        compose = Compose(config())
        completed = subprocess.CompletedProcess(
            ["podman", "ps"], 0, stdout="project_runtime_1\n", stderr=""
        )
        with mock.patch.object(common, "_bounded_run", return_value=completed):
            self.assertEqual(compose.service_container("runtime"), "project_runtime_1")

    def test_service_container_is_empty_when_absent(self) -> None:
        compose = Compose(config())
        completed = subprocess.CompletedProcess(["podman", "ps"], 0, stdout="", stderr="")
        with mock.patch.object(common, "_bounded_run", return_value=completed):
            self.assertEqual(compose.service_container("runtime"), "")


class WaitServiceTests(unittest.TestCase):
    """CR-20: the readiness wait must fail closed on unhealthy exec calls."""

    def test_wait_returns_once_the_service_is_healthy(self) -> None:
        compose = mock.Mock()
        attempts = {"count": 0}

        def exec_service(service, arguments, check=True):
            attempts["count"] += 1
            if attempts["count"] < 3:
                raise ScenarioError("connection refused")
            return "ok"

        compose.exec_service.side_effect = exec_service
        common.wait_service(compose, "runtime", "http://127.0.0.1:8770/health", 5)
        self.assertEqual(attempts["count"], 3)

    def test_wait_fails_closed_on_a_never_healthy_service(self) -> None:
        compose = mock.Mock()
        compose.exec_service.side_effect = ScenarioError("connection refused")
        with self.assertRaises(ScenarioError):
            common.wait_service(compose, "runtime", "http://127.0.0.1:8770/health", 0.2)


class CampaignHelperTests(unittest.TestCase):
    """CR-17/CR-18: consumer helpers take the projection, not stray locals."""

    def test_review_status_accepts_only_the_closed_set(self) -> None:
        self.assertTrue(compose_intake.review_status_accepted({"reviewStatus": "AUTO_APPROVED"}))
        self.assertTrue(compose_intake.review_status_accepted({"reviewStatus": "OWNER_APPROVED"}))
        for rejected in ["PENDING", "", None, "AUTO_APPROVED_EXTRA"]:
            self.assertFalse(
                compose_intake.review_status_accepted({"reviewStatus": rejected}),
                f"{rejected!r} must not be accepted",
            )

    def test_projected_plan_hash_reads_the_projection_only(self) -> None:
        self.assertEqual(compose_intake.projected_plan_hash({"plan": {"planHash": "a"}}), "a")
        self.assertEqual(
            compose_intake.projected_plan_hash({"plan": {"plan_hash": "b"}}), "b"
        )
        self.assertEqual(compose_intake.projected_plan_hash({}), "")
        self.assertEqual(compose_intake.projected_plan_hash({"plan": None}), "")




class OneshotPrerequisiteTests(unittest.TestCase):
    """The start consumer requires init and the bundles to finish with exit 0.

    The canonical bring-up is kept: the guard validates the real order (up first,
    then the one-shots) and never imposes a runtime-first alternative.
    """

    def test_missing_prerequisite_container_is_a_hard_error(self) -> None:
        compose = mock.Mock()
        compose.container_state.return_value = None
        with self.assertRaises(ScenarioError):
            common.wait_oneshot(compose, "init", 1)

    def test_failed_prerequisite_exit_code_is_rejected(self) -> None:
        compose = mock.Mock()
        compose.container_state.return_value = ("exited", 3)
        with self.assertRaises(ScenarioError):
            common.wait_oneshot(compose, "dependency-bundle", 1)

    def test_successful_prerequisite_exit_is_accepted(self) -> None:
        compose = mock.Mock()
        compose.container_state.return_value = ("exited", 0)
        common.wait_oneshot(compose, "environment-builder-bundle", 1)
        compose.container_state.assert_called()

    def test_running_prerequisite_times_out_closed(self) -> None:
        compose = mock.Mock()
        compose.container_state.return_value = ("running", 0)
        with self.assertRaises(ScenarioError):
            common.wait_oneshot(compose, "init", 0.2)




class ProjectionShapeTests(unittest.TestCase):
    """CR-23/CR-24: strict-JSON captures and nested graph units."""

    def test_graph_units_are_read_from_the_nested_json(self) -> None:
        nested = json.dumps({"units": [{"id": "u1"}], "graphSha256": "x" * 64})
        projection = {"graph": {"graphJson": nested, "plannerProvider": "fixture"}}
        self.assertEqual(len(compose_intake.projected_graph_units(projection)), 1)

    def test_flat_units_still_work(self) -> None:
        self.assertEqual(
            compose_intake.projected_graph_units({"units": [{"id": "a"}]}), [{"id": "a"}]
        )

    def test_missing_or_broken_graph_returns_no_units(self) -> None:
        self.assertEqual(compose_intake.projected_graph_units({}), [])
        self.assertEqual(
            compose_intake.projected_graph_units({"graph": {"graphJson": "not json"}}), []
        )

    def test_capture_strips_raw_control_characters(self) -> None:
        dirty = {"detail": "bad\x01value\nok", "nested": ["a\x02b"]}
        cleaned = common._strip_control_characters(dirty)
        self.assertEqual(cleaned["detail"], "badvalue\nok")
        self.assertEqual(cleaned["nested"], ["ab"])
        json.dumps(cleaned)  # strict JSON on the cleaned value


if __name__ == "__main__":
    unittest.main()
