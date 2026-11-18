#!/usr/bin/env python3
"""Real generation-2 recovery scenario for Factory (P1-P4 acceptance).

Development/UAT tooling, not the product install path and not independent
acceptance. It drives the existing Core operator/Lead HTTP surfaces against a
disposable real-provider stack and a small deterministic project whose two
functional results are exact backend checks.

The approved ActionSpec is owned by the Lead plan. This scenario verifies that
the approved checks assert the exact contract below; it never rewrites the
plan to make a run pass. A plan that differs is INCONCLUSIVE/FAIL before any
generation-2 intervention.

See docs/implementation/real-gen2-recovery-scenario.md for the credential
transfer proposal, the P2 quiescence barrier and the evidence rules.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any, Callable, Iterable

from real_recovery_profile import profile_mismatches

STATUS = "status"
VERSION = "version"
STATUS_OBJECT = {"service": "factory-e2e", "status": "ok"}
VERSION_OBJECT = {"version": "1.0"}
CHECKS = {
    STATUS: {
        "id": STATUS,
        "kind": "functional",
        "description": "status() returns the exact status object",
        "command": [
            "python3",
            "-c",
            "import sys; sys.path.insert(0,'src'); import app; "
            "assert app.status() == {'service': 'factory-e2e', 'status': 'ok'}",
        ],
        "expectExit": 0,
        "timeoutSecs": 30,
    },
    VERSION: {
        "id": VERSION,
        "kind": "functional",
        "description": "version() returns the exact version object",
        "command": [
            "python3",
            "-c",
            "import sys; sys.path.insert(0,'src'); import app; "
            "assert app.version() == {'version': '1.0'}",
        ],
        "expectExit": 0,
        "timeoutSecs": 30,
    },
}
OBJECTIVE = (
    "Implement src/app.py with two functions. status() must return exactly "
    "{'service': 'factory-e2e', 'status': 'ok'} and version() must return "
    "exactly {'version': '1.0'}. Both frozen checks must pass. Use only the "
    "Python 3.12 standard library; do not add third-party dependencies. The "
    "project is MIT-licensed. Commit only the required source change."
)
ALLOWED_PATHS = ["src"]
# The UAT fixture commits a real flake.nix/flake.lock, so the environment
# source is PROJECT_COMMITTED and Core does not generate `buildInputs` from
# explicit packages. The request only pins the exact nixpkgs revision that the
# committed flake and lock must reference. No generated toolchain/package is
# implied, and no license catalogue or policy entry changes.
ENVIRONMENT_CONSTRAINTS = {
    "nixpkgs": "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d",
}
# The current production MODEL path is subscription-funded: the request-scoped
# Owner admission is Core-created for an AUTONOMOUS R0 request and the V2 policy
# carries `urn:factory:no-billable-inference`. A positive value is only the
# test-support billable-envelope path; it is never a global cap.
NO_BILLABLE_ENVELOPE = "urn:factory:no-billable-inference"
DEFAULT_AUTHORIZED_MICROS = 0


def utc() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


@dataclass(frozen=True)
class CheckReceipt:
    attempt_id: str
    generation: int
    check_id: str
    outcome: str
    candidate_sha256: str
    receipt_id: str
    recorded_at: str
    command_sha256: str = ""
    session_record_id: str = ""
    sequence: int = 0
    call_id: str = ""
    work_item_id: str = ""


@dataclass(frozen=True)
class Observation:
    attempt_id: str
    campaign_id: str
    claimed_generation_two: bool
    claimed_at: str
    generation_two_session_state: str
    generation_two_session_error: str
    generation_one_receipts: list[CheckReceipt] = field(default_factory=list)
    generation_two_receipts: list[CheckReceipt] = field(default_factory=list)
    pre_intervention_version_outcome: str = ""
    candidates_after_claim: list[dict[str, Any]] = field(default_factory=list)
    allocation_conflict: bool = False
    gen2_provider: str = ""
    gen2_model: str = ""
    gen2_provider_mode: str = ""
    gate_outcome: str = ""
    candidate_sha: str = ""
    candidate_tree: str = ""
    candidate_base: str = ""
    campaign_status: str = ""
    restart_required: bool = False
    restart_observed: bool = False
    late_effect_risk: str = ""
    kill_at: str = ""
    kill_started_at: str = ""
    expected_provider: str = ""
    expected_model: str = ""
    expected_provider_mode: str = ""
    expected_work_item: str = ""
    expected_command_sha256: str = ""
    gen2_session_record_id: str = ""
    candidate_id: str = ""
    gate_candidate_id: str = ""
    campaign_action: str = ""
    expected_action: str = ""
    expected_base_commit: str = ""
    expected_status_command_sha256: str = ""
    campaign_detail_id: str = ""
    gen1_session_record_ids: list[str] = field(default_factory=list)


@dataclass(frozen=True)
class Verdict:
    status: str  # OK | FAIL | NOT_TESTED | INCONCLUSIVE
    reason: str
    evidence: dict[str, Any]


class ScenarioError(RuntimeError):
    pass


class ScenarioInvalid(ScenarioError):
    """The scenario preconditions were not met; the run is NOT_TESTED."""


def command_sha256(command: list[str]) -> str:
    import hashlib

    return hashlib.sha256(json.dumps(command, separators=(",", ":")).encode("utf-8")).hexdigest()


def barrier_failures(snapshot: dict[str, Any]) -> list[str]:
    """Every pre-intervention requirement, derived only from observed facts."""

    failures: list[str] = []
    if snapshot.get("status_pass") is not True:
        failures.append("generation-1 status PASS not observed")
    if snapshot.get("version_pass_absent") is not True:
        failures.append("generation-1 already has a version PASS")
    if str(snapshot.get("attempt_status") or "").upper() not in {"LEASED", "RUNNING"}:
        failures.append(f"attempt_status={snapshot.get('attempt_status')!r}")
    if int(snapshot.get("active_turns") or 0) != 0:
        failures.append(f"active_turns={snapshot.get('active_turns')}")
    if snapshot.get("gen1_quiescent") is not True:
        failures.append("gen1_quiescent is not true")
    if snapshot.get("tools_settled") is not True:
        failures.append("tool calls are not settled")
    if snapshot.get("quiescence_backend_confirmed") is not True:
        failures.append("quiescence_backend_confirmed is not true")
    if snapshot.get("p2_check_absent") is not True:
        failures.append("the P2 check is already present")
    if snapshot.get("p2_artifacts_absent") is not True:
        failures.append("P2 artifacts are already present")
    if int(snapshot.get("candidate_count") or 0) != 0:
        failures.append(f"candidate_count={snapshot.get('candidate_count')}")
    if not str(snapshot.get("lease_state") or "").strip():
        failures.append("lease_state not observed")
    for service in ("runtime", "supervisor", "worker"):
        if snapshot.get(f"{service}_health") != "healthy":
            failures.append(f"{service}_health={snapshot.get(f'{service}_health')!r}")
    return failures


def _receipts_for(receipts: Iterable[CheckReceipt], check_id: str, generation: int) -> list[CheckReceipt]:
    return [r for r in receipts if r.check_id == check_id and r.generation == generation]


def _find_passing(receipts: Iterable[CheckReceipt], check_id: str, generation: int) -> CheckReceipt | None:
    return next((r for r in _receipts_for(receipts, check_id, generation) if r.outcome == "PASS"), None)


def classify_p2_precondition(observation: Observation) -> tuple[str, str]:
    """'PENDING' | 'ALREADY_DONE' | 'UNKNOWN' with a causal reason.

    PENDING is proven by durable Core facts: a passing generation-1 status
    receipt and no passing generation-1 version receipt. A generation-1 version
    FAIL receipt is also a real negative observation. No synthetic check or
    reconstructed snapshot is involved.
    """

    if _find_passing(observation.generation_one_receipts, STATUS, 1) is None:
        return "UNKNOWN", "generation 1 has no passing status receipt"
    if _find_passing(observation.generation_one_receipts, VERSION, 1) is not None:
        return "ALREADY_DONE", "generation 1 has a passing version receipt"
    if observation.pre_intervention_version_outcome:
        outcome = observation.pre_intervention_version_outcome.upper()
        if outcome == "PASS":
            return "ALREADY_DONE", "generation 1 already completed the version result"
        return "PENDING", f"generation 1 version receipt is {outcome}"
    return "PENDING", "generation 1 has no passing version receipt"


def _parse_iso(value: str):
    from datetime import datetime

    if not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return parsed if parsed.tzinfo else None


LEGITIMATE_LATE = {
    "provider.session_terminal",
    "provider.session_cleaned",
    "worker.terminal_reported",
    "worker.session_rotated",
    "worker.claimed",
    "run.status_changed",
    "cleanup",
}
DANGEROUS_LATE = {
    "provider.turn_authorized",
    "provider.turn_settled",
    "candidate.created",
    "candidate.recorded",
    "progress.checkpoint",
    "execution_check.receipt",
}


def classify_late_effects(
    events: Iterable[dict[str, Any]],
    kill_at: str,
    claim_at: str,
    gen1_session_ids: Iterable[str] = (),
) -> tuple[str, str]:
    """Classify events after the kill, including the post-claim window.

    A generation-1 tool result or check after the kill (even after the gen2
    claim) is dangerous; it proves gen1 was still working when the recovery was
    supposed to start. The generation-2 claim, run lifecycle, session terminal
    and cleanup are legitimate. Anything unrecognised is ambiguous and never a
    PASS.
    """

    kill = _parse_iso(kill_at)
    claim = _parse_iso(claim_at)
    if kill is None:
        return "AMBIGUOUS", "kill timestamp is missing"
    gen1 = {str(value) for value in gen1_session_ids if value}
    for event in events:
        recorded = _parse_iso(str(event.get("recorded_at") or ""))
        if recorded is None or recorded < kill:
            continue
        kind = str(event.get("kind") or "")
        if kind == "provider.tool_event":
            subject = str(event.get("subject_id") or "")
            before_claim = claim is None or recorded <= claim
            if subject in gen1 or (before_claim and "RESULT" in str(event.get("detail") or "").upper()):
                return "DANGEROUS", f"tool event at {event.get('recorded_at')}"
            continue
        if kind in DANGEROUS_LATE:
            # Turn changes after the gen2 claim belong to the recovered session;
            # before the claim they are late generation-1 effects.
            if claim is None or recorded <= claim:
                return "DANGEROUS", f"{kind} at {event.get('recorded_at')}"
            continue
        if kind in LEGITIMATE_LATE or kind.startswith("cleanup") or kind.startswith("audit"):
            continue
        return "AMBIGUOUS", f"unclassified event {kind} at {event.get('recorded_at')}"
    return "NONE", ""


def tool_event_rows(payload: Any) -> list[dict[str, Any]]:
    """Unwrap the `/provider/sessions/{id}/tool_events` projection."""

    events = payload.get("events") if isinstance(payload, dict) else payload
    return [event for event in (events or []) if isinstance(event, dict)]


def tool_calls_settled(tool_events: Iterable[dict[str, Any]]) -> bool:
    """True when every observed tool call reached a terminal decision.

    Tool lifecycle decisions are REQUEST -> ALLOW/DENY -> RESULT/ERROR. A
    trailing REQUEST or ALLOW means a tool call is still in flight, so the
    generation cannot be treated as quiescent.
    """

    last: dict[str, str] = {}
    for event in tool_events:
        call_id = str(event.get("call_id") or "")
        if call_id:
            last[call_id] = str(event.get("decision") or "").upper()
    return bool(last) and all(decision in {"RESULT", "ERROR", "DENY"} for decision in last.values())


def quiescence_confirmed(
    sessions: Iterable[dict[str, Any]],
    tool_events: Iterable[dict[str, Any]],
    turn_authorized: int,
    turn_settled: int,
    health: dict[str, str],
) -> bool:
    """A live lease alone is not quiescence.

    The turn balance must be settled, every tool call must have a terminal
    decision, the generation-1 sessions must still be live with a turn id, and
    every runtime service must be healthy. No new endpoint or field is used.
    """

    session_rows = list(sessions)
    sessions_ok = bool(session_rows) and all(
        str(session.get("state") or "").upper() in {"ACTIVE", "COMPLETED"}
        and str(session.get("turn_id") or "").strip()
        for session in session_rows
    )
    active_turns = max(0, turn_authorized - turn_settled)
    return (
        sessions_ok
        and tool_calls_settled(tool_events)
        and turn_settled > 0
        and active_turns == 0
        and all(value == "healthy" for value in health.values())
    )


def confirmed_by_matches(recorded_by: str, file_by: str) -> bool:
    """Accept only the identity forms Core actually records.

    Core stores `format!("{principal_id}:{confirmed_by}")`. The accepted forms
    are the exact unauthenticated value, `operator-token:<file>` and
    `device:<non-empty id>:<file>`. Anything else is a mismatch; the value is
    never trimmed or synthesized here.
    """

    if not file_by:
        return False
    if recorded_by == file_by:
        return True
    if recorded_by == f"operator-token:{file_by}":
        return True
    prefix = "device:"
    suffix = f":{file_by}"
    if recorded_by.startswith(prefix) and recorded_by.endswith(suffix):
        device_id = recorded_by[len(prefix) : -len(suffix)]
        return bool(device_id) and ":" not in device_id
    return False


def _is_after(value: str, reference) -> bool:
    parsed = _parse_iso(str(value or ""))
    return parsed is not None and reference is not None and parsed > reference


def _is_git_sha(value: str) -> bool:
    return len(value) == 40 and all(ch in "0123456789abcdef" for ch in value.lower())


def _is_sha256(value: str) -> bool:
    return len(value) == 64 and all(ch in "0123456789abcdef" for ch in value.lower())


def classify_recovery(observation: Observation) -> Verdict:
    """Deterministic P1-P4 verdict. A claim alone is never OK."""

    evidence: dict[str, Any] = {
        "attempt_id": observation.attempt_id,
        "campaign_id": observation.campaign_id,
        "claimed_at": observation.claimed_at,
        "pre_intervention_version_outcome": observation.pre_intervention_version_outcome,
        "generation_two_session_state": observation.generation_two_session_state,
        "generation_two_session_error": observation.generation_two_session_error,
    }
    if observation.allocation_conflict:
        return Verdict("FAIL", "generation-2 provider workspace allocation was refused", evidence)
    if not observation.claimed_generation_two:
        return Verdict("NOT_TESTED", "generation 2 was not observed", evidence)
    if (observation.generation_two_session_state or "").upper() in {"FAILED", "CANCELLED"}:
        return Verdict(
            "FAIL",
            "generation-2 session did not complete: "
            f"{observation.generation_two_session_error or observation.generation_two_session_state}",
            evidence,
        )
    if (observation.generation_two_session_state or "").upper() not in {"ACTIVE", "COMPLETED"}:
        return Verdict("NOT_TESTED", f"generation-2 session state is {observation.generation_two_session_state!r}", evidence)

    if observation.late_effect_risk in {"DANGEROUS", "AMBIGUOUS"}:
        return Verdict(
            "NOT_TESTED",
            f"late generation-1 effect after the kill: {observation.late_effect_risk}",
            evidence,
        )

    status_receipt = _find_passing(observation.generation_one_receipts, STATUS, 1)
    if status_receipt is None:
        return Verdict("NOT_TESTED", "P1: generation 1 has no passing status receipt", evidence)
    evidence["p1_status_receipt"] = status_receipt.__dict__
    status_problems = []
    if (
        not observation.expected_status_command_sha256
        or status_receipt.command_sha256 != observation.expected_status_command_sha256
    ):
        status_problems.append("command_sha256")
    if not observation.expected_work_item or status_receipt.work_item_id != observation.expected_work_item:
        status_problems.append("work_item_id")
    if status_receipt.session_record_id not in observation.gen1_session_record_ids:
        status_problems.append("session_record_id")
    if status_problems:
        return Verdict("NOT_TESTED", "P1 receipt identity incomplete: " + ", ".join(status_problems), evidence)

    precondition, reason = classify_p2_precondition(observation)
    if precondition == "ALREADY_DONE":
        return Verdict("NOT_TESTED", f"P2: {reason}", evidence)
    if precondition != "PENDING":
        return Verdict("NOT_TESTED", f"P2: {reason}", evidence)

    version_receipt = _find_passing(observation.generation_two_receipts, VERSION, 2)
    if version_receipt is None:
        return Verdict("FAIL", "P3: generation 2 has no passing version receipt for the pending work", evidence)
    evidence["p3_version_receipt"] = version_receipt.__dict__
    receipt_problems = []
    if not observation.expected_command_sha256 or version_receipt.command_sha256 != observation.expected_command_sha256:
        receipt_problems.append("command_sha256")
    if not observation.expected_work_item or version_receipt.work_item_id != observation.expected_work_item:
        receipt_problems.append("work_item_id")
    if not observation.gen2_session_record_id or version_receipt.session_record_id != observation.gen2_session_record_id:
        receipt_problems.append("session_record_id")
    if int(version_receipt.sequence or 0) <= 0 or not version_receipt.call_id:
        receipt_problems.append("sequence/call_id")
    if receipt_problems:
        return Verdict("NOT_TESTED", "P3 receipt identity incomplete: " + ", ".join(receipt_problems), evidence)

    fresh = [c for c in observation.candidates_after_claim if str(c.get("attempt_id") or "") == observation.attempt_id]
    if not fresh:
        return Verdict("NOT_TESTED", "no candidate exists for the attempt after the generation-2 claim", evidence)
    if len(fresh) != 1:
        return Verdict("NOT_TESTED", f"{len(fresh)} candidates exist for the attempt after the claim", evidence)
    candidate = fresh[0]
    evidence["candidate"] = candidate
    for field_name in ("provider", "model", "provider_mode"):
        actual = getattr(observation, f"gen2_{field_name}", "")
        expected = getattr(observation, f"expected_{field_name}", "")
        if not actual or (expected and actual != expected):
            return Verdict("NOT_TESTED", f"gen2 session route {field_name}={actual!r} expected {expected!r}", evidence)
    if not _is_git_sha(observation.candidate_sha) or not _is_git_sha(observation.candidate_tree) or not _is_git_sha(observation.candidate_base):
        return Verdict("NOT_TESTED", "candidate sha/tree/base are not exact Git object ids", evidence)
    if observation.expected_base_commit and observation.candidate_base != observation.expected_base_commit:
        return Verdict(
            "NOT_TESTED",
            f"candidate base {observation.candidate_base} differs from the frozen ActionSpec base {observation.expected_base_commit}",
            evidence,
        )
    if not observation.candidate_id or str(candidate.get("id") or "") != observation.candidate_id:
        return Verdict("NOT_TESTED", "the candidate detail is not the same post-claim candidate", evidence)
    if observation.gate_candidate_id != observation.candidate_id:
        return Verdict("NOT_TESTED", "the gate does not reference the same candidate", evidence)
    if observation.expected_action and observation.campaign_action != observation.expected_action:
        return Verdict("NOT_TESTED", "the campaign does not reference the expected action", evidence)
    if observation.campaign_detail_id and observation.campaign_detail_id != observation.campaign_id:
        return Verdict("NOT_TESTED", "the Campaign detail is not the expected Campaign", evidence)
    if not observation.campaign_detail_id:
        return Verdict("NOT_TESTED", "the Campaign detail has no id", evidence)
    campaign_status = observation.campaign_status.upper()
    if campaign_status in {"FAILED", "BLOCKED", "CANCELLED", "REJECTED", "WITHDRAWN"}:
        return Verdict("FAIL", f"Campaign status is {campaign_status}", evidence)
    if campaign_status not in {"AUTHORIZED", "RUNNING", "COMPLETED"}:
        return Verdict("NOT_TESTED", f"Campaign status is {observation.campaign_status!r}", evidence)
    missing = []
    if observation.gate_outcome != "ACCEPTED":
        missing.append(f"quality gate={observation.gate_outcome or 'missing'}")
    if observation.restart_required and not observation.restart_observed:
        missing.append("required restart not observed")
    if missing:
        return Verdict(
            "OK_SCOPED",
            "P1-P3 recovered and verified; not full acceptance: " + "; ".join(missing),
            evidence,
        )
    return Verdict(
        "ACCEPTED",
        "generation-1 verified, pending work completed and verified in generation 2, candidate after the claim, Gate/Campaign satisfied",
        evidence,
    )


def authorization_problems(
    record: dict[str, Any],
    campaign_id: str,
    provider: str,
    model: str,
    provider_mode: str,
    allowed_paths: Iterable[str],
) -> list[str]:
    """Validate the durable V2 `CampaignAuthorizationRecord` JSON exactly.

    Core serializes this record with `rename_all = "camelCase"`; the policy is
    `ExecutionPolicyV2`, also camelCase. There is no snake_case alias: a missing
    canonical field is a real defect, not something to reinterpret.
    """

    schema_version = record.get("schemaVersion")
    authorization_id = str(record.get("authorizationId") or "")
    row_campaign = str(record.get("campaignId") or "")
    risk_class = str(record.get("riskClass") or "")
    execution_mode = str(record.get("executionMode") or "")
    delegation_id = str(record.get("delegationId") or "")
    row_provider = str(record.get("provider") or "")
    row_model = str(record.get("model") or "")
    row_mode = str(record.get("providerMode") or "")
    paths = record.get("authorizedPaths")
    policy = record.get("policy")
    envelope = ""
    if isinstance(policy, dict):
        envelope = str(policy.get("monetaryEnvelopeId") or "")
    problems = []
    if schema_version != 2:
        problems.append(f"schemaVersion={schema_version!r}")
    if not authorization_id:
        problems.append("authorizationId missing")
    if row_campaign != campaign_id:
        problems.append(f"campaignId={row_campaign!r}")
    if risk_class != "R0":
        problems.append(f"riskClass={risk_class!r}")
    if execution_mode != "TRUSTED_LOCAL_PROVIDER":
        problems.append(f"executionMode={execution_mode!r}")
    if row_provider != provider or row_model != model or row_mode != provider_mode:
        problems.append(f"route={row_provider}/{row_model}/{row_mode}")
    if not delegation_id:
        problems.append("delegationId missing")
    if not isinstance(paths, list) or sorted(paths) != sorted(allowed_paths):
        problems.append(f"authorizedPaths={paths!r}")
    if not isinstance(policy, dict):
        problems.append("policy missing")
    if not envelope:
        problems.append("policy.monetaryEnvelopeId missing")
    return problems


def environment_binding_problems(
    spec: dict[str, Any], checks: list[dict[str, Any]], environment: dict[str, Any]
) -> list[str]:
    """Every frozen check must execute in the exact frozen environment."""

    spec_sha = str(environment.get("specSha256") or "")
    if not spec_sha:
        return ["environment review has no frozen spec"]
    plan_id = str(environment.get("planId") or "")
    problems = []
    if str(spec.get("environment_plan_id") or "") != plan_id:
        problems.append("ActionSpec environment_plan_id")
    if str(spec.get("environment_spec_sha256") or "") != spec_sha:
        problems.append("ActionSpec environment_spec_sha256")
    for check in checks:
        if str(check.get("environment_plan_id") or "") != plan_id:
            problems.append(f"check {check.get('id')!r} environment_plan_id")
        if str(check.get("environment_spec_sha256") or "") != spec_sha:
            problems.append(f"check {check.get('id')!r} environment_spec_sha256")
    return problems


def contract_mismatch(approved_checks: list[dict[str, Any]]) -> str:
    """Return a causal mismatch string when the approved ActionSpec differs.

    The approved plan belongs to the Lead; the scenario only verifies that its
    frozen checks assert the exact contract. It never rewrites the plan.
    """

    by_id = {str(check.get("id") or ""): check for check in approved_checks}
    for wanted in (STATUS, VERSION):
        actual = by_id.get(wanted)
        if actual is None:
            return f"approved ActionSpec has no {wanted!r} check"
        command = actual.get("command") or []
        expected = CHECKS[wanted]["command"]
        if command != expected:
            return f"approved {wanted!r} check command differs from the exact contract: {command!r}"
        if int(actual.get("expectExit", -1)) != 0:
            return f"approved {wanted!r} check expectExit is not 0"
    return ""


def plan_document() -> dict[str, Any]:
    return {
        "objective": OBJECTIVE,
        "allowed_paths": ALLOWED_PATHS,
        "checks": [CHECKS[STATUS], CHECKS[VERSION]],
        "authorized_micros": DEFAULT_AUTHORIZED_MICROS,
        "phases": [
            "prepare disposable real-provider stack; verify the provider route and a new operator identity",
            "use the pre-run-bound project, execution profile and grant; post the Lead intake",
            "record the request-scoped Owner subscription admission, run the bounded assessment and approve the exact frozen plan",
            "verify the approved ActionSpec checks against the exact contract before any intervention",
            "wait for generation-1 status PASS; prove no generation-1 version PASS receipt exists (P2 pending)",
            "confirm generation-1 provider turn/check quiescence from Core facts",
            "kill the Worker, await EXPIRED, restart it, observe worker.claimed generation=2",
            "wait for the generation-2 version PASS receipt and a candidate after the claim",
            "run independent verifier/reviewer/gate per contract; capture evidence; clean up only copies",
        ],
        "p4": "the frozen ProgressSupervisor replay test remains the negative control",
        "funding": "Core projection decides: subscription admission (no-charge) or a test-support monetary envelope; never inferred from a flag",
    }


class Api:
    def __init__(self, base: str, token: str, timeout: float = 360.0) -> None:
        parts = urlsplit(base)
        if not parts.scheme or not parts.netloc:
            raise ScenarioError("core API URL must be absolute")
        # Core checks `Origin` against the scheme and authority; the API base
        # may legitimately carry a path such as `/core-api`, which must not be
        # copied into the Origin header.
        self.origin = f"{parts.scheme}://{parts.netloc}"
        self.base = base.rstrip("/")
        self.token = token
        self.last_request: tuple[str, str] = ("", "")
        self.last_error: tuple[int, str] | None = None
        # The bounded Lead assessment runs a real model call synchronously
        # (profile modelRequestTimeoutSecs=300), so the HTTP client must not
        # abort before Core's own timeout.
        self.timeout = timeout

    def _request(self, method: str, path: str, payload: Any | None = None) -> Any:
        self.last_request = (method, path)
        data = None if payload is None else json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            self.base + path,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "Origin": self.origin,
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                body = response.read().decode("utf-8")
                return json.loads(body) if body else None
        except urllib.error.HTTPError as error:
            detail = error.read().decode("utf-8", errors="replace")
            self.last_error = (error.code, detail[:1_000])
            raise

    def get(self, path: str) -> Any:
        return self._request("GET", path)

    def post(self, path: str, payload: dict[str, Any]) -> Any:
        return self._request("POST", path, payload)

    def put(self, path: str, payload: dict[str, Any]) -> Any:
        return self._request("PUT", path, payload)

    def post_status(self, path: str, payload: dict[str, Any]) -> tuple[int, Any]:
        self.last_request = ("POST", path)
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            self.base + path,
            data=data,
            method="POST",
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "Origin": self.origin,
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                body = response.read().decode("utf-8")
                return response.status, (json.loads(body) if body else None)
        except urllib.error.HTTPError as error:
            body = error.read().decode("utf-8")
            self.last_error = (error.code, body[:1_000])
            try:
                payload = json.loads(body) if body else None
            except json.JSONDecodeError:
                payload = body
            return error.code, payload


def _operator_token(path: Path) -> str:
    token = path.read_text(encoding="utf-8").strip()
    if not token:
        raise ScenarioError("operator token file is empty")
    return token


def _podman_compose(project: str, root: Path, git_dir: Path, args: list[str]) -> subprocess.CompletedProcess:
    env = os.environ.copy()
    env["FACTORY_RP_UAT_GIT_DIR"] = str(git_dir)
    return subprocess.run(
        [
            "podman", "compose", "-p", project,
            "-f", "compose.yaml", "-f", "compose.real-provider-uat.yaml", *args,
        ],
        cwd=root,
        env=env,
        text=True,
        capture_output=True,
    )


class Scenario:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.root = Path(args.root).resolve()
        self.evidence = Path(args.evidence_dir)
        self.api = Api(args.core_api, _operator_token(Path(args.operator_token_file)))
        self.git_dir = Path(args.git_dir)
        self.project = args.compose_project
        self.events_baseline = 0
        self.attempt = ""
        self.campaign = ""
        self.action = ""
        self.intake = ""
        self.killed = False
        self.authorization_record: dict[str, Any] = {}
        self.action_checks: list[dict[str, Any]] = []
        # Every ACCEPTED verdict requires a real restart-persistence
        # observation; the dimension is never inferred from a model field.
        self.restart_required = True
        self.core_record: dict[str, Any] = {}
        self.expected_command_sha256 = ""
        self.expected_status_command_sha256 = ""
        self.expected_base_commit = ""
        self.work_item_id = ""
        self.plan_hash = ""
        self.profile_revision_id = ""
        self.profile_hash = ""
        self.admission_id = ""
        self.assumption: dict[str, Any] = {}
        self.assumption_sha256 = ""
        self.admission_record: dict[str, Any] = {}
        self.lead_request_id = ""
        self.started_at = utc()
        self.started_ms = int(time.time() * 1000)
        self.assessment_projection: dict[str, Any] = {}
        self.environment_plan: dict[str, Any] = {}
        self.environment: dict[str, Any] = {}
        self.evidence.mkdir(parents=True, exist_ok=True)
        os.chmod(self.evidence, 0o700)

    # ---- evidence -----------------------------------------------------
    def write(self, name: str, payload: Any) -> None:
        path = self.evidence / name
        path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        os.chmod(path, 0o600)

    def log(self, message: str) -> None:
        line = f"[{utc()}] {message}"
        print(line, flush=True)
        with (self.evidence / "scenario.log").open("a", encoding="utf-8") as handle:
            handle.write(line + "\n")

    # ---- phases -------------------------------------------------------
    def prepare(self) -> None:
        # The pre-run binding owns the project, execution profile and grant.
        # This scenario consumes the exact profile and never creates one.
        profile = self.api.get(f"/execution-profiles/{self.args.profile_id}")
        problems = profile_mismatches(profile, provider=self.args.provider, model=self.args.model)
        if problems:
            raise ScenarioError(
                "bound execution profile is not the canonical profile: " + ", ".join(problems)
            )
        if int(profile.get("schemaVersion") or 0) != 2:
            raise ScenarioError(f"bound execution profile is not V2: {profile.get('schemaVersion')!r}")
        self.profile_revision_id = str(profile.get("revisionId") or "")
        self.profile_hash = str(profile.get("contentHash") or "")
        if not self.profile_revision_id or not _is_sha256(self.profile_hash):
            raise ScenarioError("bound execution profile has no frozen revision identity")
        self.write("execution-profile.json", profile)
        intake = {
            "project_id": self.args.project_id,
            "objective": OBJECTIVE,
            "execution_profile_id": self.args.profile_id,
            "mode": "REVIEW_PLAN",
            "idempotency_key": f"{self.args.authorization}:recovery-intake",
            "environment_constraints": ENVIRONMENT_CONSTRAINTS,
        }
        if self.args.authorized_micros > 0:
            intake["authorized_micros"] = self.args.authorized_micros
        created = self.api.post("/intake/requests", intake)
        self.intake = created["id"]
        self.action = created["actionId"]
        self.lead_request_id = str(created.get("leadRequestId") or "")
        self.write("intake.json", created)
        self.log(f"intake={self.intake} action={self.action}")

    def _profile_identity(self) -> None:
        """Prefer the create projection; fall back to the intake detail."""

        if self.profile_revision_id and _is_sha256(self.profile_hash):
            return
        projection = self.api.get(f"/intake/requests/{self.intake}")
        profile = projection.get("profile") or {}
        self.profile_revision_id = str(profile.get("revisionId") or "")
        self.profile_hash = str(profile.get("contentHash") or "")
        self.write("intake-detail.json", projection)
        if not self.profile_revision_id or not _is_sha256(self.profile_hash):
            raise ScenarioError("intake detail did not expose the frozen profile identity")

    def load_assumption(self) -> dict[str, Any]:
        """Read the Owner-provided subscription assumption metadata.

        The Owner's generic paid-run permission does not confirm the current
        Extra Usage switch. Only a bounded, non-secret metadata file produced
        from the Owner's current answer may create the admission. The file is
        never synthesized from elapsed time, an old permission or a default.
        """

        path = Path(self.args.assumption_file) if self.args.assumption_file else None
        if path is None or not path.is_file():
            raise ScenarioError(
                "no current Owner subscription assumption file; refusing to create an admission"
            )
        if path.stat().st_mode & 0o077:
            raise ScenarioError("the Owner assumption file must not be group/world readable")
        raw = path.read_bytes()
        import hashlib

        self.assumption_sha256 = hashlib.sha256(raw).hexdigest()
        try:
            data = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ScenarioError(f"the Owner assumption file is not valid JSON: {error}") from error
        if not isinstance(data, dict):
            raise ScenarioError("the Owner assumption file must be a JSON object")
        required = {
            "assumptionId",
            "confirmedBy",
            "confirmedAtMs",
            "validUntilMs",
            "extraUsage",
            "statement",
        }
        missing = sorted(required - set(data))
        unknown = sorted(set(data) - required)
        if missing:
            raise ScenarioError("Owner assumption file is missing: " + ", ".join(missing))
        if unknown:
            raise ScenarioError("Owner assumption file has unknown fields: " + ", ".join(unknown))
        extra_usage = str(data["extraUsage"])
        if extra_usage != "DISABLED":
            raise ScenarioError("the Owner assumption does not record Extra Usage as DISABLED")
        statement = str(data["statement"])
        if not (20 <= len(statement) <= 2_000):
            raise ScenarioError("the Owner assumption statement is not bounded")
        confirmed = data["confirmedAtMs"]
        valid_until = data["validUntilMs"]
        if not isinstance(confirmed, int) or not isinstance(valid_until, int):
            raise ScenarioError("the Owner assumption window must be integer milliseconds")
        now_ms = int(time.time() * 1000)
        if confirmed <= 0 or confirmed > now_ms:
            raise ScenarioError("the Owner assumption confirmation time is not a past instant")
        if valid_until <= now_ms or valid_until <= confirmed:
            raise ScenarioError("the Owner assumption is expired or has no validity window")
        self.assumption = data
        return data

    def admit(self) -> None:
        """Record the request-scoped Owner-confirmed subscription admission."""

        self._profile_identity()
        assumption = self.load_assumption()
        payload = {
            "assumption_id": str(assumption["assumptionId"]),
            "confirmed_by": str(assumption["confirmedBy"]),
            "confirmed_at_ms": int(assumption["confirmedAtMs"]),
            "valid_until_ms": int(assumption["validUntilMs"]),
            "extra_usage_disabled": True,
            "statement": str(assumption["statement"]),
            "test_profile_id": "opencode-go-subscription",
            "idempotency_key": f"{self.args.authorization}:admission:{self.intake}",
        }
        record = self.api.post(f"/intake/requests/{self.intake}/subscription-admission", payload)
        if not isinstance(record, dict):
            raise ScenarioError("Core returned no subscription admission object")
        self.admission_record = record
        self.admission_id = str(record.get("admissionId") or "")
        recorded = record.get("assumption") or {}
        routes = record.get("routes") or []
        problems = []
        if not self.admission_id:
            problems.append("admissionId")
        if str(record.get("state") or "") != "ACTIVE":
            problems.append(f"state={record.get('state')!r}")
        if str(record.get("intakeRequestId") or "") != self.intake:
            problems.append("intakeRequestId")
        if str(record.get("provider") or "") != self.args.provider:
            problems.append("provider")
        if str(record.get("model") or "") != self.args.model:
            problems.append("model")
        if str(record.get("profileRevisionId") or "") != self.profile_revision_id:
            problems.append("profileRevisionId")
        if str(record.get("profileHash") or "") != self.profile_hash:
            problems.append("profileHash")
        if str(recorded.get("assumptionId") or "") != str(assumption["assumptionId"]):
            problems.append("assumptionId")
        if str(recorded.get("extraUsage") or "") != "DISABLED":
            problems.append("extraUsage")
        recorded_by = str(recorded.get("confirmedBy") or "")
        file_by = str(assumption["confirmedBy"])
        # This driver authenticates with the operator token, so the exact
        # Core-recorded identity is `operator-token:<file>`. A future device
        # session must pass its real `device:<id>`; no generic suffix matches.
        if not confirmed_by_matches(recorded_by, file_by):
            problems.append("confirmedBy")
        if not _is_sha256(str(record.get("receiptSha256") or "")):
            problems.append("receiptSha256")
        try:
            if int(record.get("expiresAtMs") or 0) <= int(time.time() * 1000):
                problems.append("expiresAtMs")
        except (TypeError, ValueError):
            problems.append("expiresAtMs")
        exact_route = any(
            str(route.get("role") or "") == "IMPLEMENTER"
            and str(route.get("provider") or "") == self.args.provider
            and str(route.get("model") or "") == self.args.model
            and str(route.get("transport") or "") == "OPENCODE_GO_SUBSCRIPTION"
            for route in routes
        )
        if not exact_route:
            problems.append("implementer route/transport")
        if problems:
            raise ScenarioError("subscription admission invalid: " + ", ".join(problems))
        self.write("subscription-admission.json", {
            "record": record,
            "assumption_sha256": self.assumption_sha256,
        })
        self.log(f"admission={self.admission_id}")

    def _staged_assess_403_valid(self) -> dict[str, Any]:
        """A 403 is accepted only when the durable assessment already exists."""

        projection = self.api.get(f"/intake/requests/{self.intake}") or {}
        state = str(projection.get("state") or "")
        proposal = projection.get("proposal") or {}
        assessment = projection.get("assessment") or {}
        authorization = projection.get("authorization") or {}
        if state not in {"PLAN_READY", "AWAITING_APPROVAL"}:
            raise ScenarioError(f"staged assess 403 has state {state!r}")
        if not proposal or not assessment:
            raise ScenarioError("staged assess 403 has no durable proposal/assessment")
        if str(proposal.get("objective") or "") != OBJECTIVE:
            raise ScenarioError("staged assess 403 proposal objective is not exact")
        if str(authorization.get("reason") or "") != "PLAN_NOT_READY":
            raise ScenarioError(
                f"staged assess 403 reason is {authorization.get('reason')!r}"
            )
        rows = self._db_rows(
            "SELECT state FROM model_call_reservations "
            "WHERE project_id=?1 AND purpose='lead_assessment'",
            (self.args.project_id,),
        )
        if [str(row[0]) for row in rows] != ["SETTLED"]:
            raise ScenarioError(f"staged assess 403 reservations are {rows!r}")
        return projection

    def assess(self, timeout: int = 1800) -> None:
        """Run the single Lead assessment; the plan is frozen in a later step."""

        status, projection = self.api.post_status(f"/intake/requests/{self.intake}/assess", {})
        if status == 403:
            body = projection if isinstance(projection, dict) else {}
            self.write("assess-403.json", {
                "status": status,
                "code": str(body.get("code") or ""),
                "error": str(body.get("error") or ""),
            })
            projection = self._staged_assess_403_valid()
        elif status not in {200, 201}:
            raise ScenarioError(f"assessment returned HTTP {status}")
        state = str(projection.get("state") or "")
        if state not in {"PLAN_READY", "AWAITING_APPROVAL"}:
            raise ScenarioError(f"assessment state is {state!r}")
        plan = projection.get("plan") or {}
        self.plan_hash = str(plan.get("plan_hash") or "")
        profile = projection.get("profile") or {}
        self.profile_revision_id = str(profile.get("revisionId") or self.profile_revision_id)
        self.profile_hash = str(profile.get("contentHash") or self.profile_hash)
        if not self.profile_revision_id or not _is_sha256(self.profile_hash):
            raise ScenarioError("assessment did not expose the frozen profile identity")
        self.assessment_projection = projection
        self.write("assessment.json", projection)
        self.log(f"assessment_state={state} plan_hash={self.plan_hash or 'pending'}")

    def environment_recompile(self) -> None:
        """Compile the committed environment; no model call."""

        response = self.api.post(
            f"/agentic/requests/{self.intake}/environment/recompile",
            {"rationale": "The committed UAT flake must be compiled before plan approval."},
        )
        if not isinstance(response, dict):
            raise ScenarioError("environment recompile returned no projection")
        status = str(response.get("status") or "")
        plan_id = str(response.get("planId") or "")
        revision = response.get("planRevision")
        plan_sha = str(response.get("planSha256") or "")
        if status not in {"CORE_VALIDATED", "LEAD_APPROVED", "FROZEN"}:
            raise ScenarioError(f"environment recompile status is {status!r}")
        if not plan_id or not isinstance(revision, int) or revision <= 0 or not _is_sha256(plan_sha):
            raise ScenarioError("environment recompile did not expose plan identity")
        self.environment_plan = response
        self.write("environment-recompile.json", response)
        self.log(f"environment_plan={plan_id} revision={revision} status={status}")

    def environment_review(self) -> None:
        """Review and freeze the compiled environment; the Lead review is funded."""

        plan = self.environment_plan
        response = self.api.post(
            f"/agentic/requests/{self.intake}/environment/review",
            {
                "planSha256": str(plan.get("planSha256") or ""),
                "planRevision": int(plan.get("planRevision") or 0),
            },
        )
        if not isinstance(response, dict):
            raise ScenarioError("environment review returned no projection")
        status = str(response.get("status") or "")
        spec_sha = str(response.get("specSha256") or "")
        if status != "FROZEN" or not _is_sha256(spec_sha):
            raise ScenarioError(f"environment review did not freeze a spec: {status!r}")
        self.environment = response
        self.write("environment-review.json", response)
        self.log(f"environment_review={status} spec={spec_sha}")

    def plan_workgraph(self) -> None:
        """Run the Planner once and require a recorded participant WorkGraph."""

        response = self.api.post(f"/agentic/requests/{self.intake}/plan", {})
        if not isinstance(response, dict):
            raise ScenarioError("planner returned no plan projection")
        validated = response.get("validated") or {}
        units = validated.get("units") or []
        participant = response.get("participantPlan")
        if not units or not isinstance(participant, dict):
            raise ScenarioError("planner did not record a validated participant WorkGraph")
        self.write("planner.json", response)
        self.log("planner_workgraph=PRESENT")

    def freeze_lead_plan(self) -> None:
        """Freeze Lead plan/ActionSpec with the committed environment; no Lead call."""

        response = self.api.post(f"/lead/requests/{self.lead_request_id}/freeze", {})
        plan = response.get("plan") or {}
        plan_hash = str(plan.get("plan_hash") or "")
        if not _is_sha256(plan_hash):
            raise ScenarioError("lead freeze did not expose a SHA-256 plan hash")
        self.plan_hash = plan_hash
        self.write("lead-freeze.json", response)
        self.log(f"lead_plan_frozen={plan_hash}")

    def prepare_plan(self) -> None:
        """Environment compile/review -> Planner -> Lead freeze, then approve."""

        if self.plan_hash and (self.assessment_projection.get("environment") or {}).get("status") == "FROZEN":
            self.log("plan_environment=ALREADY_FROZEN")
            return
        self.environment_recompile()
        self.environment_review()
        self.plan_workgraph()
        before_freeze = self.model_purposes()
        self.write("model-purposes-before-freeze.json", self.purpose_evidence(before_freeze))
        self.freeze_lead_plan()
        after_freeze = self.model_purposes()
        self.write("model-purposes-after-freeze.json", self.purpose_evidence(after_freeze))
        if after_freeze != before_freeze:
            raise ScenarioError("lead freeze changed the model reservation pattern")
        self.verify_model_purposes()

    def approve(self) -> None:
        # Negative production probe: a monetary claim on the subscription path
        # must be refused before any Campaign start. It is not a substitute for
        # the real approval and cannot consume the admission.
        status, body = self.api.post_status(
            f"/intake/requests/{self.intake}/approve",
            {
                "expected_plan_hash": self.plan_hash,
                "rationale": "negative production monetary approval probe",
                "idempotency_key": f"{self.args.authorization}:negative-monetary-approve",
                "authorized_micros": max(1, self.args.authorized_micros or 5_000_000),
                "subscription_admission_id": self.admission_id,
            },
        )
        code = str((body or {}).get("code") or "") if isinstance(body, dict) else ""
        self.write("negative-monetary-approval.json", {"status": status, "body": body})
        if status != 409 or code != "MONETARY_AUTHORIZATION_NOT_APPLICABLE":
            raise ScenarioError(
                f"production monetary approval was not refused as expected: {status} {code!r}"
            )
        approved = self.api.post(
            f"/intake/requests/{self.intake}/approve",
            {
                "expected_plan_hash": self.plan_hash,
                "expected_profile_revision_id": self.profile_revision_id,
                "expected_profile_hash": self.profile_hash,
                "rationale": (
                    "Approved the exact frozen plan for the bounded real recovery UAT "
                    "under the Owner-confirmed subscription admission."
                ),
                "idempotency_key": f"{self.args.authorization}:approve",
                "subscription_admission_id": self.admission_id,
            },
        )
        campaign_id = approved.get("campaignId") or (approved.get("campaign") or {}).get("id")
        if not campaign_id or str(campaign_id) == "None":
            raise ScenarioError("intake approval returned no Campaign")
        self.campaign = str(campaign_id)
        self.write("approval.json", approved)
        self.log(f"campaign={self.campaign}")

    def verify_contract(self) -> None:
        # The intake projection does not carry the ActionSpec; `/actions` is the
        # authenticated Core surface that does. The Action must be the one the
        # Campaign was created for and its frozen checks must match exactly.
        projection = self.api.get(f"/intake/requests/{self.intake}")
        actions = self.api.get("/actions") or []
        action = next((row for row in actions if str(row.get("id")) == self.action), None)
        if not isinstance(action, dict):
            raise ScenarioError("Core has no Action for the intake projection")
        spec = action.get("spec") or {}
        checks = spec.get("checks") or []
        self.action_checks = checks
        self.expected_command_sha256 = command_sha256(CHECKS[VERSION]["command"])
        self.expected_status_command_sha256 = command_sha256(CHECKS[STATUS]["command"])
        base_commit = str(spec.get("base_commit") or "")
        if not _is_git_sha(base_commit):
            raise ScenarioError(f"approved ActionSpec base_commit is not an exact Git id: {base_commit!r}")
        self.expected_base_commit = base_commit
        self.write("approved-action.json", action)
        mismatch = contract_mismatch(checks)
        if mismatch:
            raise ScenarioError(f"approved ActionSpec mismatch: {mismatch}")
        if str(spec.get("provider") or "") != self.args.provider or str(spec.get("model") or "") != self.args.model:
            raise ScenarioError(
                f"approved ActionSpec route differs: {spec.get('provider')}/{spec.get('model')}"
            )
        if sorted(spec.get("allowed_paths") or []) != sorted(ALLOWED_PATHS):
            raise ScenarioError(f"approved ActionSpec allowed_paths differ: {spec.get('allowed_paths')!r}")
        env_problems = environment_binding_problems(spec, checks, self.environment or {})
        if self.environment and env_problems:
            raise ScenarioError("environment binding invalid: " + ", ".join(env_problems))

        record = self.api.get(f"/campaigns/{self.campaign}/authorization")
        if not isinstance(record, dict):
            raise ScenarioError("Core authorization record is not an object")
        self.authorization_record = record
        self.write("authorization-record.json", record)
        problems = authorization_problems(
            record,
            self.campaign,
            self.args.provider,
            self.args.model,
            "build",
            ALLOWED_PATHS,
        )
        if problems:
            raise ScenarioError("Core authorization record invalid: " + "; ".join(problems))
        authorization_id = str(record.get("authorizationId") or "")

        # The monetary amount and owner reference must come from Core, not from
        # the caller's flags. The intake projection exposes the request envelope
        # when the request was created with an explicit monetary authorization.
        spending = projection.get("spending")
        envelope = (spending or {}).get("envelope") if isinstance(spending, dict) else None
        policy = record.get("policy") if isinstance(record.get("policy"), dict) else {}
        policy_envelope = str(policy.get("monetaryEnvelopeId") or "")
        if policy_envelope == NO_BILLABLE_ENVELOPE:
            if envelope:
                raise ScenarioError("subscription policy carries a billable intake envelope")
            if self.args.authorized_micros != 0:
                raise ScenarioError("a positive amount was claimed for a subscription-funded request")
            funding_mode = "REAL_SUBSCRIPTION"
        else:
            if not envelope:
                raise ScenarioError("a monetary policy has no intake spending envelope")
            envelope_id = str(envelope.get("envelopeId") or "")
            amount = envelope.get("authorizedMicros")
            if envelope_id != policy_envelope:
                raise ScenarioError(
                    f"intake envelope {envelope_id!r} differs from policy {policy_envelope!r}"
                )
            if amount != self.args.authorized_micros or amount <= 0:
                raise ScenarioError(
                    f"intake envelope amount {amount!r} differs from the claimed {self.args.authorized_micros}"
                )
            funding_mode = "MONETARY_ENVELOPE"
        if funding_mode == "REAL_SUBSCRIPTION":
            if not self.admission_id:
                raise ScenarioError("the subscription Campaign has no request-scoped admission id")
            assumption = (self.admission_record.get("assumption") or {})
            owner_reference = (
                f"{assumption.get('confirmedBy') or ''}#{assumption.get('assumptionId') or ''}"
            )
            if not owner_reference.strip("#"):
                raise ScenarioError("the admission has no Owner assumption identity")
        else:
            owner_reference = policy_envelope
        if not owner_reference:
            raise ScenarioError("Core did not project an owner funding reference")

        authorized_event = next(
            (
                event
                for event in reversed(self.events())
                if event.get("kind") == "campaign.authorized" and str(event.get("subject_id")) == self.campaign
            ),
            {},
        )
        core_record = {
            "authorization_id": authorization_id,
            "owner_reference": owner_reference,
            "funding_mode": funding_mode,
            "admission_id": self.admission_id,
            "assumption_sha256": self.assumption_sha256,
            "api_key_route_disabled": "NOT_TESTED_NO_VISIBLE_SURFACE",
            "no_extra_metered": "NOT_TESTED_OWNER_CONFIRMATION_PENDING",
            "provider": self.args.provider,
            "model": self.args.model,
            "caller_reference": self.args.authorization,
            "event_sequence": int(authorized_event.get("sequence") or 0),
            "recorded_at": str(authorized_event.get("recorded_at") or record.get("createdAt") or ""),
            "action_id": self.action,
            "spec_id": str(action.get("id") or ""),
            "plan_hash": self.plan_hash,
            "spec_binding_hash": str(action.get("spec_sha256") or ""),
            "base_commit": base_commit,
            "core_path": f"/campaigns/{self.campaign}/authorization",
            "core_path_sha256": __import__("hashlib").sha256(
                json.dumps(record, sort_keys=True, separators=(",", ":")).encode("utf-8")
            ).hexdigest(),
        }
        if not core_record["spec_id"] or not _is_git_sha(core_record["base_commit"]):
            raise ScenarioError("Core action identity or frozen base commit is missing")
        if not core_record["spec_binding_hash"]:
            raise ScenarioError("Core action spec binding hash is missing")
        self.core_record = core_record
        self.write("core-authorization-record.json", core_record)
        if core_record["event_sequence"] <= 0 or not core_record["recorded_at"]:
            raise ScenarioError("Core authorization event sequence/time missing")
        if funding_mode == "REAL_SUBSCRIPTION":
            self.verify_admission_binding()

        # Negative: a direct TRUSTED_LOCAL_PROVIDER authorization must be refused.
        status, body = self.api.post_status(
            f"/campaigns/{self.campaign}/authorize",
            {
                "authorization_id": f"{self.args.authorization}:negative",
                "risk_class": "R0",
                "execution_mode": "TRUSTED_LOCAL_PROVIDER",
                "actor": "operator",
                "rationale": "negative direct provider authorization probe",
                "authorized_paths": ALLOWED_PATHS,
                "provider": self.args.provider,
                "model": self.args.model,
                "provider_mode": "build",
                "operations": {
                    "modelRequestTimeoutSecs": 300,
                    "toolCallTimeoutSecs": 600,
                    "maxRequestBytes": 2_097_152,
                    "maxResponseBytes": 2_097_152,
                    "maxGenerationTokens": 32_768,
                    "maxInFlightOperations": 2,
                },
                "progress": {
                    "diagnoseAfterStagnantCheckpoints": 2,
                    "escalateAfterUnsuccessfulReplans": 1,
                    "observationTimeoutSecs": 300,
                },
                "authorized_micros": self.args.authorized_micros,
            },
        )
        code = str((body or {}).get("code") or "") if isinstance(body, dict) else ""
        self.write("negative-direct-authorize.json", {"status": status, "body": body})
        refused = status == 403 or (status == 409 and code == "ENVIRONMENT_PLAN_REQUIRED")
        if not refused:
            raise ScenarioError(
                f"direct TRUSTED_LOCAL_PROVIDER authorization was not refused: {status} {code!r}"
            )

    def campaign_detail(self) -> dict[str, Any]:
        detail = self.api.get(f"/campaigns/{self.campaign}") or {}
        if not self.work_item_id:
            tasks = detail.get("tasks") or []
            self.work_item_id = str((tasks[0] if tasks else {}).get("unit_id") or "")
        return detail

    def attempt_id(self) -> str:
        detail = self.campaign_detail()
        tasks = detail.get("tasks") or []
        attempts = detail.get("attempts") or []
        if tasks and tasks[0].get("attempt_id"):
            return str(tasks[0]["attempt_id"])
        if attempts:
            return str(attempts[0]["id"])
        return ""

    def events(self) -> list[dict[str, Any]]:
        return self.api.get(f"/events?after={self.events_baseline}&limit=500") or []

    def watch_gen1(self, timeout: int = 3600) -> dict[str, Any]:
        """Wait for the status PASS and prove the P2 pending barrier."""
        deadline = time.monotonic() + timeout
        settle_deadline: float | None = None
        self.events_baseline = max((int(e.get("sequence") or 0) for e in self.events()), default=0)
        while time.monotonic() < deadline:
            events = self.events()
            attempt = self.attempt_id()
            if attempt:
                self.attempt = attempt
            version_outcome = self._version_outcome(1)
            if self._status_passed(1) and version_outcome == "PASS":
                self.write("result.json", {
                    "status": "NOT_TESTED",
                    "reason": "generation 1 already completed the pending version result",
                    "captured_utc": utc(),
                })
                raise ScenarioInvalid("generation 1 already completed both results")
            if self._status_passed(1) and version_outcome != "PASS":
                if settle_deadline is None:
                    # The model may still be settling the turn that produced the
                    # status result; wait for real quiescence, then fail closed.
                    settle_deadline = time.monotonic() + 180
                snapshot = self._pre_intervention_snapshot(events)
                self.write("pre-intervention.json", snapshot)
                failures = barrier_failures(snapshot)
                hard = [
                    failure for failure in failures
                    if failure.startswith(("the P2 check", "P2 artifacts", "candidate_count"))
                ]
                if hard or (failures and time.monotonic() > settle_deadline):
                    self.write("result.json", {
                        "status": "NOT_TESTED",
                        "reason": "pre-intervention barrier: " + "; ".join(failures),
                        "snapshot": snapshot,
                        "captured_utc": utc(),
                    })
                    raise ScenarioInvalid("pre-intervention barrier failed: " + "; ".join(failures))
                if not failures:
                    return snapshot
            for event in events:
                if event.get("kind") == "run.status_changed" and "status=FAILED" in str(event.get("detail") or ""):
                    raise ScenarioError(f"generation 1 failed before the intervention: {event.get('detail')}")
            time.sleep(3)
        raise ScenarioError("timed out waiting for the generation-1 status PASS")

    def _pre_intervention_snapshot(self, events: list[dict[str, Any]]) -> dict[str, Any]:
        detail = self.campaign_detail()
        attempt_status = ""
        for attempt in detail.get("attempts") or []:
            if str(attempt.get("id")) == self.attempt:
                attempt_status = str(attempt.get("status") or "")
        sessions = [
            s for s in (self.api.get("/provider/sessions") or [])
            if str(s.get("attempt_id")) == self.attempt and int(s.get("generation") or 0) == 1
        ]
        tool_events: list[dict[str, Any]] = []
        for session in sessions:
            record_id = session.get("session_record_id")
            if record_id:
                tool_events.extend(
                    tool_event_rows(self.api.get(f"/provider/sessions/{record_id}/tool_events"))
                )
        turn_authorized = [e for e in events if e.get("kind") == "provider.turn_authorized" and str(e.get("subject_id")) == self.attempt]
        turn_settled = [e for e in events if e.get("kind") == "provider.turn_settled" and str(e.get("subject_id")) == self.attempt]
        active_turns = max(0, len(turn_authorized) - len(turn_settled))
        attempt_events = [e for e in events if str(e.get("subject_id")) == self.attempt]
        last_event_at = str((attempt_events or [{}])[-1].get("recorded_at") or "")
        last_turn_event_at = str((turn_settled or turn_authorized or [{}])[-1].get("recorded_at") or "")
        participants = self.api.get(f"/office/participants?campaign_id={self.campaign}&limit=200") or {}
        rows = participants.get("participants") if isinstance(participants, dict) else participants
        implementer = next(
            (row for row in (rows or []) if str(row.get("attempt_id")) == self.attempt and str(row.get("role") or "").upper() == "IMPLEMENTER"),
            {},
        )
        version_receipts = _receipts_for(self._receipts(1), VERSION, 1)
        version_pass = any(r.outcome == "PASS" for r in version_receipts)
        candidates = [c for c in (self.api.get("/candidates") or []) if str(c.get("attempt_id")) == self.attempt]
        health = {service: self._container_health(service) for service in ("runtime", "supervisor", "worker")}
        tools_settled = tool_calls_settled(tool_events)
        gen1_quiescent = (
            bool(sessions)
            and all(str(s.get("state") or "").upper() in {"ACTIVE", "COMPLETED"} for s in sessions)
            and all(str(s.get("turn_id") or "").strip() for s in sessions)
            and active_turns == 0
            and bool(last_turn_event_at)
        )
        lease_state = str(implementer.get("lease_state") or "")
        return {
            "observed_at": utc(),
            "status_pass": True,
            "version_pass_absent": not version_pass,
            "attempt_status": attempt_status,
            "gen1_sessions": sessions,
            "gen1_tool_events": tool_events,
            "last_turn_event_at": last_turn_event_at,
            "last_event_at": last_event_at,
            "turn_authorized": len(turn_authorized),
            "turn_settled": len(turn_settled),
            "active_turns": active_turns,
            "lease_state": lease_state,
            "runtime_health": health["runtime"],
            "supervisor_health": health["supervisor"],
            "worker_health": health["worker"],
            "gen1_quiescent": gen1_quiescent,
            "tools_settled": tools_settled,
            "quiescence_backend_confirmed": quiescence_confirmed(
                sessions,
                tool_events,
                len(turn_authorized),
                len(turn_settled),
                health,
            ),
            "p2_check_absent": not version_pass and not version_receipts,
            "p2_artifacts_absent": not version_receipts,
            "candidate_count": len(candidates),
            "version_outcome": self._version_outcome(1),
        }

    def _container_health(self, service: str) -> str:
        name = f"{self.project}_{service}_1"
        result = subprocess.run(
            ["podman", "inspect", name, "--format", "{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}"],
            text=True,
            capture_output=True,
        )
        if result.returncode != 0:
            return "missing"
        return result.stdout.strip() or "unknown"

    def _status_passed(self, generation: int) -> bool:
        return _find_passing(self._receipts(generation), STATUS, generation) is not None

    def _version_outcome(self, generation: int) -> str:
        receipts = _receipts_for(self._receipts(generation), VERSION, generation)
        if any(r.outcome == "PASS" for r in receipts):
            return "PASS"
        if receipts:
            return receipts[-1].outcome
        return ""

    def _receipts(self, generation: int) -> list[CheckReceipt]:
        """Read durable Core execution receipts from the disposable DB copy.

        Harness-only evidence source until a read-only receipt projection exists.
        The independent verifier does not rely on this helper.
        """
        rows = self._db_rows(
            "SELECT attempt_id,generation,check_id,outcome,candidate_sha256,receipt_id,created_at,"
            "command_sha256,session_record_id,sequence,call_id,work_item_id "
            "FROM execution_check_receipts WHERE attempt_id=?1 AND generation=?2 ORDER BY rowid",
            (self.attempt, generation),
        )
        return [CheckReceipt(*row) for row in rows]

    def _db_rows_with_columns(
        self, sql: str, params: tuple[Any, ...]
    ) -> tuple[list[str], list[tuple[Any, ...]]]:
        import sqlite3

        database = self.args.core_database
        if not database:
            raise ScenarioError("--core-database is required for durable inspection")
        path = Path(database).resolve()
        if path.name != "factory.sqlite" or self.project not in str(path):
            raise ScenarioError("--core-database must be the disposable UAT project's own factory.sqlite")
        connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        try:
            cursor = connection.execute(sql, params)
            columns = [description[0] for description in cursor.description or []]
            return columns, list(cursor)
        finally:
            connection.close()

    def _db_rows(self, sql: str, params: tuple[Any, ...]) -> list[tuple[Any, ...]]:
        import sqlite3

        database = self.args.core_database
        if not database:
            raise ScenarioError("--core-database is required for durable receipt inspection")
        path = Path(database).resolve()
        if path.name != "factory.sqlite" or self.project not in str(path):
            raise ScenarioError("--core-database must be the disposable UAT project's own factory.sqlite")
        connection = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        try:
            connection.row_factory = None
            return list(connection.execute(sql, params))
        finally:
            connection.close()

    def intervene(self, snapshot: dict[str, Any]) -> dict[str, Any]:
        # Revalidate the whole barrier immediately before the kill.
        current = self._pre_intervention_snapshot(self.events())
        failures = barrier_failures(current)
        if failures:
            self.write("result.json", {
                "status": "NOT_TESTED",
                "reason": "pre-kill barrier changed: " + "; ".join(failures),
                "snapshot": current,
                "captured_utc": utc(),
            })
            raise ScenarioInvalid("pre-kill barrier failed: " + "; ".join(failures))
        current["kill_started_at"] = utc()
        result = _podman_compose(self.project, self.root, self.git_dir, ["kill", "worker"])
        self.killed = True
        current["kill_at"] = utc()
        self.write("pre-intervention.json", current)
        self.log(f"kill_worker exit={result.returncode} at={current['kill_at']} started={current['kill_started_at']}")
        if result.returncode != 0:
            raise ScenarioError(f"kill worker failed: {result.stderr.strip()[:160]}")
        # Await the lease expiry from the participant projection, the only
        # surface that exposes `lease_state`; never sleep blindly.
        deadline = time.monotonic() + 180
        expired = False
        while time.monotonic() < deadline:
            page = self.api.get(
                f"/office/participants?campaign_id={self.campaign}&role=IMPLEMENTER&limit=200"
            ) or {}
            rows = page.get("participants") if isinstance(page, dict) else page
            implementer = next(
                (row for row in (rows or []) if str(row.get("attempt_id")) == self.attempt),
                {},
            )
            if str(implementer.get("lease_state") or "") in {"EXPIRED", "RELEASED"}:
                expired = True
                break
            time.sleep(3)
        if not expired:
            self.write("result.json", {
                "status": "NOT_TESTED",
                "reason": "the generation-1 lease did not expire after the worker kill",
                "snapshot": current,
                "captured_utc": utc(),
            })
            raise ScenarioInvalid("generation-1 lease did not expire")
        result = _podman_compose(self.project, self.root, self.git_dir, ["start", "worker"])
        self.log(f"start_worker exit={result.returncode} at={utc()}")
        if result.returncode != 0:
            raise ScenarioError(f"start worker failed: {result.stderr.strip()[:160]}")
        return current

    def watch_gen2(self, snapshot: dict[str, Any], timeout: int = 3600) -> Observation:
        deadline = time.monotonic() + timeout
        claimed = None
        candidates: list[dict[str, Any]] = []
        while time.monotonic() < deadline:
            events = self.events()
            for event in events:
                if (
                    event.get("kind") == "worker.claimed"
                    and str(event.get("subject_id")) == self.attempt
                    and "generation=2" in str(event.get("detail") or "")
                ):
                    claimed = event
            sessions = self.api.get("/provider/sessions") or []
            gen2 = [
                s for s in sessions
                if str(s.get("attempt_id")) == self.attempt and int(s.get("generation") or 0) == 2
            ]
            candidates = [
                c for c in (self.api.get("/candidates") or [])
                if str(c.get("attempt_id")) == self.attempt
            ]
            if claimed and gen2:
                state = str(gen2[0].get("state") or "")
                receipts = self._receipts(2)
                version_pass = _find_passing(receipts, VERSION, 2) is not None
                if version_pass and candidates:
                    break
                if state.upper() in {"FAILED", "CANCELLED"}:
                    break
            time.sleep(3)
        sessions = self.api.get("/provider/sessions") or []
        gen2 = next(
            (s for s in sessions if str(s.get("attempt_id")) == self.attempt and int(s.get("generation") or 0) == 2),
            {},
        )
        claim_at = str((claimed or {}).get("recorded_at") or "")
        kill_started = str(snapshot.get("kill_started_at") or snapshot.get("kill_at") or "")
        if claimed is not None and (not kill_started or claim_at < kill_started):
            self.write("result.json", {
                "status": "NOT_TESTED",
                "reason": "the generation-2 claim did not follow the recorded kill_started_at",
                "snapshot": snapshot,
                "claimed_at": claim_at,
                "captured_utc": utc(),
            })
            raise ScenarioInvalid("kill/claim order invalid")

        candidate = candidates[0] if candidates else {}
        detail: dict[str, Any] = {}
        inspection: dict[str, Any] = {}
        if candidate.get("id"):
            detail = self.api.get(f"/quality/candidates/{candidate['id']}") or {}
            try:
                inspection = self.api.get(f"/quality/candidates/{candidate['id']}/inspection") or {}
            except Exception:  # noqa: BLE001 - base commit is NOT_TESTED without it
                inspection = {}
        gate = self._gate_detail(str(candidate.get("id") or ""))
        campaign = self.campaign_detail().get("campaign") or {}
        gen1_ids = {str(s.get("session_record_id")) for s in snapshot.get("gen1_sessions") or [] if s.get("session_record_id")}

        observation = Observation(
            attempt_id=self.attempt,
            campaign_id=self.campaign,
            claimed_generation_two=claimed is not None,
            claimed_at=claim_at,
            generation_two_session_state=str(gen2.get("state") or ""),
            generation_two_session_error=str(gen2.get("error") or ""),
            generation_one_receipts=self._receipts(1),
            generation_two_receipts=self._receipts(2),
            pre_intervention_version_outcome=self._pre_intervention_outcome(),
            candidates_after_claim=[
                c for c in candidates if _is_after(str(c.get("created_at") or ""), _parse_iso(claim_at))
            ],
            allocation_conflict=any("allocation conflict" in str(e.get("detail") or "") for e in self.events()),
            gen2_provider=str(gen2.get("provider") or ""),
            gen2_model=str(gen2.get("model") or ""),
            gen2_provider_mode=str(gen2.get("provider_mode") or ""),
            gate_outcome=str((gate or {}).get("outcome") or ""),
            candidate_sha=str(detail.get("commit_sha") or ""),
            candidate_tree=str(detail.get("tree_sha") or ""),
            candidate_base=str(inspection.get("base_commit") or ""),
            campaign_status=str(campaign.get("status") or ""),
            restart_required=self.restart_required,
            restart_observed=False,
            late_effect_risk=classify_late_effects(self.events(), kill_started, claim_at, gen1_ids)[0],
            kill_at=str(snapshot.get("kill_at") or ""),
            kill_started_at=kill_started,
            expected_provider=self.args.provider,
            expected_model=self.args.model,
            expected_provider_mode="build",
            expected_work_item=self.work_item_id,
            expected_command_sha256=self.expected_command_sha256,
            gen2_session_record_id=str(gen2.get("session_record_id") or ""),
            candidate_id=str(candidate.get("id") or ""),
            gate_candidate_id=str((gate or {}).get("candidate_id") or ""),
            campaign_action=str(campaign.get("action_id") or ""),
            expected_action=self.action,
            expected_base_commit=self.expected_base_commit,
            expected_status_command_sha256=self.expected_status_command_sha256,
            campaign_detail_id=str(campaign.get("id") or ""),
            gen1_session_record_ids=[
                str(s.get("session_record_id"))
                for s in snapshot.get("gen1_sessions") or []
                if s.get("session_record_id")
            ],
        )
        return observation

    def _restart_fingerprint(self, observation: Observation) -> dict[str, Any]:
        detail = self.api.get(f"/quality/candidates/{observation.candidate_id}") or {}
        gate = self._gate_detail(observation.candidate_id)
        campaign = self.campaign_detail().get("campaign") or {}
        attempt = next(
            (row for row in (self.campaign_detail().get("attempts") or []) if str(row.get("id")) == self.attempt),
            {},
        )
        return {
            "candidate_sha": str(detail.get("commit_sha") or ""),
            "candidate_tree": str(detail.get("tree_sha") or ""),
            "gate_outcome": str(gate.get("outcome") or ""),
            "gate_decided_at": str(gate.get("decided_at") or ""),
            "campaign_status": str(campaign.get("status") or ""),
            "attempt_status": str(attempt.get("status") or ""),
        }

    def verify_restart(self, observation: Observation) -> bool:
        """Restart the disposable stack and prove the exact state survived."""

        if not observation.candidate_id:
            return False
        before = self._restart_fingerprint(observation)
        if not before["candidate_sha"]:
            return False
        result = _podman_compose(self.project, self.root, self.git_dir, ["restart", "core", "supervisor", "worker"])
        self.write("restart.json", {"phase": "requested", "exit": result.returncode, "before": before, "at": utc()})
        if result.returncode != 0:
            return False
        deadline = time.monotonic() + 300
        healthy = False
        while time.monotonic() < deadline:
            healthy = all(
                self._container_health(service) == "healthy"
                for service in ("runtime", "supervisor", "worker")
            )
            if healthy:
                break
            time.sleep(3)
        after = self._restart_fingerprint(observation) if healthy else {}
        observed = healthy and after == before
        self.write("restart.json", {
            "phase": "observed",
            "healthy": healthy,
            "before": before,
            "after": after,
            "observed": observed,
            "at": utc(),
        })
        return observed

    def verify_admission_binding(self) -> None:
        """Bind approval -> admission -> Campaign from durable Core rows."""

        rows = self._db_rows(
            "SELECT admission_id,campaign_id,state,provider,model,profile_revision_id,"
            "profile_hash,assumption_json,routes_json,receipt_sha256,expires_at_ms "
            "FROM subscription_scope_admissions WHERE admission_id=?1",
            (self.admission_id,),
        )
        if len(rows) != 1:
            raise ScenarioError(
                f"expected one subscription admission row, found {len(rows)}"
            )
        (
            admission_id, campaign_id, state, provider, model, revision, profile_hash,
            assumption_json, routes_json, receipt_sha256, expires_at_ms,
        ) = rows[0]
        import json as _json
        assumption = _json.loads(assumption_json)
        routes = _json.loads(routes_json)
        problems = []
        if str(admission_id) != self.admission_id:
            problems.append("admission_id")
        if str(campaign_id) != self.campaign:
            problems.append(f"campaign_id={campaign_id!r}")
        if str(state) not in {"ACTIVE", "CONSUMED"}:
            problems.append(f"state={state!r}")
        if str(provider) != self.args.provider or str(model) != self.args.model:
            problems.append(f"route={provider}/{model}")
        if str(revision) != self.profile_revision_id or str(profile_hash) != self.profile_hash:
            problems.append("profile identity")
        if str(assumption.get("extraUsage") or "") != "DISABLED":
            problems.append("extraUsage")
        if not _is_sha256(str(receipt_sha256)):
            problems.append("receipt_sha256")
        if not any(
            str(route.get("role") or "") == "IMPLEMENTER"
            and str(route.get("provider") or "") == self.args.provider
            and str(route.get("model") or "") == self.args.model
            and str(route.get("transport") or "") == "OPENCODE_GO_SUBSCRIPTION"
            for route in routes
        ):
            problems.append("implementer route/transport")
        if int(expires_at_ms) <= int(time.time() * 1000):
            problems.append("expired")
        self.write("admission-binding.json", {
            "admission_id": admission_id,
            "campaign_id": campaign_id,
            "state": state,
            "provider": provider,
            "model": model,
            "profile_revision_id": revision,
            "profile_hash": profile_hash,
            "extra_usage": assumption.get("extraUsage"),
            "receipt_sha256": receipt_sha256,
            "expires_at_ms": expires_at_ms,
        })
        if problems:
            raise ScenarioError("admission binding invalid: " + ", ".join(problems))

    def _failure_db_rows(self) -> dict[str, Any]:
        """Read durable model-call/assessment state without touching prompts.

        A failed critical query is explicit evidence of NOT_CAPTURED; it is
        never treated as "no model call".
        """

        result: dict[str, Any] = {
            "calls": [],
            "agentic_assessments": None,
            "lead_assessments": None,
            "provider_sessions": None,
            "execution_check_receipts": None,
            "db_errors": {},
            "critical_error": "",
        }
        critical = {
            "model_call_reservations": (
                "SELECT role,state,purpose,provider,model,created_at FROM model_call_reservations "
                "WHERE project_id=?1 ORDER BY created_at",
                (self.args.project_id,),
            ),
            "agentic_assessments": (
                "SELECT count(*) FROM agentic_assessments WHERE request_id=?1",
                (self.intake,),
            ),
            "lead_assessments": (
                "SELECT count(*) FROM lead_assessments WHERE request_id=?1",
                (self.lead_request_id or self.intake,),
            ),
        }
        for name, (sql, params) in critical.items():
            try:
                rows = self._db_rows(sql, params)
                if name == "model_call_reservations":
                    result["calls"] = rows
                else:
                    result[name] = int(rows[0][0])
            except Exception as error:  # noqa: BLE001
                result["db_errors"][name] = f"{type(error).__name__}: {error}"
                if not result["critical_error"]:
                    result["critical_error"] = name
        try:
            result["provider_sessions"] = int(
                self._db_rows(
                    "SELECT count(*) FROM provider_sessions WHERE attempt_id IS NOT NULL "
                    "AND generation>=1",
                    (),
                )[0][0]
            )
        except Exception as error:  # noqa: BLE001
            result["db_errors"]["provider_sessions"] = f"{type(error).__name__}: {error}"
        try:
            result["execution_check_receipts"] = int(
                self._db_rows("SELECT count(*) FROM execution_check_receipts", ())[0][0]
            )
        except Exception as error:  # noqa: BLE001
            result["db_errors"]["execution_check_receipts"] = f"{type(error).__name__}: {error}"
        return result

    def model_purposes(self) -> dict[str, Any]:
        rows = self._db_rows(
            "SELECT purpose,state FROM model_call_reservations WHERE project_id=?1",
            (self.args.project_id,),
        )
        purposes: dict[str, int] = {}
        states: dict[str, list[str]] = {}
        for purpose, state in rows:
            key = str(purpose)
            purposes[key] = purposes.get(key, 0) + 1
            states.setdefault(key, []).append(str(state))
        return {"purposes": purposes, "states": states}

    def purpose_evidence(self, snapshot: dict[str, Any]) -> dict[str, Any]:
        expected = ("lead_assessment", "lead_environment_review", "agentic_planner")
        return {
            "purposes": snapshot["purposes"],
            "states": snapshot["states"],
            "expected": {purpose: {"count": 1, "state": "SETTLED"} for purpose in expected},
            "captured_utc": utc(),
        }

    def verify_model_purposes(self) -> dict[str, Any]:
        """Exactly one settled reservation per required purpose; no ambiguity."""

        snapshot = self.model_purposes()
        purposes = snapshot["purposes"]
        states = snapshot["states"]
        expected = ("lead_assessment", "lead_environment_review", "agentic_planner")
        problems = []
        for purpose in expected:
            if purposes.get(purpose, 0) != 1:
                problems.append(f"{purpose}={purposes.get(purpose, 0)}")
            elif states.get(purpose) != ["SETTLED"]:
                problems.append(f"{purpose} states={states.get(purpose)!r}")
        if purposes.get("lead_assessment", 0) > 1:
            problems.append("second lead_assessment")
        evidence = self.purpose_evidence(snapshot)
        self.write("model-purposes.json", evidence)
        if problems:
            raise ScenarioError("model purpose contract invalid: " + ", ".join(problems))
        return evidence

    def reconcile_failure(self, settle_timeout: float = 180.0) -> dict[str, Any]:
        """Classify the model-call outcome; never infer success from a timeout."""

        deadline = time.monotonic() + settle_timeout
        projection: dict[str, Any] = {}
        facts: dict[str, Any] = {
            "calls": [],
            "agentic_assessments": None,
            "lead_assessments": None,
            "provider_sessions": None,
            "execution_check_receipts": None,
            "db_errors": {},
            "critical_error": "",
        }
        original_timeout = self.api.timeout
        self.api.timeout = 10.0
        try:
            while True:
                try:
                    projection = self.api.get(f"/intake/requests/{self.intake}") or {}
                except Exception:  # noqa: BLE001 - Core may be shutting down
                    projection = {}
                facts = self._failure_db_rows()
                if facts["critical_error"]:
                    # Without the critical durable state a cleanup decision is
                    # impossible; do not wait or guess.
                    break
                states = {str(row[1]) for row in facts["calls"]}
                settled = (
                    "SETTLED" in states
                    or (facts["agentic_assessments"] or 0) > 0
                    or (facts["lead_assessments"] or 0) > 0
                )
                unresolved = bool(states & {"RESERVED", "AMBIGUOUS", "UNKNOWN"})
                intake_state = str(projection.get("state") or "")
                terminal = (
                    settled
                    or (states and not unresolved)
                    or intake_state
                    in {
                        "FAILED",
                        "BLOCKED",
                        "CANCELLED",
                        "WITHDRAWN",
                        "AWAITING_APPROVAL",
                        "EXECUTING",
                        "COMPLETED",
                    }
                )
                if terminal or time.monotonic() >= deadline:
                    break
                time.sleep(5)
        finally:
            self.api.timeout = original_timeout
        states = [str(row[1]) for row in facts["calls"]]
        intake_state = str(projection.get("state") or "")
        if facts["critical_error"]:
            outcome = "MODEL_CALL_OUTCOME_UNKNOWN"
        elif any(state in {"RESERVED", "AMBIGUOUS", "UNKNOWN"} for state in states):
            outcome = "MODEL_CALL_OUTCOME_UNKNOWN"
        elif any(state == "SETTLED" for state in states) or (facts["agentic_assessments"] or 0) > 0 or (facts["lead_assessments"] or 0) > 0:
            outcome = "MODEL_CALL_SETTLED"
        elif states and all(state == "RELEASED" for state in states):
            outcome = "NO_MODEL_CALL_SETTLED"
        elif intake_state in {"SUBMITTED", "ASSESSING"}:
            outcome = "MODEL_CALL_OUTCOME_UNKNOWN"
        else:
            outcome = "NO_MODEL_CALL_CAPTURED"
        return {
            "outcome": outcome,
            "db_captured": not bool(facts["critical_error"]),
            "critical_error": facts["critical_error"],
            "db_errors": facts["db_errors"],
            "intake_state": intake_state,
            "assessment_present": (facts["agentic_assessments"] or 0) > 0 or (facts["lead_assessments"] or 0) > 0,
            "agentic_assessments": facts["agentic_assessments"],
            "lead_assessments": facts["lead_assessments"],
            "provider_sessions": facts["provider_sessions"],
            "execution_check_receipts": facts["execution_check_receipts"],
            "model_calls": [
                {
                    "role": row[0],
                    "state": row[1],
                    "purpose": row[2],
                    "provider": row[3],
                    "model": row[4],
                }
                for row in facts["calls"]
            ],
            "settle_timeout_secs": settle_timeout,
            "reconciled_at": utc(),
        }

    def capture_failure_db(self) -> dict[str, Any]:
        """Sanitized read-only snapshot of the disposable Core database.

        No prompts, submission bodies, ciphertext, command bodies or raw errors
        are copied; this is evidence of state, not a database backup for reuse.
        """

        queries = {
            "intake_requests": (
                "SELECT id,project_id,action_id,lead_request_id,campaign_id,"
                "profile_revision_id,profile_hash,created_at,updated_at "
                "FROM intake_requests WHERE id=?1",
                (self.intake,),
            ),
            "lead_requests": (
                "SELECT id,state,created_at,updated_at FROM lead_requests WHERE id=?1",
                (self.lead_request_id or self.intake,),
            ),
            "model_call_reservations": (
                "SELECT reservation_id,role,purpose,provider,model,state,input_tokens,"
                "output_tokens,total_tokens,cost_micros,usage_known,created_at,updated_at "
                "FROM model_call_reservations WHERE project_id=?1 ORDER BY created_at",
                (self.args.project_id,),
            ),
            "provider_sessions": (
                "SELECT session_record_id,attempt_id,generation,state,provider,model,"
                "cleanup_receipt,updated_at FROM provider_sessions ORDER BY updated_at DESC LIMIT 50",
                (),
            ),
            "execution_check_receipts": (
                "SELECT receipt_id,attempt_id,generation,work_item_id,suite,outcome,created_at "
                "FROM execution_check_receipts ORDER BY created_at DESC LIMIT 100",
                (),
            ),
            "subscription_scope_admissions": (
                "SELECT admission_id,intake_request_id,state,campaign_id,expires_at_ms "
                "FROM subscription_scope_admissions WHERE intake_request_id=?1",
                (self.intake,),
            ),
        }
        snapshot: dict[str, Any] = {}
        for name, (sql, params) in queries.items():
            try:
                columns, rows = self._db_rows_with_columns(sql, params)
                snapshot[name] = {"columns": columns, "rows": [list(row) for row in rows]}
            except Exception as error:  # noqa: BLE001 - record, never fabricate
                snapshot[name] = {"state": "NOT_CAPTURED", "error": type(error).__name__}
        return snapshot

    def capture_failure(self, error: BaseException, stage: str = "") -> dict[str, Any]:
        """Preserve causal state before any cleanup; never claim a PASS."""

        # Only wait for a Core reconciliation when a model call could exist.
        settle = 0.0 if (stage == "scenario_invalid" or not self.intake or not self.admission_id) else 180.0
        failed_request = list(getattr(self.api, "last_request", ("", "")))
        failed_error = getattr(self.api, "last_error", None)
        reconciliation = self.reconcile_failure(settle_timeout=settle)
        try:
            self.write(
                "failure-intake.json",
                self.api.get(f"/intake/requests/{self.intake}") or {},
            )
        except Exception:  # noqa: BLE001 - capture is best-effort
            pass
        try:
            self.write("failure-db.json", self.capture_failure_db())
        except Exception:  # noqa: BLE001 - capture is best-effort
            pass
        payload = {
            "status": "NOT_TESTED",
            "stage": stage,
            "error": f"{type(error).__name__}: {error}",
            "at": utc(),
            "started_at": self.started_at,
            "intake": self.intake,
            "campaign": self.campaign,
            "attempt": self.attempt,
            "admission_id": self.admission_id,
            "last_request": failed_request,
            "last_error": failed_error,
            "transport_timeout_secs": self.api.timeout,
            "reconciliation": reconciliation,
        }
        self.write("failure.json", payload)
        self.write("error.json", {"error": payload["error"], "at": payload["at"], "stage": stage})
        self.log(f"failure outcome={reconciliation['outcome']} stage={stage}")
        return payload

    def _gate_detail(self, candidate_id: str) -> dict[str, Any]:
        if not candidate_id:
            return {}
        try:
            gate = self.api.get(f"/quality/candidates/{candidate_id}/gate")
            return gate if isinstance(gate, dict) else {}
        except Exception:  # noqa: BLE001 - missing gate is NOT_TESTED, not a crash
            return {}

    def _pre_intervention_outcome(self) -> str:
        path = self.evidence / "pre-intervention.json"
        if not path.exists():
            return ""
        return str(json.loads(path.read_text(encoding="utf-8")).get("version_outcome") or "")

    def capture(self, verdict: Verdict, observation: Observation) -> None:
        self.write("result.json", {
            "status": verdict.status,
            "reason": verdict.reason,
            "evidence": verdict.evidence,
            "captured_utc": utc(),
            "observation": observation.__dict__,
        })
        self.write("campaign.json", self.campaign_detail())
        self.write("candidates.json", self.api.get("/candidates") or [])
        self.write("provider-sessions.json", self.api.get("/provider/sessions") or [])
        self.write("events.json", self.events())

    def cleanup(self) -> None:
        result = _podman_compose(self.project, self.root, self.git_dir, ["down", "-v", "--remove-orphans"])
        self.write("cleanup.json", {"exit": result.returncode, "at": utc()})


def _safe_to_cleanup(scenario: "Scenario") -> bool:
    """Cleanup only after a captured, terminal reconciliation.

    A client timeout with a RESERVED/AMBIGUOUS/UNKNOWN model call is not
    terminal. The stack and database stay for the owner to reconcile; deleting
    them would destroy the only evidence of a possible provider call.
    """

    path = scenario.evidence / "failure.json"
    if not path.exists():
        return False
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return False
    outcome = str((payload.get("reconciliation") or {}).get("outcome") or "")
    if outcome == "MODEL_CALL_OUTCOME_UNKNOWN":
        scenario.write(
            "cleanup-held.json",
            {
                "status": "STACK_RETAINED_FOR_RECONCILIATION",
                "reason": "model-call outcome unknown; stack and database preserved for owner reconciliation",
                "outcome": outcome,
                "owner_action": (
                    "Reconcile the durable intake/lead/model-call rows to a terminal state and capture "
                    "the evidence before removing the disposable project."
                ),
                "condition": "model_call_reservations has no RESERVED/AMBIGUOUS/UNKNOWN row",
                "cleanup_command": (
                    f"podman compose -p {scenario.project} -f compose.yaml "
                    "-f compose.real-provider-uat.yaml down -v --remove-orphans"
                ),
                "at": utc(),
            },
        )
        return False
    return True


def run_scenario(scenario: "Scenario") -> tuple[Verdict, Observation]:
    """The ordered consumer path: prepare, contract, gen1, kill, gen2, restart."""

    scenario.prepare()
    scenario.admit()
    scenario.assess()
    scenario.prepare_plan()
    scenario.approve()
    scenario.verify_contract()
    snapshot = scenario.watch_gen1()
    snapshot = scenario.intervene(snapshot)
    observation = scenario.watch_gen2(snapshot)
    observation = replace(observation, restart_observed=scenario.verify_restart(observation))
    return classify_recovery(observation), observation


def _require(value: Any, name: str, errors: list[str]) -> None:
    if not value:
        errors.append(name)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Real generation-2 recovery scenario")
    parser.add_argument("--plan", action="store_true", help="print the frozen scenario plan and exit")
    parser.add_argument("--dry-run", action="store_true", help="validate the scenario without a stack")
    parser.add_argument("--execute", action="store_true", help="run the live scenario (requires a prepared stack)")
    parser.add_argument("--root", default=".")
    parser.add_argument("--core-api")
    parser.add_argument("--operator-token-file")
    parser.add_argument("--project-id")
    parser.add_argument("--profile-id", help="execution profile created by the pre-run binding")
    parser.add_argument("--authorization")
    parser.add_argument("--evidence-dir")
    parser.add_argument("--compose-project")
    parser.add_argument("--git-dir")
    parser.add_argument("--core-database")
    parser.add_argument("--authorized-micros", type=int, default=DEFAULT_AUTHORIZED_MICROS)
    parser.add_argument(
        "--assumption-file",
        help="0600 non-secret JSON metadata from the Owner's current subscription answer",
    )
    parser.add_argument("--provider", default="opencode-go")
    parser.add_argument("--model", default="deepseek-v4.1-flash")
    args = parser.parse_args(argv)

    if args.plan:
        print(json.dumps(plan_document(), indent=2))
        return 0
    errors: list[str] = []
    _require(args.core_api, "--core-api", errors)
    _require(args.operator_token_file, "--operator-token-file", errors)
    _require(args.project_id, "--project-id", errors)
    _require(args.profile_id, "--profile-id", errors)
    _require(args.authorization, "--authorization", errors)
    _require(args.evidence_dir, "--evidence-dir", errors)
    _require(args.compose_project, "--compose-project", errors)
    _require(args.git_dir, "--git-dir", errors)
    _require(args.core_database, "--core-database", errors)
    if args.authorized_micros < 0:
        errors.append("--authorized-micros must not be negative")
    project = str(args.compose_project or "")
    if project and (
        project == "software-factory"
        or not any(token in project for token in ("recovery", "uat"))
    ):
        errors.append("--compose-project must be a disposable *recovery*/*uat* project")
    if errors:
        print("invalid scenario inputs: " + ", ".join(errors), file=sys.stderr)
        return 2
    if args.dry_run:
        print(json.dumps({"status": "DRY_RUN_OK", "plan": plan_document()}, indent=2))
        return 0
    if not args.execute:
        print("use --plan/--dry-run/--execute", file=sys.stderr)
        return 2

    scenario = Scenario(args)
    try:
        verdict, observation = run_scenario(scenario)
        scenario.capture(verdict, observation)
        scenario.cleanup()
        print(json.dumps({"status": verdict.status, "reason": verdict.reason}, indent=2))
        if verdict.status == "ACCEPTED":
            return 0
        if verdict.status == "OK_SCOPED":
            return 3
        return 2
    except ScenarioInvalid as error:
        try:
            scenario.capture_failure(error, stage="scenario_invalid")
        except Exception:  # noqa: BLE001
            pass
        if not _safe_to_cleanup(scenario):
            return 2
        try:
            scenario.cleanup()
        except Exception:  # noqa: BLE001
            pass
        return 2
    except Exception as error:  # noqa: BLE001 - capture then fail closed
        captured = False
        try:
            scenario.capture_failure(error)
            captured = True
        except Exception as capture_error:  # noqa: BLE001 - still fail closed
            scenario.write(
                "error.json",
                {
                    "error": f"{type(error).__name__}: {error}",
                    "capture_error": f"{type(capture_error).__name__}: {capture_error}",
                    "at": utc(),
                },
            )
        if not captured or not _safe_to_cleanup(scenario):
            return 1
        try:
            scenario.cleanup()
        except Exception:  # noqa: BLE001
            pass
        print(f"scenario failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
