"""Unit tests for the stage-addressed Compose scenario runner.

These tests intentionally do not start Podman, Compose, Core or a browser. They
cover the deterministic orchestration surface that can fail before any
container work: stage selection, state resume, project isolation, manifest
truthfulness and the opt-in modes that must fail closed.
"""

from __future__ import annotations

import json
import socket
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from common import (  # noqa: E402
    Config,
    ScenarioError,
    State,
    free_port,
    project_name,
    write_manifest,
)
from compose_intake import stage_order  # noqa: E402
from scenario import _load_scenario, _stage_slice  # noqa: E402


class StageOrderTests(unittest.TestCase):
    def test_canonical_stage_order(self) -> None:
        self.assertEqual(
            stage_order(),
            [
                "prepare",
                "start",
                "enroll",
                "import",
                "freeze",
                "delegate",
                "settings",
                "intake",
                "assess",
                "approve",
                "gate",
                "restart",
                "browser",
                "summary",
            ],
        )

    def test_stage_slice_is_inclusive(self) -> None:
        stages = stage_order()
        config = _config_for("import", "gate")
        self.assertEqual(
            _stage_slice(config, stages),
            ["import", "freeze", "delegate", "settings", "intake", "assess", "approve", "gate"],
        )

    def test_scenario_rejects_out_of_order_stages(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scenario.json"
            path.write_text(
                json.dumps({"name": "broken", "stages": ["gate", "prepare"]}),
                encoding="utf-8",
            )
            with self.assertRaises(ScenarioError):
                _load_scenario(str(path))

    def test_scenario_accepts_a_subset(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scenario.json"
            path.write_text(
                json.dumps({"name": "smoke", "stages": ["prepare", "start", "summary"]}),
                encoding="utf-8",
            )
            name, stages = _load_scenario(str(path))
            self.assertEqual(name, "smoke")
            self.assertEqual(stages, ["prepare", "start", "summary"])


class StateTests(unittest.TestCase):
    def test_state_round_trip_and_require(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.json"
            state = State.load_or_create(path)
            state.set("project_id", "project-1")
            state.append("stage_results", {"name": "prepare", "status": "PASS"})
            state.save()

            loaded = State.load_or_create(path)
            self.assertEqual(loaded.get("project_id"), "project-1")
            self.assertEqual(loaded.data["stage_results"][0]["status"], "PASS")
            loaded.require("project_id")
            with self.assertRaises(ScenarioError):
                loaded.require("campaign_id")

    def test_state_append_rejects_non_list(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            state = State.load_or_create(Path(directory) / "state.json")
            state.set("stage_results", "not-a-list")
            with self.assertRaises(ScenarioError):
                state.append("stage_results", {"name": "x"})


class ProjectNameTests(unittest.TestCase):
    def test_rejects_shared_project(self) -> None:
        with self.assertRaises(ScenarioError):
            project_name("software-factory")

    def test_rejects_project_without_e2e(self) -> None:
        with self.assertRaises(ScenarioError):
            project_name("factory-test")

    def test_rejects_invalid_characters(self) -> None:
        with self.assertRaises(ScenarioError):
            project_name("factory-e2e/../../bad")

    def test_accepts_dedicated_project(self) -> None:
        self.assertEqual(project_name("software-factory-e2e-123"), "software-factory-e2e-123")


class ManifestTests(unittest.TestCase):
    def test_partial_manifest_never_claims_pass(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = _config_for("prepare", "gate", root=root)
            state = State.load_or_create(config.state_path)
            manifest = write_manifest(
                config,
                state,
                status="PARTIAL",
                started_at="2026-11-12T00:00:00Z",
                scenarios_ran=["prepare", "gate"],
            )
            self.assertEqual(manifest["status"], "PARTIAL")
            self.assertFalse(manifest["paidInference"])
            self.assertFalse(manifest["security"]["isolated"])
            self.assertFalse(manifest["security"]["certified"])
            self.assertEqual(manifest["provider"], "fixture")
            self.assertEqual(manifest["github"], "fake")
            self.assertEqual(manifest["browser"], "none")

    def test_manifest_records_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = _config_for("prepare", "summary", root=root)
            state = State.load_or_create(config.state_path)
            manifest = write_manifest(
                config,
                state,
                status="FAIL",
                started_at="2026-11-12T00:00:00Z",
                error="gate: timeout",
            )
            self.assertEqual(manifest["status"], "FAIL")
            self.assertIn("timeout", manifest["error"])
            self.assertTrue((config.artifacts_dir / "manifest.json").is_file())


class PortTests(unittest.TestCase):
    def test_free_port_is_bindable(self) -> None:
        port = free_port()
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.bind(("127.0.0.1", port))


def _config_for(
    from_stage: str,
    to_stage: str,
    *,
    root: Path | None = None,
) -> Config:
    base = root or Path(tempfile.mkdtemp(prefix="factory-e2e-test-"))
    run_dir = base / "run"
    return Config(
        root=base,
        run_id="factory-e2e-test",
        run_dir=run_dir,
        artifacts_dir=run_dir / "artifacts",
        project="software-factory-e2e-test",
        port=8894,
        provider="fixture",
        github="fake",
        cassette=None,
        browser="none",
        from_stage=from_stage,
        to_stage=to_stage,
        keep=True,
        snapshot=False,
        restore=False,
        snapshot_dir=run_dir / "snapshots",
        no_build=False,
        timeout=60,
        scenario="compose-intake",
    )


if __name__ == "__main__":
    unittest.main()
