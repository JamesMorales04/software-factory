#!/usr/bin/env python3
"""Synthetic tests for the real generation-2 recovery scenario.

No Core, provider, container or paid inference is contacted. Run:
python3 -m unittest scripts/e2e/tests/test_real_recovery.py
"""

from __future__ import annotations

import argparse
import json
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


class FakeApi:
    """Records calls and serves canonical Core JSON shapes."""

    def __init__(self, get_routes=None, post_routes=None, post_status_response=None):
        self.get_routes = dict(get_routes or {})
        self.post_routes = dict(post_routes or {})
        self.post_status_response = post_status_response or (
            403,
            {"error": "direct provider authorization refused"},
        )
        self.calls = []

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
        return self.post_status_response


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
        assess_projection = {
            "id": "intake-1",
            "state": "AWAITING_APPROVAL",
            "plan": {"plan_hash": "9" * 64},
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
                    "profile": {"revisionId": "rev-1", "contentHash": "8" * 64},
                },
                "/intake/requests/intake-1/assess": assess_projection,
                "/intake/requests/intake-1/subscription-admission": admission,
                "/intake/requests/intake-1/approve": approval,
            },
            post_status_response=(409, {"code": "MONETARY_AUTHORIZATION_NOT_APPLICABLE"}),
        )
        scenario.prepare()
        self.write_assumption(scenario)
        scenario.admit()
        scenario.assess()
        scenario.approve()
        self.assertEqual(scenario.plan_hash, "9" * 64)
        self.assertEqual(scenario.profile_revision_id, "rev-1")
        self.assertEqual(scenario.admission_id, "admission-1")
        self.assertEqual(scenario.campaign, "campaign-1")
        approve_call = next(
            call for call in scenario.api.calls
            if call[0] == "POST" and call[1] == "/intake/requests/intake-1/approve"
        )
        self.assertEqual(approve_call[2]["subscription_admission_id"], "admission-1")
        self.assertNotIn("authorized_micros", approve_call[2])
        self.assertEqual(approve_call[2]["expected_plan_hash"], "9" * 64)
        negative = next(
            call for call in scenario.api.calls
            if call[0] == "POST_STATUS" and call[1] == "/intake/requests/intake-1/approve"
        )
        self.assertGreater(negative[2]["authorized_micros"], 0)
        self.assertEqual(negative[2]["subscription_admission_id"], "admission-1")

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
