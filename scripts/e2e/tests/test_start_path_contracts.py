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
    """CR-24: the documented nested graph contract, with hard failures."""

    def test_units_are_read_from_the_nested_json(self) -> None:
        nested = json.dumps({"units": [{"id": "u1"}], "graphSha256": "x" * 64})
        projection = {"graph": {"graphJson": nested, "plannerProvider": "fixture"}}
        self.assertEqual(len(compose_intake.projected_graph_units(projection)), 1)

    def test_a_missing_graph_fails_clearly(self) -> None:
        with self.assertRaises(ScenarioError):
            compose_intake.projected_graph_units({})

    def test_a_missing_graph_json_fails_clearly(self) -> None:
        with self.assertRaises(ScenarioError):
            compose_intake.projected_graph_units({"graph": {"plannerProvider": "fixture"}})

    def test_malformed_graph_json_fails_clearly(self) -> None:
        with self.assertRaises(ScenarioError):
            compose_intake.projected_graph_units({"graph": {"graphJson": "not json"}})

    def test_empty_units_fail_clearly(self) -> None:
        with self.assertRaises(ScenarioError):
            compose_intake.projected_graph_units(
                {"graph": {"graphJson": json.dumps({"units": []})}}
            )


class CaptureWriterTests(unittest.TestCase):
    """CR-23: redaction happens before encoding; the file is strict JSON."""

    def test_redaction_never_breaks_a_json_string(self) -> None:
        secret = "token=" + "a" * 40 + "\nnext_line"
        value = {"detail": secret, "nested": [secret]}
        redacted = common.redact_value(value)
        payload = json.dumps(redacted)
        json.loads(payload)
        self.assertNotIn("a" * 40, payload)


class ExactProjectionContractTests(unittest.TestCase):
    """The exact nested shapes Core projects, including provenance and receipt."""

    def real_graph(self):
        nested = json.dumps({"units": [{"id": "u1"}], "graphSha256": "y" * 64})
        return {
            "graph": {
                "graphJson": nested,
                "graphSha256": "y" * 64,
                "plannerProvider": "opencode-go",
                "plannerModel": "deepseek-v4.1-flash",
                "costMicros": 0,
                "createdAt": "2026-11-17T02:53:02Z",
                "inputTokens": 0,
                "outputTokens": 0,
            }
        }

    def test_provenance_comes_from_the_nested_graph_only(self) -> None:
        digest, provider, model = compose_intake.projected_graph_provenance(self.real_graph())
        self.assertEqual(digest, "y" * 64)
        self.assertEqual(provider, "opencode-go")
        self.assertEqual(model, "deepseek-v4.1-flash")

    def test_missing_provenance_fails_clearly(self) -> None:
        for field in ("graphSha256", "plannerProvider", "plannerModel"):
            projection = self.real_graph()
            del projection["graph"][field]
            with self.subTest(field=field), self.assertRaises(ScenarioError):
                compose_intake.projected_graph_provenance(projection)

    def test_receipt_must_bind_the_frozen_spec(self) -> None:
        environment = {
            "specSha256": "s" * 64,
            "builder": {"receipt": {"specSha256": "s" * 64, "status": "SUCCEEDED"}},
        }
        self.assertEqual(compose_intake.receipt_binds_frozen_spec(environment), (True, ""))

    def test_receipt_mismatch_and_failure_are_refused(self) -> None:
        for receipt in (
            {"specSha256": "x" * 64, "status": "SUCCEEDED"},
            {"specSha256": "s" * 64, "status": "FAILED"},
            None,
        ):
            environment = {"specSha256": "s" * 64, "builder": {"receipt": receipt}}
            ok, problem = compose_intake.receipt_binds_frozen_spec(environment)
            self.assertFalse(ok, problem)

    def real_campaign(self):
        return {
            "tasks": [{"task_id": "t1", "attempt_id": "a1"}],
            "attempts": [{"id": "a1", "run_id": "r1", "status": "LEASED"}],
            "runs": [{"id": "r1", "campaign_id": "c1"}],
        }

    def test_the_real_attempt_chain_is_accepted(self) -> None:
        ok, problem = compose_intake.attempt_chain_is_observable(self.real_campaign(), "c1")
        self.assertTrue(ok, problem)

    def test_running_without_a_projected_attempt_is_refused(self) -> None:
        campaign = self.real_campaign()
        campaign["attempts"] = []
        ok, problem = compose_intake.attempt_chain_is_observable(campaign, "c1")
        self.assertFalse(ok, problem)

    def test_attempt_run_from_another_campaign_is_refused(self) -> None:
        campaign = self.real_campaign()
        campaign["runs"] = [{"id": "r1", "campaign_id": "other"}]
        ok, problem = compose_intake.attempt_chain_is_observable(campaign, "c1")
        self.assertFalse(ok, problem)

    def test_unknown_attempt_or_missing_task_attempt_is_refused(self) -> None:
        campaign = self.real_campaign()
        campaign["tasks"] = [{"task_id": "t1", "attempt_id": "missing"}]
        ok, problem = compose_intake.attempt_chain_is_observable(campaign, "c1")
        self.assertFalse(ok, problem)
        campaign = self.real_campaign()
        campaign["tasks"] = [{"task_id": "t1"}]
        ok, problem = compose_intake.attempt_chain_is_observable(campaign, "c1")
        self.assertFalse(ok, problem)

    def test_queued_attempt_is_not_execution(self) -> None:
        campaign = self.real_campaign()
        campaign["attempts"] = [{"id": "a1", "run_id": "r1", "status": "QUEUED"}]
        ok, problem = compose_intake.attempt_chain_is_observable(campaign, "c1")
        self.assertFalse(ok, problem)


