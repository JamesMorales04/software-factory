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
    _validate_snapshot_index,
    ensure_dir,
    ensure_run_identity,
    free_port,
    project_name,
    require_owned_run,
    write_manifest,
)
from compose_intake import stage_order  # noqa: E402
from scenario import (  # noqa: E402
    _load_scenario,
    _stage_slice,
    apply_saved_identity,
    completion_status,
    validate_resume_binding,
)


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


class RunOwnershipTests(unittest.TestCase):
    def test_new_run_gets_a_sentinel_and_resume_is_stable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("prepare", "summary", root=Path(directory))
            state = State.load_or_create(config.state_path)
            first = ensure_run_identity(config, state)
            second = ensure_run_identity(config, state)
            self.assertEqual(first["runId"], second["runId"])
            self.assertEqual(first["project"], second["project"])

    def test_existing_unowned_directory_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config = _config_for("prepare", "summary", root=root)
            ensure_dir(config.run_dir)
            (config.run_dir / "unrelated").write_text("keep", encoding="utf-8")
            state = State.load_or_create(config.state_path)
            with self.assertRaises(ScenarioError):
                ensure_run_identity(config, state)

    def test_cleanup_marker_is_required(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("prepare", "summary", root=Path(directory))
            with self.assertRaises(ScenarioError):
                require_owned_run(config)

    def test_snapshot_rejects_foreign_project(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("prepare", "summary", root=Path(directory))
            index = {"schema": "factory-e2e-volume-snapshot-v1", "project": "other", "volumes": []}
            with self.assertRaises(ScenarioError):
                _validate_snapshot_index(config, index)

    def test_snapshot_rejects_duplicate_volumes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("prepare", "summary", root=Path(directory))
            index = {
                "schema": "factory-e2e-volume-snapshot-v1",
                "project": config.project,
                "volumes": [
                    {"name": f"{config.project}_data", "archive": "a.tar", "labels": {}},
                    {"name": f"{config.project}_data", "archive": "b.tar", "labels": {}},
                ],
            }
            with self.assertRaises(ScenarioError):
                _validate_snapshot_index(config, index)

    def test_snapshot_rejects_archive_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("prepare", "summary", root=Path(directory))
            index = {
                "schema": "factory-e2e-volume-snapshot-v1",
                "project": config.project,
                "volumes": [
                    {
                        "name": f"{config.project}_data",
                        "archive": "../escape.tar",
                        "labels": {},
                    }
                ],
            }
            with self.assertRaises(ScenarioError):
                _validate_snapshot_index(config, index)


class ResumeIdentityTests(unittest.TestCase):
    def test_resume_reuses_saved_project_and_port(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("gate", "summary", root=Path(directory))
            state = State.load_or_create(config.state_path)
            state.set("run_id", "factory-e2e-original")
            state.set("project", "software-factory-e2e-original")
            state.set("port", 18894)
            state.save()
            apply_saved_identity(config, state, None, None)
            self.assertEqual(config.run_id, "factory-e2e-original")
            self.assertEqual(config.project, "software-factory-e2e-original")
            self.assertEqual(config.port, 18894)

    def test_resume_rejects_a_different_project(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config_for("gate", "summary", root=Path(directory))
            state = State.load_or_create(config.state_path)
            state.set("project", "software-factory-e2e-original")
            state.save()
            with self.assertRaises(ScenarioError):
                apply_saved_identity(config, state, "other-e2e", None)


class ResumeBindingTests(unittest.TestCase):
    def test_resume_rejects_a_new_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            state = State.load_or_create(Path(directory) / "state.json")
            state.set("candidate_sha", "a" * 40)
            with self.assertRaises(ScenarioError):
                validate_resume_binding(
                    state,
                    candidate_sha="b" * 40,
                    provider="fixture",
                    cassette_sha256=None,
                    scenario="compose-intake",
                    artifacts_dir="/tmp/artifacts",
                )

    def test_resume_rejects_provider_or_cassette_change(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            state = State.load_or_create(Path(directory) / "state.json")
            state.set("provider", "fixture")
            state.set("cassette_sha256", "c" * 64)
            with self.assertRaises(ScenarioError):
                validate_resume_binding(
                    state,
                    candidate_sha="a" * 40,
                    provider="cassette",
                    cassette_sha256="d" * 64,
                    scenario="compose-intake",
                    artifacts_dir="/tmp/artifacts",
                )


class PortTests(unittest.TestCase):
    def test_summary_without_restart_is_partial(self) -> None:
        completed = set(stage_order()) - {"restart"}
        self.assertEqual(completion_status(completed, "PASS", False, False), "PARTIAL")

    def test_full_clean_run_is_pass(self) -> None:
        self.assertEqual(completion_status(set(stage_order()), "PASS", True, False), "PASS")

    def test_dirty_run_is_never_pass(self) -> None:
        self.assertEqual(completion_status(set(stage_order()), "PASS", True, True), "PARTIAL")

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
