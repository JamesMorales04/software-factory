#!/usr/bin/env python3
"""Synthetic tests for the real generation-2 recovery scenario.

No Core, provider, container or paid inference is contacted. Run:
python3 -m unittest scripts/e2e/tests/test_real_recovery.py
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve()
sys.path.insert(0, str(HERE.parents[1]))
import real_recovery  # noqa: E402


COMMAND_SHA = "d" * 64
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
                "2026-11-17T23:59:00Z", COMMAND_SHA, "sess-1", 1, "call-1", "unit-1",
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
        verdict = real_recovery.classify_recovery(observation())
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

    def test_gate_candidate_mismatch_is_not_tested(self):
        verdict = real_recovery.classify_recovery(observation(gate_candidate_id="other"))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_candidate_base_missing_is_not_tested(self):
        verdict = real_recovery.classify_recovery(observation(candidate_base=""))
        self.assertEqual(verdict.status, "NOT_TESTED", verdict.reason)

    def test_barrier_failures_are_not_guaranteed(self):
        snapshot = {
            "status_pass": True, "version_pass_absent": True, "attempt_status": "LEASED",
            "active_turns": 0, "gen1_quiescent": True, "quiescence_backend_confirmed": True,
            "p2_check_absent": True, "p2_artifacts_absent": True, "candidate_count": 0,
            "lease_state": "LIVE", "runtime_health": "healthy", "supervisor_health": "healthy",
            "worker_health": "healthy",
        }
        self.assertEqual(real_recovery.barrier_failures(snapshot), [])
        snapshot["worker_health"] = "unhealthy"
        self.assertTrue(real_recovery.barrier_failures(snapshot))
        snapshot["worker_health"] = "healthy"
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

    def test_core_camel_case_record_is_valid(self):
        problems = real_recovery.authorization_problems(self.record(), "campaign-1", "opencode-go", "deepseek-v4.1-flash", "build", 5_000_000)
        self.assertEqual(problems, [])

    def test_envelope_must_live_in_policy_v2(self):
        record = self.record(policy={})
        problems = real_recovery.authorization_problems(record, "campaign-1", "opencode-go", "deepseek-v4.1-flash", "build", 5_000_000)
        self.assertIn("policy.monetaryEnvelopeId missing", problems)

    def test_wrong_mode_or_campaign_or_micros_fails(self):
        for record, micros in (
            (self.record(executionMode="FIXTURE"), 5_000_000),
            (self.record(campaignId="other"), 5_000_000),
            (self.record(), 0),
        ):
            self.assertTrue(real_recovery.authorization_problems(record, "campaign-1", "opencode-go", "deepseek-v4.1-flash", "build", micros))


class CliTests(unittest.TestCase):
    def test_dry_run_rejects_zero_micros(self):
        code = real_recovery.main(
            [
                "--dry-run",
                "--core-api", "http://127.0.0.1:8899/core-api",
                "--operator-token-file", "/tmp/opencode/operator.token",
                "--project-id", "project-1",
                "--authorization", "owner-ref",
                "--evidence-dir", "/tmp/opencode/evidence",
                "--compose-project", "sfe2e-recovery",
                "--git-dir", "/tmp/opencode/git",
                "--authorized-micros", "0",
            ]
        )
        self.assertEqual(code, 2)

    def test_plan_is_available_without_inputs(self):
        import contextlib
        import io

        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(real_recovery.main(["--plan"]), 0)


if __name__ == "__main__":
    unittest.main()