class WorkerAssignmentTests(unittest.TestCase):
    """Real assignment needs a provider session bound to the attempt and Worker."""

    def session(self, **overrides):
        record = {
            "session_record_id": "s1",
            "attempt_id": "a1",
            "worker_id": "compose-worker",
            "generation": 1,
            "provider": "fixture",
            "model": "fixture-model",
            "state": "ACTIVE",
        }
        record.update(overrides)
        return record

    def test_a_bound_session_proves_the_assignment(self) -> None:
        worker, generation, problem = compose_intake.worker_assignment_from_sessions(
            [self.session()], "a1", {"compose-worker"}
        )
        self.assertEqual((worker, generation, problem), ("compose-worker", 1, ""))

    def test_no_session_is_a_clear_failure(self) -> None:
        worker, generation, problem = compose_intake.worker_assignment_from_sessions(
            [], "a1", {"compose-worker"}
        )
        self.assertEqual(worker, "")
        self.assertIn("no provider session", problem)

    def test_attempt_mismatch_is_refused(self) -> None:
        worker, _, _ = compose_intake.worker_assignment_from_sessions(
            [self.session(attempt_id="other")], "a1", {"compose-worker"}
        )
        self.assertEqual(worker, "")

    def test_zero_or_missing_generation_is_refused(self) -> None:
        for generation in (0, -1, None):
            worker, _, _ = compose_intake.worker_assignment_from_sessions(
                [self.session(generation=generation)], "a1", {"compose-worker"}
            )
            self.assertEqual(worker, "", f"generation {generation!r} must be refused")

    def test_unknown_worker_is_refused(self) -> None:
        worker, _, problem = compose_intake.worker_assignment_from_sessions(
            [self.session(worker_id="stranger")], "a1", {"compose-worker"}
        )
        self.assertEqual(worker, "")
        self.assertIn("no provider session", problem)

    def test_registered_worker_alone_is_not_enough(self) -> None:
        worker, _, _ = compose_intake.worker_assignment_from_sessions(
            [], "a1", {"compose-worker"}
        )
        self.assertEqual(worker, "")


class AsyncConsumerTests(unittest.TestCase):
    """The consumer waits for the lease instead of reading once."""

    def api(self, projections):
        class Api:
            def __init__(self):
                self.calls = 0

            def get(self, path):
                index = min(self.calls, len(projections) - 1)
                self.calls += 1
                return projections[index]

        return Api()

    def leased(self):
        return {
            "tasks": [{"task_id": "t1", "attempt_id": "a1"}],
            "attempts": [{"id": "a1", "run_id": "r1", "status": "LEASED"}],
            "runs": [{"id": "r1", "campaign_id": "c1"}],
        }

    def unleased(self):
        campaign = self.leased()
        campaign["attempts"] = [{"id": "a1", "run_id": "r1", "status": "PENDING"}]
        return campaign

    def test_consumer_waits_for_a_late_lease(self) -> None:
        api = self.api([self.unleased(), self.unleased(), self.leased()])
        projection = compose_intake.await_attempt_chain(api, "c1", 5)
        self.assertTrue(projection.get("tasks"))
        self.assertGreaterEqual(api.calls, 3)

    def test_consumer_fails_closed_when_the_lease_never_arrives(self) -> None:
        api = self.api([self.unleased()])
        with self.assertRaises(ScenarioError):
            compose_intake.await_attempt_chain(api, "c1", 0.2)


