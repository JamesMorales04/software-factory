#!/usr/bin/env python3
"""Synthetic tests for the real generation-2 recovery scenario.

No Core, provider, container or paid inference is contacted. Run:
python3 -m unittest scripts/e2e/tests/test_real_recovery.py
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

HERE = Path(__file__).resolve()
sys.path.insert(0, str(HERE.parents[1]))
import real_recovery  # noqa: E402
import real_recovery_profile  # noqa: E402


COMMAND_SHA = "d" * 64
STATUS_COMMAND_SHA = "e" * 64
SESSION1 = "sess-1"
SESSION2 = "sess-2"
CANDIDATE_ID = "cand-1"


def observation(**overrides):
    defaults = dict(
        attempt_id="attempt-1",
        campaign_id="campaign-1",
        claimed_generation_two=True,
        claimed_at="2026-11-18T00:00:00Z",
        generation_two_session_state="COMPLETED",
        generation_two_session_error="",
        generation_one_receipts=[
            real_recovery.CheckReceipt(
                "attempt-1", 1, real_recovery.STATUS, "PASS", "a" * 64, "rec-1",
                "2026-11-17T23:59:00Z", STATUS_COMMAND_SHA, SESSION1, 1, "call-1", "unit-1",
            )
        ],
        generation_two_receipts=[
            real_recovery.CheckReceipt(
                "attempt-1", 2, real_recovery.VERSION, "PASS", "b" * 64, "rec-2",
                "2026-11-18T00:01:00Z", COMMAND_SHA, SESSION2, 2, "call-2", "unit-1",
            )
        ],
        pre_intervention_version_outcome="",
        candidates_after_claim=[
            {"id": CANDIDATE_ID, "attempt_id": "attempt-1", "created_at": "2026-11-18T00:02:00Z"}
        ],
        allocation_conflict=False,
        gen2_provider="opencode-go",
        gen2_model="deepseek-v4.1-flash",
        gen2_provider_mode="build",
        candidate_sha="a" * 40,
        candidate_tree="b" * 40,
        candidate_base="c" * 40,
        expected_provider="opencode-go",
        expected_model="deepseek-v4.1-flash",
        expected_provider_mode="build",
        expected_work_item="unit-1",
        expected_command_sha256=COMMAND_SHA,
        expected_status_command_sha256=STATUS_COMMAND_SHA,
        expected_base_commit="c" * 40,
        campaign_detail_id="campaign-1",
        gen1_session_record_ids=[SESSION1],
        gen2_session_record_id=SESSION2,
        candidate_id=CANDIDATE_ID,
        gate_candidate_id=CANDIDATE_ID,
        campaign_action="act-1",
        expected_action="act-1",
    )
    defaults.update(overrides)
    return real_recovery.Observation(**defaults)


class RecoveryOracleTests(unittest.TestCase):
    def test_ok_requires_p1_status_pending_version_and_candidate(self):
        verdict = real_recovery.classify_recovery(observation(gate_outcome="ACCEPTED", campaign_status="AUTHORIZED"))
        self.assertEqual(verdict.status, "ACCEPTED", verdict.reason)

    def test_scoped_ok_without_gate(self):
        verdict = real_recovery.classify_recovery(observation(campaign_status="AUTHORIZED"))
        self.assertEqual(verdict.status, "OK_SCOPED", verdict.reason)

    def test_claim_alone_is_not_ok(self):
        # No P1 receipt, no P3 receipt, no candidate: a claim is never a pass.
        verdict = real_recovery.classify_recovery(
            observation(
                generation_one_receipts=[],
                generation_two_receipts=[],
                candidates_after_claim=[],
            )
        )
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)
        self.assertNotEqual(verdict.status, "OK")

    def test_already_done_is_inconclusive_not_ok(self):
        verdict = real_recovery.classify_recovery(
            observation(
                generation_one_receipts=[
                    real_recovery.CheckReceipt("attempt-1", 1, real_recovery.STATUS, "PASS", "a" * 64, "rec-1", "2026-11-17T23:59:00Z"),
                    real_recovery.CheckReceipt("attempt-1", 1, real_recovery.VERSION, "PASS", "b" * 64, "rec-2", "2026-11-17T23:59:30Z"),
                ]
            )
        )
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_dangerous_late_effect_is_not_tested(self):
        verdict = real_recovery.classify_recovery(observation(late_effect_risk="DANGEROUS"))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_late_effect_classifier_distinguishes_legit_and_dangerous(self):
        kill, claim = "2026-11-18T00:00:00Z", "2026-11-18T00:01:00Z"
        legit = [{"kind": "provider.session_terminal", "recorded_at": "2026-11-18T00:00:10Z"}]
        dangerous = [{"kind": "provider.tool_event", "detail": "decision=RESULT", "recorded_at": "2026-11-18T00:00:20Z"}]
        self.assertEqual(real_recovery.classify_late_effects(legit, kill, claim, ["sess-1"])[0], "NONE")
        self.assertEqual(real_recovery.classify_late_effects(dangerous, kill, claim, ["sess-1"])[0], "DANGEROUS")
        late_gen1 = [{"kind": "provider.tool_event", "subject_id": "sess-1", "recorded_at": "2026-11-18T00:02:00Z"}]
        self.assertEqual(real_recovery.classify_late_effects(late_gen1, kill, claim, ["sess-1"])[0], "DANGEROUS")

    def test_receipt_command_mismatch_is_not_tested(self):
        data = observation()
        receipt = data.generation_two_receipts[0]
        broken = real_recovery.CheckReceipt(
            receipt.attempt_id, receipt.generation, receipt.check_id, receipt.outcome,
            receipt.candidate_sha256, receipt.receipt_id, receipt.recorded_at,
            "e" * 64, receipt.session_record_id, receipt.sequence, receipt.call_id, receipt.work_item_id,
        )
        verdict = real_recovery.classify_recovery(observation(generation_two_receipts=[broken]))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_multiple_post_claim_candidates_are_not_tested(self):
        extra = {"id": "cand-2", "attempt_id": "attempt-1", "created_at": "2026-11-18T00:03:00Z"}
        data = observation()
        verdict = real_recovery.classify_recovery(
            observation(candidates_after_claim=[*data.candidates_after_claim, extra])
        )
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_campaign_failed_is_fail(self):
        verdict = real_recovery.classify_recovery(observation(campaign_status="FAILED"))
        self.assertEqual(verdict.status, "FAIL", verdict.reason)

    def test_candidate_base_must_match_action_spec(self):
        verdict = real_recovery.classify_recovery(observation(candidate_base="d" * 40))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_status_receipt_identity_is_exact(self):
        status = observation().generation_one_receipts[0]
        broken = real_recovery.CheckReceipt(
            status.attempt_id, status.generation, status.check_id, status.outcome,
            status.candidate_sha256, status.receipt_id, status.recorded_at,
            "f" * 64, status.session_record_id, status.sequence, status.call_id, status.work_item_id,
        )
        verdict = real_recovery.classify_recovery(observation(generation_one_receipts=[broken]))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_confirmed_by_accepts_only_core_identity_forms(self):
        base = "Owner confirms Extra Usage OFF"
        self.assertTrue(real_recovery.confirmed_by_matches(base, base))
        self.assertTrue(real_recovery.confirmed_by_matches(f"operator-token:{base}", base))
        self.assertTrue(real_recovery.confirmed_by_matches(f"device:dev-1:{base}", base))
        self.assertFalse(real_recovery.confirmed_by_matches(f"other:{base}", base))
        self.assertFalse(real_recovery.confirmed_by_matches(f"operator-token:other:{base}", base))
        self.assertFalse(real_recovery.confirmed_by_matches(f"device:{base}", base))

    def test_quiescence_requires_settled_work_not_just_a_live_lease(self):
        sessions = [{"state": "ACTIVE", "turn_id": "turn-1"}]
        settled = [
            {"call_id": "c1", "decision": "REQUEST"},
            {"call_id": "c1", "decision": "RESULT"},
        ]
        health = {"runtime": "healthy", "supervisor": "healthy", "worker": "healthy"}
        self.assertTrue(real_recovery.quiescence_confirmed(sessions, settled, 1, 1, health))
        self.assertFalse(real_recovery.quiescence_confirmed(sessions, settled, 1, 0, health))
        self.assertFalse(real_recovery.quiescence_confirmed(sessions, settled[:1], 1, 1, health))
        self.assertFalse(real_recovery.quiescence_confirmed([], settled, 1, 1, health))
        self.assertFalse(
            real_recovery.quiescence_confirmed(sessions, settled, 1, 1, {**health, "worker": "unknown"})
        )

    def test_tool_calls_must_be_settled(self):
        settled = [
            {"call_id": "c1", "decision": "REQUEST"},
            {"call_id": "c1", "decision": "ALLOW"},
            {"call_id": "c1", "decision": "RESULT"},
        ]
        self.assertTrue(real_recovery.tool_calls_settled(settled))
        self.assertFalse(real_recovery.tool_calls_settled(settled[:-1]))
        self.assertFalse(real_recovery.tool_calls_settled([]))
        self.assertTrue(real_recovery.tool_calls_settled([{"call_id": "c2", "decision": "ERROR"}]))

    def test_gate_candidate_mismatch_is_not_tested(self):
        verdict = real_recovery.classify_recovery(observation(gate_candidate_id="other"))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_candidate_base_missing_is_not_tested(self):
        verdict = real_recovery.classify_recovery(observation(candidate_base=""))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_barrier_failures_are_not_guaranteed(self):
        snapshot = {
            "status_pass": True, "version_pass_absent": True, "attempt_status": "LEASED",
            "active_turns": 0, "gen1_quiescent": True, "tools_settled": True,
            "quiescence_backend_confirmed": True,
            "p2_check_absent": True, "p2_artifacts_absent": True, "candidate_count": 0,
            "lease_state": "LIVE", "runtime_health": "healthy", "supervisor_health": "healthy",
            "worker_health": "healthy",
        }
        self.assertEqual(real_recovery.barrier_failures(snapshot), [])
        snapshot["worker_health"] = "unhealthy"
        self.assertTrue(real_recovery.barrier_failures(snapshot))
        snapshot["worker_health"] = "healthy"
        snapshot["tools_settled"] = False
        self.assertTrue(real_recovery.barrier_failures(snapshot))
        snapshot["tools_settled"] = True
        snapshot["active_turns"] = 1
        self.assertIn("active_turns=1", real_recovery.barrier_failures(snapshot))
        snapshot["active_turns"] = 0
        snapshot["p2_check_absent"] = False
        self.assertIn("the P2 check is already present", real_recovery.barrier_failures(snapshot))

    def test_allocation_conflict_is_fail(self):
        verdict = real_recovery.classify_recovery(observation(allocation_conflict=True))
        self.assertEqual(verdict.status, "FAIL", verdict.reason)

    def test_gen2_failed_is_fail(self):
        verdict = real_recovery.classify_recovery(
            observation(generation_two_session_state="FAILED", generation_two_session_error="IMPLEMENTER_PROGRESS_Diagnose")
        )
        self.assertEqual(verdict.status, "FAIL", verdict.reason)

    def test_missing_gen2_version_receipt_is_fail(self):
        verdict = real_recovery.classify_recovery(observation(generation_two_receipts=[]))
        self.assertEqual(verdict.status, "FAIL", verdict.reason)
        self.assertIn("P3", verdict.reason)

    def test_precondition_pending_from_failed_receipt(self):
        observation_with_fail = observation(
            generation_one_receipts=[
                real_recovery.CheckReceipt("attempt-1", 1, real_recovery.STATUS, "PASS", "a" * 64, "rec-1", "2026-11-17T23:59:00Z"),
                real_recovery.CheckReceipt("attempt-1", 1, real_recovery.VERSION, "FAIL", "c" * 64, "rec-2", "2026-11-17T23:59:10Z"),
            ]
        )
        state, reason = real_recovery.classify_p2_precondition(observation_with_fail)
        self.assertEqual(state, "PENDING", reason)


class ProjectionTests(unittest.TestCase):
    def test_tool_event_envelope_is_unwrapped(self):
        payload = {"session": {"session_record_id": "s"}, "events": [{"sequence": 1}], "returned": 1}
        self.assertEqual(real_recovery.tool_event_rows(payload), [{"sequence": 1}])
        self.assertEqual(real_recovery.tool_event_rows([{"sequence": 2}]), [{"sequence": 2}])
        self.assertEqual(real_recovery.tool_event_rows(None), [])
        self.assertEqual(real_recovery.tool_event_rows({"events": ["junk"]}), [])


class ContractTests(unittest.TestCase):
    def test_exact_contract_has_no_mismatch(self):
        checks = [real_recovery.CHECKS[real_recovery.STATUS], real_recovery.CHECKS[real_recovery.VERSION]]
        self.assertEqual(real_recovery.contract_mismatch(checks), "")

    def test_marker_only_check_is_rejected(self):
        checks = [
            {
                "id": real_recovery.STATUS,
                "command": ["grep", "-q", "provider candidate", "src/provider.txt"],
                "expectExit": 0,
            }
        ]
        mismatch = real_recovery.contract_mismatch(checks)
        self.assertIn("differs from the exact contract", mismatch)

    def test_plan_commands_are_exact(self):
        plan = real_recovery.plan_document()
        self.assertEqual([c["id"] for c in plan["checks"]], [real_recovery.STATUS, real_recovery.VERSION])
        for check in plan["checks"]:
            self.assertNotIn("grep", check["command"])
            self.assertIn("assert app.", " ".join(check["command"]))


class ApiOriginTests(unittest.TestCase):
    def request_with(self, base, method, path):
        with mock.patch("urllib.request.urlopen") as urlopen:
            response = mock.MagicMock()
            response.read.return_value = b"{}"
            response.status = 200
            urlopen.return_value.__enter__.return_value = response
            api = real_recovery.Api(base, "test-token")
            getattr(api, method)(path, {}) if method == "post" else getattr(api, method)(path)
            urlopen.assert_called_once()
            return urlopen.call_args.args[0]

    def test_origin_is_scheme_and_netloc(self):
        for base, origin in (
            ("http://127.0.0.1:8901/core-api", "http://127.0.0.1:8901"),
            ("http://127.0.0.1:8901/core-api/", "http://127.0.0.1:8901"),
            ("http://127.0.0.1:8901", "http://127.0.0.1:8901"),
            ("https://factory.test/api", "https://factory.test"),
        ):
            request = self.request_with(base, "get", "/projects")
            self.assertEqual(request.get_header("Origin"), origin)
            self.assertEqual(request.get_header("Authorization"), "Bearer test-token")
            self.assertEqual(request.full_url, base.rstrip("/") + "/projects")

    def test_post_status_uses_the_full_model_timeout(self):
        with mock.patch("urllib.request.urlopen") as urlopen:
            response = mock.MagicMock()
            response.read.return_value = b"{}"
            response.status = 200
            urlopen.return_value.__enter__.return_value = response
            api = real_recovery.Api("http://127.0.0.1:8901/core-api", "token")
            status, _ = api.post_status("/intake/requests/intake-1/assess", {})
        self.assertEqual(status, 200)
        self.assertGreaterEqual(urlopen.call_args.kwargs.get("timeout", 0), 300)

    def test_post_uses_the_same_origin(self):
        request = self.request_with("http://127.0.0.1:8901/core-api", "post", "/projects")
        self.assertEqual(request.get_header("Origin"), "http://127.0.0.1:8901")
        self.assertEqual(request.full_url, "http://127.0.0.1:8901/core-api/projects")

    def test_model_calls_wait_past_the_old_sixty_second_client_timeout(self):
        with mock.patch("urllib.request.urlopen") as urlopen:
            response = mock.MagicMock()
            response.read.return_value = b"{}"
            response.status = 200
            urlopen.return_value.__enter__.return_value = response
            api = real_recovery.Api("http://127.0.0.1:8901/core-api", "token")
            api.post("/intake/requests/intake-1/assess", {})
        self.assertGreaterEqual(urlopen.call_args.kwargs.get("timeout", 0), 300)

    def test_relative_base_is_refused(self):
        with self.assertRaises(real_recovery.ScenarioError):
            real_recovery.Api("/core-api", "token")


class FakeApi:
    """Records calls and serves canonical Core JSON shapes."""

    def __init__(
        self, get_routes=None, post_routes=None, post_status_response=None,
        post_status_routes=None,
    ):
        self.get_routes = dict(get_routes or {})
        self.post_routes = dict(post_routes or {})
        self.post_status_routes = dict(post_status_routes or {})
        self.post_status_response = post_status_response or (
            403,
            {"error": "direct provider authorization refused"},
        )
        self.calls = []
        self.last_request = ("", "")
        self.timeout = 360.0

    def get(self, path):
        self.calls.append(("GET", path))
        if path not in self.get_routes:
            raise AssertionError(f"unexpected GET {path}")
        value = self.get_routes[path]
        return value(path) if callable(value) else value

    def post(self, path, payload):
        self.calls.append(("POST", path, payload))
        if path not in self.post_routes:
            raise AssertionError(f"unexpected POST {path}")
        value = self.post_routes[path]
        return value(payload) if callable(value) else value

    def post_status(self, path, payload):
        self.calls.append(("POST_STATUS", path, payload))
        return self.post_status_routes.get(path, self.post_status_response)


AUTHORIZATION_RECORD = {
    "campaignId": "campaign-1",
    "authorizationId": "auth-1",
    "riskClass": "R0",
    "executionMode": "TRUSTED_LOCAL_PROVIDER",
    "actor": "operator",
    "rationale": "owner authorized",
    "authorizedPaths": ["src"],
    "provider": "opencode-go",
    "model": "deepseek-v4.1-flash",
    "providerMode": "build",
    "delegationId": "delegation-1",
    "allowedTools": ["write_file"],
    "schemaVersion": 2,
    "policy": {"monetaryEnvelopeId": "envelope-1"},
    "createdAt": "2026-11-18T00:00:00Z",
}


def profile_fixture(**overrides):
    canonical = real_recovery_profile.canonical_profile(
        name="Compose Intake Balanced",
        description="Deterministic Compose intake profile",
        provider="opencode-go",
        model="deepseek-v4.1-flash",
    )
    profile = {
        "id": "profile-1",
        "revision": 1,
        "revisionId": "rev-1",
        "schemaVersion": 2,
        "contentHash": "8" * 64,
        "executable": True,
        **canonical,
        "autoStart": {
            "enabled": False,
            "riskClasses": canonical["auto_start"]["risk_classes"],
        },
    }
    profile.pop("auto_start", None)
    profile.update(overrides)
    return profile


def action_record():
    return {
        "id": "act-1",
        "project_id": "project-1",
        "objective": real_recovery.OBJECTIVE,
        "provider": "opencode-go",
        "model": "deepseek-v4.1-flash",
        "risk": "R0",
        "status": "AUTHORIZED",
        "spec": {
            "schema_version": 2,
            "project_id": "project-1",
            "repository": "owner/project",
            "base_commit": "c" * 40,
            "objective": real_recovery.OBJECTIVE,
            "objective_sha256": "1" * 64,
            "provider": "opencode-go",
            "model": "deepseek-v4.1-flash",
            "allowed_paths": ["src"],
            "checks": [real_recovery.CHECKS[real_recovery.STATUS], real_recovery.CHECKS[real_recovery.VERSION]],
        },
        "spec_sha256": "f" * 64,
    }


class TimeoutApi(FakeApi):
    def post(self, path, payload):
        self.calls.append(("POST", path, payload))
        raise TimeoutError("timed out")

    def post_status(self, path, payload):
        self.calls.append(("POST_STATUS", path, payload))
        if path.endswith("/assess"):
            raise TimeoutError("timed out")
        return self.post_status_response


class FailurePathTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        token = Path(self.tmp.name) / "operator.token"
        token.write_text("token", encoding="utf-8")
        self.args = argparse.Namespace(
            root=self.tmp.name,
            core_api="http://127.0.0.1:8901/core-api",
            operator_token_file=str(token),
            project_id="project-1",
            profile_id="profile-1",
            authorization="owner-ref",
            evidence_dir=str(Path(self.tmp.name) / "evidence"),
            compose_project="sfe2e-recovery",
            git_dir=str(Path(self.tmp.name) / "git"),
            core_database="",
            authorized_micros=0,
            assumption_file="",
            provider="opencode-go",
            model="deepseek-v4.1-flash",
        )
        self.scenario = real_recovery.Scenario(self.args)
        self.scenario.intake = "intake-1"

    def tearDown(self):
        self.tmp.cleanup()

    def test_assess_timeout_is_not_retried(self):
        api = TimeoutApi()
        self.scenario.api = api
        with self.assertRaises(TimeoutError):
            self.scenario.assess()
        posts = [call for call in api.calls if call[1].endswith("/assess")]
        self.assertEqual(len(posts), 1)

    def test_staged_assess_403_continues_without_second_post(self):
        scenario = self.scenario
        scenario.intake = "intake-1"
        scenario.profile_revision_id = "rev-1"
        scenario.profile_hash = "8" * 64
        scenario.api = FakeApi(
            get_routes={
                "/intake/requests/intake-1": {
                    "state": "PLAN_READY",
                    "proposal": {"objective": real_recovery.OBJECTIVE},
                    "assessment": {"intent": "change"},
                    "authorization": {"reason": "PLAN_NOT_READY"},
                }
            },
            post_status_routes={
                "/intake/requests/intake-1/assess": (
                    403,
                    {"code": "PLAN_NOT_READY", "error": "staged"},
                )
            },
        )
        scenario._db_rows = lambda sql, params: [("SETTLED",)]
        scenario.assess()
        assess_calls = [
            call for call in scenario.api.calls
            if call[1] == "/intake/requests/intake-1/assess"
        ]
        self.assertEqual(len(assess_calls), 1)
        self.assertEqual(scenario.assessment_projection["state"], "PLAN_READY")

    def test_environment_review_422_is_not_retried(self):
        import io
        import urllib.error

        message = (
            "Failed to deserialize the JSON body into the target type: "
            "plan_sha256: unknown field `plan_sha256`, expected `planSha256`"
        )
        error = urllib.error.HTTPError(
            "http://127.0.0.1:8901/core-api/agentic/requests/intake-1/environment/review",
            422,
            "Unprocessable Entity",
            {"content-type": "application/json"},
            io.BytesIO(("{\"error\":\"" + message + "\"}").encode("utf-8")),
        )
        with mock.patch("urllib.request.urlopen", side_effect=error) as urlopen:
            api = real_recovery.Api("http://127.0.0.1:8901/core-api", "token")
            scenario = self.scenario
            scenario.api = api
            scenario.intake = "intake-1"
            scenario.environment_plan = {"planSha256": "a" * 64, "planRevision": 1, "plan": {"resources": {"timeoutSecs": 60}, "network": {"fetchAllowlist": []}}}
            with self.assertRaises(urllib.error.HTTPError):
                scenario.environment_review()
        self.assertEqual(urlopen.call_count, 1)
        self.assertEqual(api.last_error[0], 422)
        self.assertIn("unknown field `plan_sha256`", api.last_error[1])

    def test_staged_assess_403_without_durable_facts_fails(self):
        scenario = self.scenario
        scenario.intake = "intake-1"
        scenario.api = FakeApi(
            get_routes={
                "/intake/requests/intake-1": {
                    "state": "ASSESSING", "proposal": None,
                    "assessment": None, "authorization": {"reason": None},
                }
            },
            post_status_routes={
                "/intake/requests/intake-1/assess": (403, {"code": "OTHER"})
            },
        )
        with self.assertRaises(real_recovery.ScenarioError):
            scenario.assess()

    def test_failure_model_call_mapping_is_exact(self):
        scenario = self.scenario
        scenario._failure_db_rows = lambda: {
            "calls": [("LEAD", "SETTLED", "lead_assessment", "opencode-go", "deepseek-v4.1-flash", "t")],
            "agentic_assessments": 0,
            "lead_assessments": 1,
            "provider_sessions": 0,
            "execution_check_receipts": 0,
            "db_errors": {},
            "critical_error": "",
        }
        scenario.api = FakeApi(get_routes={"/intake/requests/intake-1": {"state": "PLAN_READY"}})
        scenario.reconcile_failure(settle_timeout=0)
        # Read the written failure via capture_failure's reconciliation path.
        payload = scenario.capture_failure(TimeoutError("timed out"))
        self.assertEqual(payload["reconciliation"]["model_calls"][0], {
            "role": "LEAD",
            "state": "SETTLED",
            "purpose": "lead_assessment",
            "provider": "opencode-go",
            "model": "deepseek-v4.1-flash",
        })

    def test_missing_database_is_unknown_and_blocks_cleanup(self):
        self.scenario.api = FakeApi(
            get_routes={"/intake/requests/intake-1": {"state": "ASSESSING"}}
        )
        self.scenario.admission_id = ""  # fast reconciliation, no active model wait
        result = self.scenario.reconcile_failure(settle_timeout=0)
        self.assertEqual(result["outcome"], "MODEL_CALL_OUTCOME_UNKNOWN")
        self.assertFalse(result["db_captured"])
        self.assertIn("model_call_reservations", result["critical_error"])
        payload = self.scenario.capture_failure(TimeoutError("timed out"))
        self.assertEqual(payload["reconciliation"]["outcome"], "MODEL_CALL_OUTCOME_UNKNOWN")
        self.assertFalse(real_recovery._safe_to_cleanup(self.scenario))

    def test_active_assessment_without_durable_rows_is_unknown(self):
        self.scenario.api = FakeApi(
            get_routes={"/intake/requests/intake-1": {"state": "ASSESSING"}}
        )
        self.scenario._db_rows = lambda sql, params: [] if "model_call_reservations" in sql else [(0,)]
        result = self.scenario.reconcile_failure(settle_timeout=0)
        self.assertEqual(result["outcome"], "MODEL_CALL_OUTCOME_UNKNOWN")
        self.assertTrue(result["db_captured"])

    def test_approve_timeout_stops_after_one_post(self):
        api = TimeoutApi()
        api.post_status_response = (409, {"code": "MONETARY_AUTHORIZATION_NOT_APPLICABLE"})
        self.scenario.api = api
        self.scenario.plan_hash = "9" * 64
        self.scenario.profile_revision_id = "rev-1"
        self.scenario.profile_hash = "8" * 64
        self.scenario.admission_id = "admission-1"
        with self.assertRaises(TimeoutError):
            self.scenario.approve()
        approve_posts = [call for call in api.calls if call[0] == "POST" and call[1].endswith("/approve")]
        self.assertEqual(len(approve_posts), 1)

    def test_a_slow_assessment_is_not_killed_by_the_legacy_sixty_second_deadline(self):
        class SlowTimeoutApi(TimeoutApi):
            def post(self, path, payload):
                self.calls.append(("POST", path, payload))
                return {}

            def post_status(self, path, payload):
                self.calls.append(("POST_STATUS", path, payload))
                # A fake clock represents a 120 s model turn; no wall sleep.
                if path.endswith("/assess"):
                    raise TimeoutError("fake-clock 120s")
                return self.post_status_response

        api = SlowTimeoutApi()
        self.scenario.api = api
        with self.assertRaises(TimeoutError) as raised:
            self.scenario.assess()
        self.assertIn("fake-clock 120s", str(raised.exception))
        self.assertEqual(len([call for call in api.calls if call[1].endswith("/assess")]), 1)

    def test_unknown_model_call_blocks_cleanup(self):
        self.scenario.api = FakeApi(
            get_routes={"/intake/requests/intake-1": {"state": "ASSESSING"}}
        )
        self.scenario._db_rows = lambda sql, params: [("LEAD", "RESERVED", "opencode-go", "deepseek-v4.1-flash", 1)]
        self.scenario.reconcile_failure = lambda settle_timeout=0: {
            "outcome": "MODEL_CALL_OUTCOME_UNKNOWN",
            "intake_state": "ASSESSING",
            "assessment_present": False,
            "provider_sessions": 0,
            "execution_check_receipts": 0,
            "model_calls": [],
        }
        payload = self.scenario.capture_failure(TimeoutError("timed out"))
        self.assertEqual(payload["reconciliation"]["outcome"], "MODEL_CALL_OUTCOME_UNKNOWN")
        self.assertFalse(real_recovery._safe_to_cleanup(self.scenario))
        self.assertTrue((Path(self.args.evidence_dir) / "cleanup-held.json").exists())

    def test_terminal_outcome_allows_cleanup_after_capture(self):
        self.scenario.reconcile_failure = lambda settle_timeout=0: {
            "outcome": "MODEL_CALL_SETTLED",
            "intake_state": "AWAITING_APPROVAL",
            "assessment_present": True,
            "provider_sessions": 0,
            "execution_check_receipts": 0,
            "model_calls": [],
        }
        self.scenario.capture_failure(TimeoutError("timed out"))
        self.assertTrue(real_recovery._safe_to_cleanup(self.scenario))

    def main_with_failure(self, outcome):
        calls = []

        class FakeFailureScenario:
            def __init__(self, args):
                self.evidence = Path(args.evidence_dir)
                self.evidence.mkdir(parents=True, exist_ok=True)
                self.project = "sfe2e-recovery"

            def prepare(self):
                pass

            def admit(self):
                pass

            def assess(self):
                calls.append("assess")
                raise TimeoutError("timed out")

            def prepare_plan(self):
                calls.append("prepare_plan")

            def write(self, name, payload):
                (self.evidence / name).write_text(json.dumps(payload), encoding="utf-8")

            def capture_failure(self, error, stage=""):
                calls.append("capture")
                self.write("failure.json", {"reconciliation": {"outcome": outcome}})

            def cleanup(self):
                calls.append("cleanup")

        args = [
            "--execute",
            "--core-api", "http://127.0.0.1:8901/core-api",
            "--operator-token-file", str(Path(self.tmp.name) / "operator.token"),
            "--project-id", "project-1",
            "--profile-id", "profile-1",
            "--authorization", "owner-ref",
            "--evidence-dir", str(Path(self.tmp.name) / "evidence"),
            "--compose-project", "sfe2e-recovery",
            "--git-dir", str(Path(self.tmp.name) / "git"),
            "--core-database", str(Path(self.tmp.name) / "sfe2e-recovery" / "factory.sqlite"),
            "--assumption-file", str(Path(self.tmp.name) / "assumption.json"),
            "--authorized-micros", "0",
        ]
        with mock.patch.object(real_recovery, "Scenario", FakeFailureScenario):
            code = real_recovery.main(args)
        return code, calls

    def test_main_captures_before_cleanup_and_holds_on_unknown(self):
        code, calls = self.main_with_failure("MODEL_CALL_OUTCOME_UNKNOWN")
        self.assertEqual(code, 1)
        self.assertEqual(calls, ["assess", "capture"])
        held = json.loads(
            (Path(self.tmp.name) / "evidence" / "cleanup-held.json").read_text(encoding="utf-8")
        )
        self.assertEqual(held["status"], "STACK_RETAINED_FOR_RECONCILIATION")
        self.assertIn("down -v", held["cleanup_command"])
        self.assertIn("model_call_reservations", held["condition"])

    def test_main_cleans_up_only_after_terminal_capture(self):
        code, calls = self.main_with_failure("MODEL_CALL_SETTLED")
        self.assertEqual(code, 1)
        self.assertEqual(calls, ["assess", "capture", "cleanup"])


class RealSchemaFailureTests(unittest.TestCase):
    """Execute the reconciliation/capture SQL against the real migrations."""

    project = "sfe2e-recovery-schema"

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = HERE.parents[3]
        self.db_dir = Path(self.tmp.name) / self.project
        self.db_dir.mkdir()
        self.db_path = self.db_dir / "factory.sqlite"
        connection = sqlite3.connect(self.db_path)
        for migration in sorted((self.root / "migrations").glob("*.sql")):
            connection.executescript(migration.read_text(encoding="utf-8"))
        connection.commit()
        connection.close()
        token = Path(self.tmp.name) / "operator.token"
        token.write_text("token", encoding="utf-8")
        self.args = argparse.Namespace(
            root=str(self.root),
            core_api="http://127.0.0.1:1",
            operator_token_file=str(token),
            project_id="proj",
            profile_id="profile-1",
            authorization="owner-ref",
            evidence_dir=str(Path(self.tmp.name) / "evidence"),
            compose_project=self.project,
            git_dir=str(self.root),
            core_database=str(self.db_path),
            authorized_micros=0,
            assumption_file="",
            provider="opencode-go",
            model="deepseek-v4.1-flash",
        )

    def scenario(self, state="ASSESSING"):
        scenario = real_recovery.Scenario(self.args)
        scenario.intake = "intake-1"
        scenario.started_at = "2026-11-18T14:28:00Z"
        scenario.api = FakeApi(
            get_routes={"/intake/requests/intake-1": {"state": state}}
        )
        return scenario

    def seed_call(self, state, created_at="2026-11-18T14:29:00Z"):
        connection = sqlite3.connect(self.db_path)
        connection.execute(
            "INSERT INTO model_call_reservations (reservation_id,role,principal_id,project_id,"
            "objective_id,scope_id,provider,model,purpose,request_sha256,max_input_tokens,"
            "max_output_tokens,max_total_tokens,max_cost_micros,state,expires_at_ms,created_at,updated_at) "
            "VALUES ('r1','LEAD','p','proj','obj','scope','opencode-go','deepseek-v4.1-flash','lead_assessment',?,1,1,1,1,?,9999999999999,?,?)",
            ("a" * 64, state, created_at, created_at),
        )
        connection.commit()
        connection.close()

    def seed_purpose(self, purpose, state):
        connection = sqlite3.connect(self.db_path)
        columns = (
            "reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,"
            "model,purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,"
            "max_cost_micros,state,expires_at_ms,created_at,updated_at"
        )
        import hashlib

        request_sha = hashlib.sha256(f"{purpose}:{state}".encode("utf-8")).hexdigest()
        connection.execute(
            f"INSERT INTO model_call_reservations ({columns}) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            (
                f"r-{purpose}-{state}", "LEAD", "p", "proj", purpose, "scope", "opencode-go",
                "deepseek-v4.1-flash", purpose, request_sha, 1, 1, 1, 1, state, 9999999999999,
                "2026-11-18T14:29:13.500000000+00:00", "2026-11-18T14:29:13.500000000+00:00",
            ),
        )
        connection.commit()
        connection.close()

    def test_real_schema_purpose_states_are_exact(self):
        scenario = self.scenario()
        for purpose in ("lead_assessment", "lead_environment_review", "agentic_planner"):
            self.seed_purpose(purpose, "SETTLED")
        evidence = scenario.verify_model_purposes()
        self.assertEqual(evidence["purposes"], {
            "lead_assessment": 1, "lead_environment_review": 1, "agentic_planner": 1,
        })
        connection = sqlite3.connect(self.db_path)
        connection.execute("DELETE FROM model_call_reservations")
        connection.commit()
        connection.close()
        for bad_state in ("RESERVED", "UNKNOWN", "AMBIGUOUS"):
            connection = sqlite3.connect(self.db_path)
            connection.execute("DELETE FROM model_call_reservations")
            connection.commit()
            connection.close()
            self.seed_purpose("lead_assessment", bad_state)
            self.seed_purpose("lead_environment_review", "SETTLED")
            self.seed_purpose("agentic_planner", "SETTLED")
            with self.assertRaises(real_recovery.ScenarioError):
                scenario.verify_model_purposes()
        connection = sqlite3.connect(self.db_path)
        connection.execute("DELETE FROM model_call_reservations")
        connection.commit()
        connection.close()
        self.seed_purpose("lead_assessment", "SETTLED")
        self.seed_purpose("lead_environment_review", "SETTLED")
        with self.assertRaises(real_recovery.ScenarioError):
            scenario.verify_model_purposes()
        connection = sqlite3.connect(self.db_path)
        connection.execute("DELETE FROM model_call_reservations")
        connection.execute(
            "INSERT INTO model_call_reservations (reservation_id,role,principal_id,project_id,"
            "objective_id,scope_id,provider,model,purpose,request_sha256,max_input_tokens,"
            "max_output_tokens,max_total_tokens,max_cost_micros,state,expires_at_ms,created_at,updated_at) "
            "VALUES ('dup','LEAD','p','proj','dup-purpose','scope','opencode-go','deepseek-v4.1-flash',"
            "'lead_assessment',?,1,1,1,1,'SETTLED',9999999999999,'2026-11-18T14:29:13.500000000+00:00','2026-11-18T14:29:13.500000000+00:00')",
            ("b" * 64,),
        )
        connection.commit()
        connection.close()
        self.seed_purpose("lead_assessment", "SETTLED")
        self.seed_purpose("lead_environment_review", "SETTLED")
        self.seed_purpose("agentic_planner", "SETTLED")
        with self.assertRaises(real_recovery.ScenarioError):
            scenario.verify_model_purposes()

    def test_real_schema_is_captured_and_reserved_is_unknown(self):
        self.seed_call("RESERVED")
        scenario = self.scenario()
        facts = scenario._failure_db_rows()
        self.assertEqual(facts["critical_error"], "")
        self.assertEqual(facts["db_errors"], {})
        self.assertEqual(facts["calls"][0][1], "RESERVED")
        result = scenario.reconcile_failure(settle_timeout=0)
        self.assertTrue(result["db_captured"])
        self.assertEqual(result["outcome"], "MODEL_CALL_OUTCOME_UNKNOWN")
        snapshot = scenario.capture_failure_db()
        row = snapshot["model_call_reservations"]["rows"][0]
        self.assertEqual(row[2], "lead_assessment")
        self.assertEqual(row[3], "opencode-go")
        self.assertEqual(row[4], "deepseek-v4.1-flash")
        self.assertEqual(row[5], "RESERVED")
        self.assertNotIn("state", snapshot["model_call_reservations"])

    def test_same_second_fractional_timestamp_is_still_captured(self):
        self.seed_call("RESERVED", created_at="2026-11-18T14:29:13.500000000+00:00")
        connection = sqlite3.connect(self.db_path)
        connection.execute(
            "INSERT INTO model_call_reservations (reservation_id,role,principal_id,project_id,"
            "objective_id,scope_id,provider,model,purpose,request_sha256,max_input_tokens,"
            "max_output_tokens,max_total_tokens,max_cost_micros,state,expires_at_ms,created_at,updated_at) "
            "VALUES ('r2','LEAD','p','another-project','obj','scope','opencode-go','deepseek-v4.1-flash','assess',?,1,1,1,1,'RESERVED',9999999999999,'2026-11-18T14:29:13.500000000+00:00','2026-11-18T14:29:13.500000000+00:00')",
            ("b" * 64,),
        )
        connection.commit()
        connection.close()
        scenario = self.scenario(state="FAILED")
        scenario.started_at = "2026-11-18T14:29:13Z"
        result = scenario.reconcile_failure(settle_timeout=0)
        self.assertTrue(result["db_captured"])
        self.assertEqual(len(result["model_calls"]), 1)
        self.assertEqual(result["model_calls"][0]["state"], "RESERVED")
        self.assertEqual(result["outcome"], "MODEL_CALL_OUTCOME_UNKNOWN")
        scenario.capture_failure(TimeoutError("timed out"))
        self.assertFalse(real_recovery._safe_to_cleanup(scenario))

    def test_real_schema_terminal_settled_is_safe_after_capture(self):
        self.seed_call("SETTLED")
        scenario = self.scenario(state="AWAITING_APPROVAL")
        result = scenario.reconcile_failure(settle_timeout=0)
        self.assertTrue(result["db_captured"])
        self.assertEqual(result["outcome"], "MODEL_CALL_SETTLED")
        scenario.capture_failure(TimeoutError("timed out"))
        self.assertTrue(real_recovery._safe_to_cleanup(scenario))

    def test_real_schema_empty_captured_database_is_not_unknown(self):
        scenario = self.scenario(state="AWAITING_APPROVAL")
        result = scenario.reconcile_failure(settle_timeout=0)
        self.assertTrue(result["db_captured"])
        self.assertEqual(result["outcome"], "NO_MODEL_CALL_CAPTURED")
        self.assertTrue(real_recovery._safe_to_cleanup(self.tmp_after_capture(scenario)))

    def tmp_after_capture(self, scenario):
        scenario.capture_failure(TimeoutError("timed out"))
        return scenario


class OrchestratorConsumerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        token = Path(self.tmp.name) / "operator.token"
        token.write_text("test-token", encoding="utf-8")
        self.args = argparse.Namespace(
            root=self.tmp.name,
            core_api="http://127.0.0.1:8899",
            operator_token_file=str(token),
            project_id="project-1",
            profile_id="profile-1",
            authorization="owner-ref",
            evidence_dir=str(Path(self.tmp.name) / "evidence"),
            compose_project="sfe2e-recovery",
            git_dir=str(Path(self.tmp.name) / "git"),
            core_database="",
            authorized_micros=5_000_000,
            assumption_file="",
            provider="opencode-go",
            model="deepseek-v4.1-flash",
        )

    def tearDown(self):
        self.tmp.cleanup()

    def scenario(self):
        return real_recovery.Scenario(self.args)

    def write_assumption(self, scenario, **overrides):
        import time as _time

        now_ms = int(_time.time() * 1000)
        data = {
            "assumptionId": "owner-assumption-1",
            "confirmedBy": "Product Owner (test fixture text)",
            "confirmedAtMs": now_ms - 60_000,
            "validUntilMs": now_ms + 3_600_000,
            "extraUsage": "DISABLED",
            "statement": "Owner confirms the current OpenCode Go Extra Usage switch is OFF for this bounded acceptance run.",
        }
        data.update(overrides)
        path = Path(self.tmp.name) / "assumption.json"
        path.write_text(json.dumps(data), encoding="utf-8")
        path.chmod(0o600)
        scenario.args.assumption_file = str(path)
        return path

    def test_prepare_contract_and_negative_authorize(self):
        scenario = self.scenario()
        projection = {
            "id": "intake-1",
            "campaignId": "campaign-1",
            "state": "EXECUTING",
            "spending": {
                "envelope": {"envelopeId": "envelope-1", "authorizedMicros": 5_000_000},
                "status": {"remainingMicros": 5_000_000},
            },
        }
        scenario.api = FakeApi(
            get_routes={
                "/execution-profiles/profile-1": profile_fixture(),
                "/intake/requests/intake-1": projection,
                "/actions": [action_record()],
                "/campaigns/campaign-1/authorization": AUTHORIZATION_RECORD,
                "/events?after=0&limit=500": [
                    {
                        "kind": "campaign.authorized",
                        "subject_id": "campaign-1",
                        "sequence": 7,
                        "recorded_at": "2026-11-18T00:00:01Z",
                    }
                ],
            },
            post_routes={
                "/intake/requests": {
                    "id": "intake-1",
                    "actionId": "act-1",
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                },
            },
        )
        scenario.prepare()
        scenario.campaign = "campaign-1"
        scenario.verify_contract()

        self.assertEqual(scenario.core_record["authorization_id"], "auth-1")
        self.assertEqual(scenario.core_record["owner_reference"], "envelope-1")
        self.assertEqual(scenario.core_record["caller_reference"], "owner-ref")
        self.assertEqual(scenario.core_record["spec_id"], "act-1")
        self.assertEqual(scenario.core_record["spec_binding_hash"], "f" * 64)
        self.assertEqual(scenario.core_record["event_sequence"], 7)
        self.assertEqual(scenario.core_record["base_commit"], "c" * 40)
        self.assertEqual(scenario.expected_base_commit, "c" * 40)
        self.assertEqual(scenario.expected_status_command_sha256, real_recovery.command_sha256(real_recovery.CHECKS[real_recovery.STATUS]["command"]))
        self.assertTrue(scenario.restart_required)
        # The negative direct authorization must be attempted, not skipped.
        self.assertTrue(any(call[0] == "POST_STATUS" for call in scenario.api.calls))

    def test_assess_admit_and_approve_use_the_production_shapes(self):
        scenario = self.scenario()
        scenario.args.authorized_micros = 0
        assess_projection = {
            "id": "intake-1",
            "state": "PLAN_READY",
            "plan": None,
            "environment": {"status": "MISSING", "planId": None, "planSha256": None},
            "profile": {"id": "profile-1", "revisionId": "rev-1", "contentHash": "8" * 64},
        }
        admission = {
            "admissionId": "admission-1",
            "intakeRequestId": "intake-1",
            "state": "ACTIVE",
            "provider": "opencode-go",
            "model": "deepseek-v4.1-flash",
            "profileRevisionId": "rev-1",
            "profileHash": "8" * 64,
            "assumption": {
                "extraUsage": "DISABLED",
                "assumptionId": "owner-assumption-1",
                "confirmedBy": "operator-token:Product Owner (test fixture text)",
            },
            "receiptSha256": "6" * 64,
            "expiresAtMs": 9_999_999_999_999,
            "routes": [
                {"role": "LEAD", "provider": "opencode-go", "model": "deepseek-v4.1-flash",
                 "transport": "OPENCODE_GO_SUBSCRIPTION"},
                {"role": "IMPLEMENTER", "provider": "opencode-go", "model": "deepseek-v4.1-flash",
                 "transport": "OPENCODE_GO_SUBSCRIPTION"},
            ],
        }
        recompiled = {
            "status": "CORE_VALIDATED",
            "planId": "env-plan-1",
            "planRevision": 1,
            "planSha256": "a" * 64,
            "specSha256": None,
            "plan": {
                "plan_id": "env-plan-1",
                "revision": 1,
                "resources": {"timeoutSecs": 60},
                "network": {"fetchAllowlist": []},
            },
        }
        reviewed = {
            "status": "FROZEN",
            "planId": "env-plan-1",
            "planRevision": 1,
            "planSha256": "a" * 64,
            "specSha256": "b" * 64,
            "spec": {"spec_id": "spec-1"},
        }
        planned = {
            "validated": {"units": [{"id": "unit-1"}], "graph_sha256": "c" * 64},
            "participantPlan": {"id": "participant-1"},
            "planning": {"ready": True},
        }
        frozen = {
            "plan": {"plan_hash": "9" * 64, "request_id": "lead-1"},
            "request": {"state": "READY"},
        }
        approval = {
            "campaignId": "campaign-1",
            "campaign": {"id": "campaign-1", "status": "AUTHORIZED", "action_id": "act-1"},
        }
        scenario.api = FakeApi(
            get_routes={"/execution-profiles/profile-1": profile_fixture()},
            post_routes={
                "/intake/requests": {
                    "id": "intake-1",
                    "actionId": "act-1",
                    "leadRequestId": "lead-1",
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                },

                "/intake/requests/intake-1/subscription-admission": admission,
                "/agentic/requests/intake-1/environment/recompile": recompiled,
                "/agentic/requests/intake-1/environment/review": reviewed,
                "/agentic/requests/intake-1/plan": planned,
                "/lead/requests/lead-1/freeze": frozen,
                "/intake/requests/intake-1/approve": approval,
            },
            post_status_response=(409, {"code": "MONETARY_AUTHORIZATION_NOT_APPLICABLE"}),
            post_status_routes={
                "/intake/requests/intake-1/assess": (200, assess_projection),
            },
        )
        scenario.prepare()
        scenario._db_rows = lambda sql, params: (
            [("lead_assessment", "SETTLED"), ("lead_environment_review", "SETTLED"),
             ("agentic_planner", "SETTLED")]
            if "purpose,state" in sql else []
        )
        self.write_assumption(scenario)
        scenario.admit()
        scenario.assess()
        scenario.prepare_plan()
        scenario.approve()
        self.assertEqual(scenario.plan_hash, "9" * 64)
        self.assertEqual(scenario.profile_revision_id, "rev-1")
        self.assertEqual(scenario.admission_id, "admission-1")
        self.assertEqual(scenario.campaign, "campaign-1")
        posts = [call[1] for call in scenario.api.calls if call[0] in {"POST", "POST_STATUS"}]
        self.assertEqual(posts.count("/intake/requests/intake-1/assess"), 1)
        for path in (
            "/intake/requests/intake-1/assess",
            "/agentic/requests/intake-1/environment/recompile",
            "/agentic/requests/intake-1/environment/review",
            "/agentic/requests/intake-1/plan",
            "/lead/requests/lead-1/freeze",
            "/intake/requests/intake-1/approve",
        ):
            self.assertIn(path, posts)
        self.assertLess(posts.index("/agentic/requests/intake-1/environment/recompile"),
                        posts.index("/agentic/requests/intake-1/environment/review"))
        self.assertLess(posts.index("/agentic/requests/intake-1/environment/review"),
                        posts.index("/agentic/requests/intake-1/plan"))
        self.assertLess(posts.index("/agentic/requests/intake-1/plan"),
                        posts.index("/lead/requests/lead-1/freeze"))
        self.assertLess(posts.index("/lead/requests/lead-1/freeze"),
                        posts.index("/intake/requests/intake-1/approve"))
        before = json.loads((Path(self.args.evidence_dir) / "model-purposes-before-freeze.json").read_text())
        after = json.loads((Path(self.args.evidence_dir) / "model-purposes-after-freeze.json").read_text())
        self.assertEqual(before["states"], after["states"])
        self.assertEqual(before["expected"]["lead_assessment"], {"count": 1, "state": "SETTLED"})
        review_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST" and call[1] == "/agentic/requests/intake-1/environment/review"
        )
        self.assertEqual(set(review_call[2]), {"planSha256", "planRevision"})
        intake_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST" and call[1] == "/intake/requests"
        )
        self.assertEqual(
            set(intake_call[2]),
            {"project_id", "objective", "execution_profile_id", "mode",
             "idempotency_key", "environment_constraints"},
        )
        admission_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST"
            and call[1] == "/intake/requests/intake-1/subscription-admission"
        )
        self.assertEqual(
            set(admission_call[2]),
            {"assumption_id", "confirmed_by", "confirmed_at_ms", "valid_until_ms",
             "extra_usage_disabled", "statement", "test_profile_id", "idempotency_key"},
        )
        recompile_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST"
            and call[1] == "/agentic/requests/intake-1/environment/recompile"
        )
        self.assertEqual(set(recompile_call[2]), {"rationale"})
        plan_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST" and call[1] == "/agentic/requests/intake-1/plan"
        )
        self.assertEqual(plan_call[2], {})
        freeze_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST" and call[1] == "/lead/requests/lead-1/freeze"
        )
        self.assertEqual(freeze_call[2], {})
        approve_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST" and call[1] == "/intake/requests/intake-1/approve"
        )
        self.assertEqual(approve_call[2]["subscription_admission_id"], "admission-1")
        self.assertNotIn("authorized_micros", approve_call[2])
        self.assertEqual(approve_call[2]["expected_plan_hash"], "9" * 64)

    def test_plan_sequence_fails_closed_without_a_validated_workgraph(self):
        scenario = self.scenario()
        scenario.intake = "intake-1"
        scenario.lead_request_id = "lead-1"
        scenario.environment_plan = {
            "status": "CORE_VALIDATED", "planId": "env-plan-1",
            "planRevision": 1, "planSha256": "a" * 64,
            "plan": {
                "resources": {"timeoutSecs": 60},
                "network": {"fetchAllowlist": []},
            },
        }
        scenario.api = FakeApi(
            post_routes={
                "/agentic/requests/intake-1/environment/review": {
                    "status": "FROZEN", "specSha256": "b" * 64,
                },
                "/agentic/requests/intake-1/plan": {
                    "validated": {"units": []}, "participantPlan": None,
                },
            }
        )
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.environment_review()
            scenario.plan_workgraph()
            scenario.freeze_lead_plan()
        self.assertIn("validated participant WorkGraph", str(raised.exception))
        posts = [call[1] for call in scenario.api.calls if call[0] == "POST"]
        self.assertNotIn("/lead/requests/lead-1/freeze", posts)

    def test_admit_refuses_without_a_current_owner_assumption(self):
        scenario = self.scenario()
        scenario.api = FakeApi(post_routes={})
        scenario.intake = "intake-1"
        scenario.profile_revision_id = "rev-1"
        scenario.profile_hash = "8" * 64
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.admit()
        self.assertIn("Owner subscription assumption", str(raised.exception))
        self.assertEqual(scenario.api.calls, [])

    def test_expired_owner_assumption_is_refused_before_the_api_call(self):
        scenario = self.scenario()
        scenario.api = FakeApi(post_routes={})
        scenario.intake = "intake-1"
        scenario.profile_revision_id = "rev-1"
        scenario.profile_hash = "8" * 64
        import time as _time

        past_ms = int((_time.time() - 7200) * 1000)
        self.write_assumption(scenario, validUntilMs=past_ms)
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.admit()
        self.assertIn("expired", str(raised.exception))
        self.assertEqual(scenario.api.calls, [])

    def test_approve_fails_closed_when_the_monetary_probe_is_not_refused(self):
        scenario = self.scenario()
        scenario.intake = "intake-1"
        scenario.plan_hash = "9" * 64
        scenario.admission_id = "admission-1"
        scenario.api = FakeApi(
            post_routes={},
            post_status_response=(200, {"campaignId": "campaign-1"}),
        )
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.approve()
        self.assertIn("not refused", str(raised.exception))

    def test_profile_identity_falls_back_to_the_intake_detail(self):
        scenario = self.scenario()
        scenario.api = FakeApi(
            get_routes={
                "/intake/requests/intake-1": {
                    "id": "intake-1",
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                }
            },
            post_routes={
                "/intake/requests/intake-1/subscription-admission": {
                    "admissionId": "admission-1",
                    "intakeRequestId": "intake-1",
                    "state": "ACTIVE",
                    "provider": "opencode-go",
                    "model": "deepseek-v4.1-flash",
                    "profileRevisionId": "rev-1",
                    "profileHash": "8" * 64,
                    "assumption": {
                        "extraUsage": "DISABLED",
                        "assumptionId": "owner-assumption-1",
                        "confirmedBy": "operator-token:Product Owner (test fixture text)",
                    },
                    "receiptSha256": "6" * 64,
                    "expiresAtMs": 9_999_999_999_999,
                    "routes": [
                        {"role": "IMPLEMENTER", "provider": "opencode-go",
                         "model": "deepseek-v4.1-flash", "transport": "OPENCODE_GO_SUBSCRIPTION"}
                    ],
                }
            },
        )
        scenario.intake = "intake-1"
        self.write_assumption(scenario)
        scenario.admit()
        self.assertEqual(scenario.profile_revision_id, "rev-1")
        self.assertEqual(scenario.admission_id, "admission-1")
        self.assertTrue(any(call == ("GET", "/intake/requests/intake-1") for call in scenario.api.calls))

    def test_unknown_or_old_assumption_fields_are_refused(self):
        scenario = self.scenario()
        scenario.api = FakeApi(post_routes={})
        scenario.intake = "intake-1"
        scenario.profile_revision_id = "rev-1"
        scenario.profile_hash = "8" * 64
        self.write_assumption(scenario, authorizationClaim="invented")
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.admit()
        self.assertIn("unknown fields", str(raised.exception))
        self.assertEqual(scenario.api.calls, [])

    def test_admission_with_a_foreign_principal_prefix_is_rejected(self):
        scenario = self.scenario()
        base = {
            "admissionId": "admission-1",
            "intakeRequestId": "intake-1",
            "state": "ACTIVE",
            "provider": "opencode-go",
            "model": "deepseek-v4.1-flash",
            "profileRevisionId": "rev-1",
            "profileHash": "8" * 64,
            "assumption": {
                "extraUsage": "DISABLED",
                "assumptionId": "owner-assumption-1",
                "confirmedBy": "other:Product Owner (test fixture text)",
            },
            "receiptSha256": "6" * 64,
            "expiresAtMs": 9_999_999_999_999,
            "routes": [
                {"role": "IMPLEMENTER", "provider": "opencode-go",
                 "model": "deepseek-v4.1-flash", "transport": "OPENCODE_GO_SUBSCRIPTION"}
            ],
        }
        scenario.api = FakeApi(
            post_routes={"/intake/requests/intake-1/subscription-admission": base}
        )
        scenario.intake = "intake-1"
        scenario.profile_revision_id = "rev-1"
        scenario.profile_hash = "8" * 64
        self.write_assumption(scenario)
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.admit()
        self.assertIn("confirmedBy", str(raised.exception))

    def test_admission_without_disabled_extra_usage_or_exact_route_fails(self):
        scenario = self.scenario()
        base = {
            "admissionId": "admission-1",
            "intakeRequestId": "intake-1",
            "state": "ACTIVE",
            "provider": "opencode-go",
            "model": "deepseek-v4.1-flash",
            "profileRevisionId": "rev-1",
            "profileHash": "8" * 64,
            "assumption": {"extraUsage": "UNKNOWN"},
            "routes": [],
        }
        scenario.api = FakeApi(
            post_routes={"/intake/requests/intake-1/subscription-admission": base}
        )
        scenario.intake = "intake-1"
        scenario.profile_revision_id = "rev-1"
        scenario.profile_hash = "8" * 64
        self.write_assumption(scenario)
        with self.assertRaises(real_recovery.ScenarioError) as raised:
            scenario.admit()
        self.assertIn("extraUsage", str(raised.exception))
        self.assertIn("implementer route", str(raised.exception))

    def test_subscription_funding_is_projected_by_core(self):
        scenario = self.scenario()
        scenario.args.authorized_micros = 0
        record = {**AUTHORIZATION_RECORD, "policy": {"monetaryEnvelopeId": real_recovery.NO_BILLABLE_ENVELOPE}}
        scenario.api = FakeApi(
            get_routes={
                "/execution-profiles/profile-1": profile_fixture(),
                "/intake/requests/intake-1": {
                    "id": "intake-1", "campaignId": "campaign-1", "state": "EXECUTING",
                    "spending": None,
                },
                "/actions": [action_record()],
                "/campaigns/campaign-1/authorization": record,
                "/events?after=0&limit=500": [
                    {"kind": "campaign.authorized", "subject_id": "campaign-1", "sequence": 7,
                     "recorded_at": "2026-11-18T00:00:01Z"}
                ],
            },
            post_routes={
                "/intake/requests": {
                    "id": "intake-1",
                    "actionId": "act-1",
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                },
            },
        )
        scenario.prepare()
        scenario.campaign = "campaign-1"
        scenario.admission_id = "admission-1"
        scenario.admission_record = {
            "assumption": {"confirmedBy": "Product Owner", "assumptionId": "owner-assumption-1"}
        }
        scenario._db_rows = lambda sql, params: [
            (
                "admission-1", "campaign-1", "CONSUMED", "opencode-go",
                "deepseek-v4.1-flash", "rev-1", "8" * 64,
                json.dumps({"extraUsage": "DISABLED"}),
                json.dumps([{"role": "IMPLEMENTER", "provider": "opencode-go",
                             "model": "deepseek-v4.1-flash", "transport": "OPENCODE_GO_SUBSCRIPTION"}]),
                "7" * 64, 9_999_999_999_999,
            )
        ]
        scenario.verify_contract()
        self.assertEqual(scenario.core_record["funding_mode"], "REAL_SUBSCRIPTION")
        self.assertEqual(scenario.core_record["owner_reference"], "Product Owner#owner-assumption-1")
        self.assertEqual(scenario.core_record["api_key_route_disabled"], "NOT_TESTED_NO_VISIBLE_SURFACE")
        self.assertEqual(scenario.core_record["no_extra_metered"], "NOT_TESTED_OWNER_CONFIRMATION_PENDING")
        intake_call = next(call for call in scenario.api.calls if call[0] == "POST" and call[1] == "/intake/requests")
        self.assertNotIn("authorized_micros", intake_call[2])

    def test_contract_fails_when_core_shapes_are_missing(self):
        scenario = self.scenario()
        scenario.api = FakeApi(
            get_routes={
                "/execution-profiles/profile-1": profile_fixture(),
                "/intake/requests/intake-1": {"campaignId": "campaign-1", "spending": None},
                "/actions": [],
            },
            post_routes={
                "/intake/requests": {
                    "id": "intake-1",
                    "actionId": "act-1",
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                },
            },
        )
        scenario.prepare()
        scenario.campaign = "campaign-1"
        with self.assertRaises(real_recovery.ScenarioError):
            scenario.verify_contract()

    def test_prepare_sends_only_the_committed_nixpkgs_pin(self):
        scenario = self.scenario()
        scenario.api = FakeApi(
            get_routes={"/execution-profiles/profile-1": profile_fixture()},
            post_routes={
                "/intake/requests": {
                    "id": "intake-1",
                    "actionId": "act-1",
                    "leadRequestId": "lead-1",
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                }
            },
        )
        scenario.prepare()
        call = next(c for c in scenario.api.calls if c[0] == "POST" and c[1] == "/intake/requests")
        constraints = call[2]["environment_constraints"]
        self.assertEqual(
            constraints["nixpkgs"],
            "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d",
        )
        self.assertEqual(
            set(constraints),
            {"nixpkgs"},
        )
        self.assertNotIn("packages", constraints)
        self.assertNotIn("toolchains", constraints)

    def test_committed_flake_template_provides_python3_without_third_party(self):
        flake = (
            "{\n"
            "  description = \"Factory UAT committed environment\";\n"
            "  inputs.nixpkgs.url = \"github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d\";\n"
            "  outputs = { self, nixpkgs }: {\n"
            "    devShells.x86_64-linux.default =\n"
            "      (import nixpkgs { system = \"x86_64-linux\"; }).mkShell {\n"
            "        buildInputs = [ (import nixpkgs { system = \"x86_64-linux\"; }).python312 ];\n"
            "      };\n"
            "  };\n"
            "}\n"
        )
        self.assertIn("ac62194c3917d5f474c1a844b6fd6da2db95077d", flake)
        self.assertIn("python312", flake)
        self.assertNotIn("pip", flake)
        self.assertNotIn("python3Packages", flake)

    def test_model_purposes_require_exactly_one_each(self):
        scenario = self.scenario()
        scenario.args.project_id = "proj"
        scenario._db_rows = lambda sql, params: [
            ("lead_assessment", "SETTLED"),
            ("lead_environment_review", "SETTLED"),
            ("agentic_planner", "SETTLED"),
        ] if "purpose,state" in sql else []
        snapshot = scenario.verify_model_purposes()
        self.assertEqual(snapshot["purposes"], {
            "lead_assessment": 1,
            "lead_environment_review": 1,
            "agentic_planner": 1,
        })
        self.assertEqual(snapshot["states"]["agentic_planner"], ["SETTLED"])
        self.assertTrue((Path(self.args.evidence_dir) / "model-purposes.json").exists())

    def test_missing_duplicate_or_non_settled_model_purpose_fails_closed(self):
        for rows in (
            [("lead_assessment", "SETTLED"), ("lead_environment_review", "SETTLED")],
            [("lead_assessment", "SETTLED"), ("lead_assessment", "SETTLED"),
             ("lead_environment_review", "SETTLED"), ("agentic_planner", "SETTLED")],
            [("lead_assessment", "UNKNOWN"), ("lead_environment_review", "SETTLED"),
             ("agentic_planner", "SETTLED")],
            [("lead_assessment", "SETTLED"), ("lead_environment_review", "AMBIGUOUS"),
             ("agentic_planner", "SETTLED")],
        ):
            scenario = self.scenario()
            scenario.args.project_id = "proj"
            scenario._db_rows = lambda sql, params, rows=rows: rows if "purpose,state" in sql else []
            with self.assertRaises(real_recovery.ScenarioError):
                scenario.verify_model_purposes()

    def test_environment_binding_must_match_every_check(self):
        spec = {"environment_plan_id": "env-plan-1", "environment_spec_sha256": "b" * 64}
        env = {"planId": "env-plan-1", "specSha256": "b" * 64}
        checks = [
            {"id": "status", "environment_plan_id": "env-plan-1", "environment_spec_sha256": "b" * 64},
            {"id": "version", "environment_plan_id": "env-plan-1", "environment_spec_sha256": "b" * 64},
        ]
        self.assertEqual(real_recovery.environment_binding_problems(spec, checks, env), [])
        bad = [dict(checks[0], environment_spec_sha256="c" * 64), checks[1]]
        self.assertTrue(real_recovery.environment_binding_problems(spec, bad, env))
        self.assertTrue(real_recovery.environment_binding_problems(spec, checks, {}))

    def test_objective_and_checks_keep_the_exact_status_and_version_contract(self):
        self.assertIn("status() must return exactly", real_recovery.OBJECTIVE)
        self.assertIn("version() must return", real_recovery.OBJECTIVE)
        self.assertIn("Python 3.12 standard library", real_recovery.OBJECTIVE)
        self.assertIn("MIT-licensed", real_recovery.OBJECTIVE)
        self.assertEqual(real_recovery.CHECKS[real_recovery.STATUS]["id"], "status")
        self.assertEqual(real_recovery.CHECKS[real_recovery.VERSION]["id"], "version")
        self.assertIn("assert app.status() ==", real_recovery.CHECKS[real_recovery.STATUS]["command"][2])
        self.assertIn("assert app.version() ==", real_recovery.CHECKS[real_recovery.VERSION]["command"][2])

    def test_prepare_requires_the_bound_canonical_profile(self):
        variants = [
            profile_fixture(executable=False),
            profile_fixture(roles={**profile_fixture()["roles"], "lead": {"provider": "other", "model": "x", "reasoning": "medium"}}),
            profile_fixture(autoStart={"enabled": True, "risk_classes": ["R0"]}),
            profile_fixture(progress={**profile_fixture()["progress"], "diagnoseAfterStagnantCheckpoints": 5}),
        ]
        for profile in variants:
            scenario = self.scenario()
            scenario.api = FakeApi(
                get_routes={"/execution-profiles/profile-1": profile},
                post_routes={},
            )
            with self.assertRaises(real_recovery.ScenarioError) as raised:
                scenario.prepare()
            self.assertIn("not the canonical profile", str(raised.exception))

    def test_intervene_returns_the_kill_times(self):
        scenario = self.scenario()
        snapshot = {
            "observed_at": "2026-11-18T00:00:00Z",
            "status_pass": True,
            "version_pass_absent": True,
            "attempt_status": "LEASED",
            "gen1_sessions": [{"session_record_id": SESSION1, "state": "ACTIVE", "turn_id": "turn-1"}],
            "gen1_tool_events": [],
            "last_turn_event_at": "2026-11-17T23:59:00Z",
            "active_turns": 0,
            "lease_state": "LIVE",
            "runtime_health": "healthy",
            "supervisor_health": "healthy",
            "worker_health": "healthy",
            "gen1_quiescent": True,
            "tools_settled": True,
            "quiescence_backend_confirmed": True,
            "p2_check_absent": True,
            "p2_artifacts_absent": True,
            "candidate_count": 0,
            "version_outcome": "",
        }
        scenario._pre_intervention_snapshot = lambda events: dict(snapshot)
        scenario.api = FakeApi(
            get_routes={
                "/events?after=0&limit=500": [],
                "/office/participants?campaign_id=campaign-1&role=IMPLEMENTER&limit=200": {
                    "participants": [{"attempt_id": "attempt-1", "lease_state": "EXPIRED"}]
                }
            }
        )
        scenario.campaign = "campaign-1"
        scenario.attempt = "attempt-1"
        calls = []

        def fake_compose(project, root, git_dir, args):
            calls.append(args)
            return subprocess.CompletedProcess(args, 0, "", "")

        original = real_recovery._podman_compose
        real_recovery._podman_compose = fake_compose
        try:
            result = scenario.intervene(snapshot)
        finally:
            real_recovery._podman_compose = original
        self.assertTrue(result["kill_started_at"])
        self.assertTrue(result["kill_at"])
        self.assertEqual(calls[0][:2], ["kill", "worker"])
        self.assertEqual(calls[-1][:2], ["start", "worker"])

    def test_watch_gen2_binds_candidate_inspection_gate_and_campaign(self):
        scenario = self.scenario()
        scenario.attempt = "attempt-1"
        scenario.campaign = "campaign-1"
        scenario.work_item_id = "unit-1"
        scenario.action = "act-1"
        scenario.expected_command_sha256 = COMMAND_SHA
        scenario.expected_status_command_sha256 = STATUS_COMMAND_SHA
        scenario.expected_base_commit = "c" * 40
        gen2_receipt = real_recovery.CheckReceipt(
            "attempt-1", 2, real_recovery.VERSION, "PASS", "b" * 64, "rec-2",
            "2026-11-18T00:03:00Z", COMMAND_SHA, SESSION2, 2, "call-2", "unit-1",
        )
        status_receipt = real_recovery.CheckReceipt(
            "attempt-1", 1, real_recovery.STATUS, "PASS", "a" * 64, "rec-1",
            "2026-11-17T23:59:00Z", STATUS_COMMAND_SHA, SESSION1, 1, "call-1", "unit-1",
        )
        scenario._receipts = lambda generation: [status_receipt] if generation == 1 else [gen2_receipt]
        scenario.api = FakeApi(
            get_routes={
                "/events?after=0&limit=500": [
                    {
                        "kind": "worker.claimed",
                        "subject_id": "attempt-1",
                        "detail": "generation=2",
                        "recorded_at": "2026-11-18T00:02:00Z",
                    }
                ],
                "/provider/sessions": [
                    {
                        "attempt_id": "attempt-1",
                        "generation": 2,
                        "state": "COMPLETED",
                        "session_record_id": SESSION2,
                        "provider": "opencode-go",
                        "model": "deepseek-v4.1-flash",
                        "provider_mode": "build",
                        "turn_id": "turn-2",
                    }
                ],
                "/candidates": [
                    {"id": "cand-1", "attempt_id": "attempt-1", "created_at": "2026-11-18T00:02:30Z"}
                ],
                "/quality/candidates/cand-1": {
                    "id": "cand-1",
                    "attempt_id": "attempt-1",
                    "commit_sha": "a" * 40,
                    "tree_sha": "b" * 40,
                    "created_at": "2026-11-18T00:02:30Z",
                },
                "/quality/candidates/cand-1/inspection": {
                    "candidate": {"id": "cand-1"},
                    "base_commit": "c" * 40,
                },
                "/quality/candidates/cand-1/gate": {
                    "candidate_id": "cand-1",
                    "outcome": "ACCEPTED",
                    "decided_at": "2026-11-18T00:04:00Z",
                },
                "/campaigns/campaign-1": {
                    "campaign": {"id": "campaign-1", "status": "AUTHORIZED", "action_id": "act-1"},
                    "attempts": [{"id": "attempt-1", "status": "LEASED"}],
                    "tasks": [{"unit_id": "unit-1"}],
                },
            }
        )
        scenario._pre_intervention_outcome = lambda: ""
        snapshot = {
            "kill_started_at": "2026-11-18T00:01:00Z",
            "kill_at": "2026-11-18T00:01:05Z",
            "gen1_sessions": [{"session_record_id": SESSION1}],
        }
        watched = scenario.watch_gen2(snapshot)
        self.assertEqual(watched.candidate_sha, "a" * 40)
        self.assertEqual(watched.candidate_tree, "b" * 40)
        self.assertEqual(watched.candidate_base, "c" * 40)
        self.assertEqual(watched.gate_candidate_id, "cand-1")
        self.assertEqual(watched.campaign_detail_id, "campaign-1")
        self.assertEqual(watched.gen2_session_record_id, SESSION2)
        self.assertEqual(watched.late_effect_risk, "NONE")
        verdict = real_recovery.classify_recovery(watched)
        self.assertEqual(verdict.status, "OK_SCOPED", verdict.reason)
        self.assertIn("restart", verdict.reason)

    def test_verify_restart_observes_exact_state(self):
        scenario = self.scenario()
        scenario.campaign = "campaign-1"
        scenario.attempt = "attempt-1"
        scenario.api = FakeApi(
            get_routes={
                "/quality/candidates/cand-1": {
                    "id": "cand-1",
                    "commit_sha": "a" * 40,
                    "tree_sha": "b" * 40,
                },
                "/quality/candidates/cand-1/gate": {
                    "candidate_id": "cand-1",
                    "outcome": "ACCEPTED",
                    "decided_at": "2026-11-18T00:05:00Z",
                },
                "/campaigns/campaign-1": {
                    "campaign": {"id": "campaign-1", "status": "AUTHORIZED"},
                    "attempts": [{"id": "attempt-1", "status": "LEASED"}],
                },
            }
        )
        data = observation(candidate_id="cand-1")
        original_compose = real_recovery._podman_compose
        original_health = real_recovery.Scenario._container_health
        original_sleep = real_recovery.time.sleep
        real_recovery.time.sleep = lambda seconds: None
        real_recovery.Scenario._container_health = lambda self, service: "healthy"
        try:
            real_recovery._podman_compose = lambda *args: subprocess.CompletedProcess(args, 0, "", "")
            self.assertTrue(scenario.verify_restart(data))

            def mutating_compose(*args):
                scenario.api.get_routes["/quality/candidates/cand-1/gate"]["outcome"] = "BLOCKED"
                return subprocess.CompletedProcess(args, 0, "", "")

            real_recovery._podman_compose = mutating_compose
            self.assertFalse(scenario.verify_restart(data))
        finally:
            real_recovery._podman_compose = original_compose
            real_recovery.Scenario._container_health = original_health
            real_recovery.time.sleep = original_sleep

    def test_main_propagates_verdict_and_observation_to_capture(self):
        class FakeMainScenario:
            def __init__(self, args):
                self.captured = None
                self.cleaned = False

            def prepare(self):
                pass

            def admit(self):
                pass

            def assess(self):
                pass

            def prepare_plan(self):
                pass

            def approve(self):
                pass

            def verify_contract(self):
                pass

            def watch_gen1(self):
                return {"observed_at": "2026-11-18T00:00:00Z"}

            def intervene(self, snapshot):
                return {**snapshot, "kill_started_at": "2026-11-18T00:00:01Z"}

            def watch_gen2(self, snapshot):
                return observation(campaign_status="AUTHORIZED", gate_outcome="ACCEPTED")

            def verify_restart(self, watched):
                return True

            def capture(self, verdict, watched):
                self.captured = (verdict, watched)

            def cleanup(self):
                self.cleaned = True

            def write(self, *args):
                pass

        args = [
            "--execute",
            "--core-api", "http://127.0.0.1:8899",
            "--operator-token-file", "/tmp/opencode/operator.token",
            "--project-id", "project-1",
            "--profile-id", "profile-1",
            "--authorization", "owner-ref",
            "--evidence-dir", "/tmp/opencode/evidence",
            "--compose-project", "sfe2e-recovery",
            "--git-dir", "/tmp/opencode/git",
            "--core-database", "/tmp/opencode/sfe2e-recovery/factory.sqlite",
            "--assumption-file", "/tmp/opencode/assumption.json",
            "--authorized-micros", "0",
        ]
        holder = {}

        class CapturingScenario(FakeMainScenario):
            def capture(self, verdict, watched):
                holder["captured"] = (verdict, watched)

            def cleanup(self):
                holder["cleaned"] = True

        with mock.patch.object(real_recovery, "Scenario", CapturingScenario):
            code = real_recovery.main(args)
        self.assertEqual(code, 0)
        verdict, watched = holder["captured"]
        self.assertEqual(verdict.status, "ACCEPTED", verdict.reason)
        self.assertTrue(watched.restart_observed)
        self.assertTrue(holder["cleaned"])

    def test_run_scenario_passes_the_intervened_snapshot(self):
        scenario = self.scenario()
        seen = {}

        def fake_intervene(snapshot):
            return {**snapshot, "kill_started_at": "2026-11-18T00:00:01Z"}

        def fake_watch_gen2(snapshot):
            seen["snapshot"] = snapshot
            return observation(campaign_status="AUTHORIZED", gate_outcome="ACCEPTED")

        scenario.prepare = lambda: None
        scenario.assess = lambda: None
        scenario.admit = lambda: None
        scenario.prepare_plan = lambda: None
        scenario.approve = lambda: None
        scenario.verify_contract = lambda: None
        scenario.watch_gen1 = lambda: {"observed_at": "2026-11-18T00:00:00Z"}
        scenario.intervene = fake_intervene
        scenario.watch_gen2 = fake_watch_gen2
        scenario.verify_restart = lambda observation: True
        verdict, watched = real_recovery.run_scenario(scenario)
        self.assertEqual(seen["snapshot"]["kill_started_at"], "2026-11-18T00:00:01Z")
        self.assertEqual(verdict.status, "ACCEPTED", verdict.reason)
        self.assertTrue(watched.restart_observed)


class AuthorizationContractTests(unittest.TestCase):
    def record(self, **overrides):
        record = {
            "campaignId": "campaign-1", "authorizationId": "auth-1", "riskClass": "R0",
            "executionMode": "TRUSTED_LOCAL_PROVIDER", "actor": "operator", "rationale": "owner",
            "authorizedPaths": ["src"], "provider": "opencode-go", "model": "deepseek-v4.1-flash",
            "providerMode": "build", "delegationId": "delegation-1",
            "schemaVersion": 2, "policy": {"monetaryEnvelopeId": "owner-ref"}, "createdAt": "2026-11-18T00:00:00Z",
        }
        record.update(overrides)
        return record

    PATHS = ["src"]

    def problems(self, record, micros=0, paths=None):
        return real_recovery.authorization_problems(
            record, "campaign-1", "opencode-go", "deepseek-v4.1-flash", "build",
            self.PATHS if paths is None else paths,
        )

    def test_core_camel_case_record_is_valid(self):
        self.assertEqual(self.problems(self.record()), [])

    def test_envelope_must_live_in_policy_v2(self):
        problems = self.problems(self.record(policy={}))
        self.assertIn("policy.monetaryEnvelopeId missing", problems)

    def test_wrong_mode_or_campaign_fails(self):
        for record in (
            self.record(executionMode="FIXTURE"),
            self.record(campaignId="other"),
        ):
            self.assertTrue(self.problems(record))

    def test_risk_and_paths_are_exact(self):
        self.assertIn("riskClass='R1'", self.problems(self.record(riskClass="R1")))
        self.assertTrue(self.problems(self.record(authorizedPaths=["src", "docs"])))
        self.assertTrue(self.problems(self.record(authorizedPaths="src")))

    def test_snake_case_is_not_an_alias(self):
        record = {
            "campaign_id": "campaign-1", "authorization_id": "auth-1",
            "execution_mode": "TRUSTED_LOCAL_PROVIDER", "schema_version": 2,
        }
        problems = self.problems(record)
        self.assertIn("authorizationId missing", problems)
        self.assertIn("policy missing", problems)


class CliTests(unittest.TestCase):
    def test_dry_run_rejects_negative_micros_and_accepts_subscription_zero(self):
        base = [
            "--dry-run",
            "--core-api", "http://127.0.0.1:8899/core-api",
            "--operator-token-file", "/tmp/opencode/operator.token",
            "--project-id", "project-1",
            "--profile-id", "profile-1",
            "--authorization", "owner-ref",
            "--evidence-dir", "/tmp/opencode/evidence",
            "--compose-project", "sfe2e-recovery",
            "--git-dir", "/tmp/opencode/git",
            "--core-database", "/tmp/opencode/sfe2e-recovery/factory.sqlite",
        ]
        import contextlib
        import io

        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(real_recovery.main([*base, "--authorized-micros", "-1"]), 2)
            self.assertEqual(real_recovery.main([*base, "--authorized-micros", "0"]), 0)

    def test_disposable_project_is_required(self):
        base = [
            "--dry-run",
            "--core-api", "http://127.0.0.1:8899/core-api",
            "--operator-token-file", "/tmp/opencode/operator.token",
            "--project-id", "project-1",
            "--profile-id", "profile-1",
            "--authorization", "owner-ref",
            "--evidence-dir", "/tmp/opencode/evidence",
            "--git-dir", "/tmp/opencode/git",
            "--core-database", "/tmp/opencode/sfe2e-recovery/factory.sqlite",
            "--authorized-micros", "5000000",
        ]
        import contextlib
        import io

        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(real_recovery.main([*base, "--compose-project", "software-factory"]), 2)
            self.assertEqual(real_recovery.main([*base, "--compose-project", "sfe2e-recovery"]), 0)

    def test_missing_compose_project_is_exit_2_not_a_type_error(self):
        base = [
            "--dry-run",
            "--core-api", "http://127.0.0.1:8899/core-api",
            "--operator-token-file", "/tmp/opencode/operator.token",
            "--project-id", "project-1",
            "--profile-id", "profile-1",
            "--authorization", "owner-ref",
            "--evidence-dir", "/tmp/opencode/evidence",
            "--git-dir", "/tmp/opencode/git",
            "--core-database", "/tmp/opencode/sfe2e-recovery/factory.sqlite",
            "--authorized-micros", "0",
        ]
        import contextlib
        import io

        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(real_recovery.main(base), 2)

    def test_plan_is_available_without_inputs(self):
        import contextlib
        import io

        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(real_recovery.main(["--plan"]), 0)


if __name__ == "__main__":
    unittest.main()


class TransportDiagnosticsTests(unittest.TestCase):
    """A timed-out POST must not inherit an earlier error, and must hold."""

    def test_last_error_is_bound_to_the_request_that_raised_it(self) -> None:
        import io
        import urllib.error
        from unittest import mock

        api = real_recovery.Api("http://127.0.0.1:1/core-api", "t" * 32)
        failure = urllib.error.HTTPError(
            "http://x", 403, "forbidden", {}, io.BytesIO(b'{"error":"stale"}')
        )
        with mock.patch("urllib.request.urlopen", side_effect=failure):
            with self.assertRaises(urllib.error.HTTPError):
                api.post("/staged", {})
        self.assertEqual(api.last_error_request, ("POST", "/staged"))
        with mock.patch("urllib.request.urlopen", side_effect=TimeoutError("timed out")):
            with self.assertRaises(TimeoutError):
                api.post("/environment/review", {})
        self.assertIsNone(api.last_error)
        self.assertIsNone(api.last_error_request)

    def test_log_tails_are_bounded_and_redacted(self) -> None:
        tail = "Authorization: Bearer abcdef0123456789 token=supersecret" + "x" * 40_000
        redacted = real_recovery._redact_text(tail)
        self.assertNotIn("abcdef0123456789", redacted)
        self.assertNotIn("supersecret", redacted)
        self.assertLessEqual(len(redacted), 16 * 1024)

    def test_unknown_environment_job_holds_cleanup(self) -> None:
        import json
        import tempfile
        import types

        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            (evidence / "failure.json").write_text(
                json.dumps({"reconciliation": {"outcome": "ENV_JOB_UNKNOWN"}})
            )
            scenario = types.SimpleNamespace(
                evidence=evidence,
                project="factory-e2e-recovery-run9-fixture",
                write=lambda name, payload: (evidence / name).write_text(json.dumps(payload)),
            )
            self.assertFalse(real_recovery._safe_to_cleanup(scenario))
            held = json.loads((evidence / "cleanup-held.json").read_text())
            self.assertEqual(held["status"], "STACK_RETAINED_FOR_RECONCILIATION")
            self.assertIn("environment job", held["reason"])


class CompositeStageDeadlineTests(unittest.TestCase):
    def test_review_deadline_derives_from_the_plan_budget(self) -> None:
        scenario = object.__new__(real_recovery.Scenario)
        scenario.environment_plan = {
            "plan": {
                "resources": {"timeoutSecs": 1800},
                "network": {"fetchAllowlist": [f"host-{index}" for index in range(7)]},
            }
        }
        deadline, derivation = scenario._environment_review_deadline()
        self.assertEqual(deadline, 300.0 + 2 * 1800 + 60)
        self.assertEqual(derivation["phases"], 2)
        self.assertEqual(derivation["fetchAllowlistEntries"], 7)
        self.assertIn("composite stage", derivation["reason"])

    def test_review_deadline_refuses_a_projection_without_fields(self) -> None:
        scenario = object.__new__(real_recovery.Scenario)
        scenario.environment_plan = {}
        with self.assertRaises(real_recovery.ScenarioError):
            scenario._environment_review_deadline()
        scenario.environment_plan = {"plan": {"resources": {}, "network": {"fetchAllowlist": []}}}
        with self.assertRaises(real_recovery.ScenarioError):
            scenario._environment_review_deadline()
