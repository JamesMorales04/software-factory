"""End-to-end consumer contracts for the autonomous stages.

The run history shows one mismatched field per attempt (a NameError in the stage
body, then `plan.planHash` on the restart consumer, then an invented
`compiled.ready` on participants). These tests execute the real stage functions
in order - autonomous_campaign, autonomous_negative, autonomous_restart,
autonomous_summary - against responses shaped after the real structs:

* intake detail renders FrozenLeadPlanRecord with snake_case `plan_hash`;
* /agentic/requests/{id}/plan renders AgenticWorkGraphRecord camelCase;
* /agentic/requests/{id}/participants returns {"plan": AgenticParticipantPlanRecord}
  with requestId, graphSha256, route, assurance, workUnits, reviewerCount,
  planJson, planSha256, createdAt, updatedAt and no `ready` flag;
* /campaigns, /workers and /provider/sessions keep their documented shapes.

The fakes only route requests; they never re-implement stage logic, so a field
mismatch fails here the same way it fails against the real Core.
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
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
import scenario  # noqa: E402
from common import ScenarioError  # noqa: E402

PLAN_HASH = "193abcbf2764082a91a127fa8ba1aef35e65b2d91a777dedf3ed1a22350b56ae"
OTHER_PLAN_HASH = "cd" * 32
GRAPH_SHA = "0e595b182e583449f4a50ed47572757a3cf0baa85f569db3af82b9a37c4ca1b6"
SPEC_SHA = "dd25f6724f3169a6" + "ab" * 24
PLAN_JSON = '{"route":"FORMAL_SPEC","assurance":"NONE","assignments":[]}'
PLAN_DIGEST = hashlib.sha256(PLAN_JSON.encode("utf-8")).hexdigest()


class State:
    def __init__(self, values):
        self.values = dict(values)

    def require(self, *keys):
        for key in keys:
            if not self.values.get(key):
                raise AssertionError(f"missing state {key}")

    def get(self, key, default=None):
        return self.values.get(key, default)

    def set(self, key, value):
        self.values[key] = value

    def save(self):
        return None


class Api:
    """Routes requests to real-shape payloads and lets a test mutate them."""

    def __init__(self, artifacts_dir: Path):
        self.artifacts_dir = artifacts_dir
        self.restarted = False
        self.context_plan_hash = PLAN_HASH
        self.after_plan_hash = PLAN_HASH
        self.participants = {
            "plan": {
                "requestId": "i1",
                "graphSha256": GRAPH_SHA,
                "route": "FORMAL_SPEC",
                "assurance": "None",
                "workUnits": 3,
                "reviewerCount": 2,
                "planJson": PLAN_JSON,
                "planSha256": PLAN_DIGEST,
                "createdAt": "2026-11-17T04:51:43.802618703+00:00",
                "updatedAt": "2026-11-17T04:51:43.802618703+00:00",
            }
        }
        self.compose = None

    def projection(self) -> dict:
        plan_hash = self.after_plan_hash if self.restarted else PLAN_HASH
        return {
            "id": "i1",
            "state": "EXECUTING",
            "campaignId": "c1",
            "plan": {
                "id": "1abe7d6d-e532-45a2-8592-ed044e7045ba",
                "plan_hash": plan_hash,
                "configHash": "config",
                "createdAt": "2026-11-17T04:51:43.772398435+00:00",
            },
            "planning": {
                "ready": True,
                "missingArtifacts": [],
                "participantPlan": {"ready": True},
                "reviewStatus": "AUTO_APPROVED",
                "reviewApproved": True,
            },
            "environment": {
                "status": "FROZEN",
                "specSha256": SPEC_SHA,
                "required": True,
                "builder": {"receipt": {"specSha256": SPEC_SHA, "status": "SUCCEEDED"}},
            },
        }

    def campaign(self) -> dict:
        return {
            "campaign": {"status": "AUTHORIZED", "effective_status": "AUTHORIZED"},
            "tasks": [{"task_id": "t1", "attempt_id": "a1", "status": "RUNNING"}],
            "attempts": [{"id": "a1", "run_id": "r1", "status": "LEASED"}],
            "runs": [{"id": "r1", "campaign_id": "c1", "status": "RUNNING"}],
        }

    def get(self, path):
        if path == "/intake/requests/i1":
            return self.projection()
        if path == "/campaigns":
            return [{"id": "c1", "action_id": "act1"}]
        if path == "/campaigns/c1":
            return self.campaign()
        if path == "/workers":
            return [{"worker_id": "compose-worker", "updated_at": "2026-11-17T04:51:43Z"}]
        if path == "/provider/sessions":
            return [
                {
                    "attempt_id": "a1",
                    "worker_id": "compose-worker",
                    "generation": 1,
                    "state": "ACTIVE",
                    "provider": "opencode-go",
                    "model": "deepseek-v4.1-flash",
                }
            ]
        if path == "/agentic/requests/i1/plan":
            return {
                "graph": {
                    "requestId": "i1",
                    "graphJson": json.dumps({"units": [{"unit_id": "u1"}]}),
                    "graphSha256": GRAPH_SHA,
                    "plannerProvider": "opencode-go",
                    "plannerModel": "deepseek-v4.1-flash",
                }
            }
        if path == "/agentic/requests/i1/participants":
            return self.participants
        raise AssertionError(f"unexpected GET {path}")

    def post(self, path, body):
        if path == "/execution-profiles":
            return {"id": "profile-negative", "content": body}
        if path == "/intake/requests":
            raise ScenarioError(
                "HTTP 400 AUTONOMOUS_POLICY_REQUIRED: the profile has no autonomous policy"
            )
        raise AssertionError(f"unexpected POST {path}")


class Compose:
    def __init__(self, api: Api):
        self.api = api
        self.calls = []
        self.counters = {
            "assessment": 1,
            "lead_stack_selection": 1,
            "planner": 1,
            "environment": 1,
            "spec": 1,
        }

    def run(self, arguments, check=False):
        payload = {"counters": dict(self.counters), "errors": {}}
        return subprocess.CompletedProcess(
            args=list(arguments), returncode=0, stdout=json.dumps(payload) + "\n", stderr=""
        )

    def down(self, volumes=False):
        self.calls.append(("down", volumes))

    def up(self, build=False):
        self.calls.append(("up", build))
        self.api.restarted = True
        # The fixture process restarts with the stack: its counters reset to zero.
        self.counters = {key: 0 for key in self.counters}


def context_factory(api: Api, artifacts_dir: Path):
    config = mock.Mock()
    config.run_timeout = None
    config.readiness_timeout = 5
    config.run_id = "test-run"
    config.artifacts_dir = artifacts_dir
    return mock.Mock(
        api=api,
        config=config,
        compose=Compose(api),
        state=State(
            {
                "project_id": "p1",
                "intake_id": "i1",
                "intake_action_id": "act1",
                "intake_lead_request_id": "lead1",
                "run_id": "run-1",
                "candidate_sha": "candidate-1",
            }
        ),
    )


class AutonomousStageSequenceTests(unittest.TestCase):
    """Drive the four stages in order with real projection shapes."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.artifacts_dir = Path(self.temporary.name)
        self.api = Api(self.artifacts_dir)
        self.context = context_factory(self.api, self.artifacts_dir)
        self.patches = [
            mock.patch.object(common, "wait_until", self._wait_once),
            mock.patch.object(compose_intake, "wait_url", lambda *args, **kwargs: None),
            mock.patch.object(compose_intake, "wait_core", lambda *args, **kwargs: None),
        ]
        for patch in self.patches:
            patch.start()
            self.addCleanup(patch.stop)

    @staticmethod
    def _wait_once(predicate, timeout, description, *, interval=1.0):
        for _ in range(3):
            if predicate():
                return True
        raise ScenarioError(f"timed out waiting for {description}")

    def run_campaign(self) -> None:
        compose_intake.autonomous_campaign(self.context)

    def test_full_sequence_passes_and_summary_reports_pass(self) -> None:
        self.run_campaign()
        compose_intake.autonomous_negative(self.context)
        compose_intake.autonomous_restart(self.context)
        compose_intake.autonomous_summary(self.context)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["status"], "PASS")
        self.assertTrue(summary["restart_persistence"])
        self.assertEqual(summary["plan_hash"], PLAN_HASH)
        self.assertEqual(summary["worker_assignment"], "PROVEN")
        self.assertEqual(summary["negative"]["outcome"], "REJECTED")

    def test_restart_fails_when_the_plan_hash_changes(self) -> None:
        self.run_campaign()
        self.api.after_plan_hash = OTHER_PLAN_HASH
        with self.assertRaises(ScenarioError):
            compose_intake.autonomous_restart(self.context)

    def test_restart_fails_when_the_after_plan_hash_is_missing(self) -> None:
        self.run_campaign()
        self.api.after_plan_hash = None
        with self.assertRaises(ScenarioError):
            compose_intake.autonomous_restart(self.context)

    def test_campaign_fails_when_participants_keep_the_invented_compiled_shape(self) -> None:
        self.api.participants = {"compiled": {"ready": True, "graphSha256": GRAPH_SHA}}
        with self.assertRaises(ScenarioError):
            self.run_campaign()

    def test_campaign_fails_when_the_participant_graph_binding_disagrees(self) -> None:
        self.api.participants["plan"]["graphSha256"] = OTHER_PLAN_HASH
        with self.assertRaises(ScenarioError):
            self.run_campaign()

    def test_campaign_fails_when_the_plan_hash_is_not_a_digest(self) -> None:
        original = self.api.projection

        def truncated() -> dict:
            projection = original()
            projection["plan"]["plan_hash"] = "193abcbf"
            return projection

        self.api.projection = truncated  # type: ignore[method-assign]
        with self.assertRaises(ScenarioError):
            self.run_campaign()

    def test_campaign_fails_when_the_claimed_plan_digest_does_not_match(self) -> None:
        self.api.participants["plan"]["planSha256"] = OTHER_PLAN_HASH
        with self.assertRaises(ScenarioError):
            self.run_campaign()

    def test_campaign_fails_when_the_plan_json_is_empty(self) -> None:
        self.api.participants["plan"]["planJson"] = ""
        with self.assertRaises(ScenarioError):
            self.run_campaign()

    def test_dispatcher_loop_keeps_persistence_and_summary_passes(self) -> None:
        for name in ("autonomous_campaign", "autonomous_negative", "autonomous_restart", "autonomous_summary"):
            scenario.dispatch_stage(self.context, name)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertTrue(self.context.state.get("restart_persistence"))
        self.assertEqual(summary["status"], "PASS")
        self.assertTrue(summary["restart_persistence"])

    def test_dispatcher_loop_keeps_pre_restart_counters_when_live_resets(self) -> None:
        for name in ("autonomous_campaign", "autonomous_negative", "autonomous_restart", "autonomous_summary"):
            scenario.dispatch_stage(self.context, name)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["status"], "PASS")
        self.assertEqual(summary["counters_source"], "pre_restart")
        self.assertEqual(summary["counters_by_phase"]["post_restart_live"]["planner"], 0)
        self.assertEqual(summary["fixture_counters"]["planner"], 1)

    def test_snapshot_binding_matrix_is_strict(self) -> None:
        cases = {
            "missing_recorded_run": lambda values: values["autonomous_counters_snapshot"].pop("run_id"),
            "missing_recorded_candidate": lambda values: values["autonomous_counters_snapshot"].pop("candidate_sha"),
            "missing_stage": lambda values: values["autonomous_counters_snapshot"].pop("stage"),
            "wrong_stage": lambda values: values["autonomous_counters_snapshot"].update(stage="autonomous_restart"),
            "missing_captured_at": lambda values: values["autonomous_counters_snapshot"].pop("captured_at"),
            "invalid_captured_at": lambda values: values["autonomous_counters_snapshot"].update(captured_at="not-a-date"),
            "naive_captured_at": lambda values: values["autonomous_counters_snapshot"].update(captured_at="2026-11-17T05:20:00"),
            "mismatch_run": lambda values: values["autonomous_counters_snapshot"].update(run_id="another-run"),
            "mismatch_candidate": lambda values: values["autonomous_counters_snapshot"].update(candidate_sha="another"),
            "missing_current_run": lambda values: values.pop("run_id"),
            "missing_current_candidate": lambda values: values.pop("candidate_sha"),
        }
        for label, mutate in cases.items():
            with self.subTest(label=label):
                api = Api(self.artifacts_dir)
                context = context_factory(api, self.artifacts_dir)
                compose_intake.autonomous_campaign(context)
                mutate(context.state.values)
                compose_intake.autonomous_negative(context)
                compose_intake.autonomous_restart(context)
                compose_intake.autonomous_summary(context)
                summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
                self.assertEqual(summary["status"], "PARTIAL", label)
                self.assertEqual(summary["counters_source"], "missing", label)

    def test_summary_is_partial_when_the_stored_snapshot_is_missing(self) -> None:
        self.run_campaign()
        state = self.context.state
        for key in ("autonomous_counters_snapshot", "autonomous_counters"):
            state.values.pop(key, None)
        self.context.compose.counters = {key: 0 for key in self.context.compose.counters}
        compose_intake.autonomous_negative(self.context)
        compose_intake.autonomous_restart(self.context)
        compose_intake.autonomous_summary(self.context)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["status"], "PARTIAL")
        self.assertEqual(summary["counters_source"], "missing")

    def test_summary_rejects_a_snapshot_with_a_mismatched_run(self) -> None:
        self.run_campaign()
        self.context.state.values["autonomous_counters_snapshot"]["run_id"] = "another-run"
        compose_intake.autonomous_negative(self.context)
        compose_intake.autonomous_restart(self.context)
        compose_intake.autonomous_summary(self.context)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["status"], "PARTIAL")
        self.assertEqual(summary["counters_source"], "missing")

    def test_dispatcher_loop_reports_partial_when_restart_never_ran(self) -> None:
        for name in ("autonomous_campaign", "autonomous_negative", "autonomous_summary"):
            scenario.dispatch_stage(self.context, name)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertFalse(self.context.state.get("restart_persistence"))
        self.assertEqual(summary["status"], "PARTIAL")

    def test_summary_is_partial_without_restart_persistence(self) -> None:
        self.run_campaign()
        compose_intake.autonomous_summary(self.context)
        summary = json.loads((self.artifacts_dir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["status"], "PARTIAL")


if __name__ == "__main__":
    unittest.main()