class AsyncTerminalFailureTests(unittest.TestCase):
    """Terminal states use the real nested Campaign projection and abort at once.

    `/campaigns/{id}` nests the Campaign under `campaign` with `status` and
    `effective_status`; the wait must abort on either, including
    BLOCKED_AUTHORIZATION, even with no timeout at all.
    """

    def api(self, campaign_fields):
        class Api:
            def __init__(self):
                self.calls = 0

            def get(self, path):
                self.calls += 1
                return {"campaign": campaign_fields}

        return Api()

    def test_chain_aborts_immediately_on_nested_terminal_states(self) -> None:
        for fields in (
            {"status": "BLOCKED", "effective_status": "BLOCKED"},
            {"status": "AUTHORIZED", "effective_status": "BLOCKED_AUTHORIZATION"},
            {"status": "CANCELLED", "effective_status": "CANCELLED"},
        ):
            api = self.api(fields)
            with self.subTest(fields=fields), self.assertRaises(
                common.TerminalScenarioError
            ) as caught:
                compose_intake.await_attempt_chain(api, "c1", None)
            self.assertIn("Campaign reached", str(caught.exception))
            self.assertEqual(api.calls, 1)

    def test_assignment_aborts_immediately_on_a_nested_terminal_state(self) -> None:
        api = self.api({"status": "AUTHORIZED", "effective_status": "BLOCKED_AUTHORIZATION"})
        with self.assertRaises(common.TerminalScenarioError):
            compose_intake.await_worker_assignment(api, "c1", "a1", None)
        self.assertEqual(api.calls, 1)

    def test_a_non_terminal_campaign_is_not_treated_as_terminal(self) -> None:
        self.assertEqual(
            compose_intake.campaign_terminal_failure(
                {"campaign": {"status": "EXECUTING", "effective_status": "EXECUTING"}}
            ),
            "",
        )


class AutonomousCampaignStageTests(unittest.TestCase):
    """Exercise the stage call site, not only the helpers it calls.

    The 3c95766e run crashed with `name 'config' is not defined` after the
    product had frozen the environment: the helpers were tested directly, so
    the stage's own call site was never executed.
    """

    class State:
        def __init__(self, values):
            self.values = dict(values)
            self.written = {}

        def require(self, *keys):
            for key in keys:
                if key not in self.values:
                    raise AssertionError(f"missing state {key}")

        def get(self, key, default=None):
            return self.values.get(key, default)

        def set(self, key, value):
            self.values[key] = value
            self.written[key] = value

    class Compose:
        def run(self, arguments, check=False):
            payload = {
                "counters": {
                    "assessment": 1,
                    "lead_stack_selection": 1,
                    "planner": 1,
                    "environment": 1,
                    "spec": 1,
                },
                "errors": {},
            }
            return subprocess.CompletedProcess(
                args=list(arguments), returncode=0, stdout=json.dumps(payload) + "\n", stderr=""
            )

    def context(self):
        projection = {
            "state": "EXECUTING",
            "campaignId": "c1",
            "planning": {
                "ready": True,
                "missingArtifacts": [],
                "participantPlan": {"ready": True},
                "reviewStatus": "AUTO_APPROVED",
                "reviewApproved": True,
            },
            "environment": {
                "status": "FROZEN",
                "specSha256": "spec1",
                "required": True,
                "builder": {"receipt": {"specSha256": "spec1", "status": "SUCCEEDED"}},
            },
            "plan": {"planHash": "193abcbf2764082a91a127fa8ba1aef35e65b2d91a777dedf3ed1a22350b56ae"},
        }
        campaign = {
            "campaign": {"status": "AUTHORIZED", "effective_status": "AUTHORIZED"},
            "tasks": [{"task_id": "t1", "attempt_id": "a1"}],
            "attempts": [{"id": "a1", "run_id": "r1", "status": "LEASED"}],
            "runs": [{"id": "r1", "campaign_id": "c1"}],
        }
        graph = {
            "graph": {
                "graphJson": json.dumps({"units": [{"unit_id": "u1"}]}),
                "graphSha256": "sha1",
                "plannerProvider": "opencode-go",
                "plannerModel": "deepseek-v4.1-flash",
            }
        }
        participants = {
            "plan": {
                "graphSha256": "sha1",
                "workUnits": 1,
                "planSha256": "ef" * 32,
            }
        }

        class Api:
            def get(self, path):
                if path.startswith("/intake/requests/"):
                    return projection
                if path.startswith("/campaigns/"):
                    return campaign
                if path == "/workers":
                    return [{"worker_id": "compose-worker"}]
                if path == "/provider/sessions":
                    return [
                        {
                            "attempt_id": "a1",
                            "worker_id": "compose-worker",
                            "generation": 1,
                            "state": "ACTIVE",
                        }
                    ]
                if path.endswith("/plan"):
                    return graph
                if path.endswith("/participants"):
                    return participants
                raise AssertionError(f"unexpected path {path}")

        config = mock.Mock()
        config.run_timeout = None
        config.readiness_timeout = 5
        return mock.Mock(
            api=Api(),
            config=config,
            compose=self.Compose(),
            state=self.State({"intake_id": "i1"}),
        )

    def test_stage_call_site_completes_and_records_the_scope(self) -> None:
        context = self.context()
        compose_intake.autonomous_campaign(context)
        self.assertEqual(
            context.state.written.get("acceptance_scope"),
            "CAMPAIGN_START_WITH_RECEIPT_AND_WORKER_ASSIGNMENT",
        )

    def test_stage_aborts_when_the_receipt_does_not_bind_the_spec(self) -> None:
        context = self.context()
        broken = context.api.get("/intake/requests/i1")
        broken["environment"]["builder"]["receipt"]["specSha256"] = "other"
        with self.assertRaises(ScenarioError):
            compose_intake.autonomous_campaign(context)


if __name__ == "__main__":
    unittest.main()
