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
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable

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
    "exactly {'version': '1.0'}. Both frozen checks must pass. Commit only the "
    "required source change."
)
ALLOWED_PATHS = ["src"]
DEFAULT_AUTHORIZED_MICROS = 5_000_000  # per-request materialization, not a global cap


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


@dataclass(frozen=True)
class Verdict:
    status: str  # OK | FAIL | NOT_TESTED | INCONCLUSIVE
    reason: str
    evidence: dict[str, Any]


class ScenarioError(RuntimeError):
    pass


class ScenarioInvalid(ScenarioError):
    """The scenario preconditions were not met; the run is NOT_TESTED."""


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


LEGITIMATE_LATE = {
    "provider.session_terminal",
    "provider.session_cleaned",
    "worker.terminal_reported",
    "worker.session_rotated",
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


def classify_late_effects(events: Iterable[dict[str, Any]], kill_at: str, claim_at: str) -> tuple[str, str]:
    """Classify events between the kill and the generation-2 claim.

    Dangerous generation-1 effects (tool RESULT, provider turns, receipts,
    candidates, progress) invalidate the scenario. Session terminal/cleanup and
    worker terminal reporting are legitimate. Anything unrecognised is
    ambiguous and must not become a PASS.
    """

    kill = _parse_iso(kill_at)
    claim = _parse_iso(claim_at)
    if kill is None:
        return "AMBIGUOUS", "kill timestamp is missing"
    for event in events:
        recorded = _parse_iso(str(event.get("recorded_at") or ""))
        if recorded is None or recorded < kill:
            continue
        if claim is not None and recorded > claim:
            continue
        kind = str(event.get("kind") or "")
        if kind in LEGITIMATE_LATE or kind.startswith("cleanup") or kind.startswith("audit"):
            continue
        if kind in DANGEROUS_LATE:
            return "DANGEROUS", f"{kind} at {event.get('recorded_at')}"
        if kind == "provider.tool_event":
            if str(event.get("detail") or "").upper().find("RESULT") >= 0:
                return "DANGEROUS", f"tool RESULT at {event.get('recorded_at')}"
            continue
        return "AMBIGUOUS", f"unclassified event {kind} at {event.get('recorded_at')}"
    return "NONE", ""


def _parse_iso(value: str):
    from datetime import datetime

    if not value:
        return None
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return parsed if parsed.tzinfo else None


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

    precondition, reason = classify_p2_precondition(observation)
    if precondition == "ALREADY_DONE":
        return Verdict("NOT_TESTED", f"P2: {reason}", evidence)
    if precondition != "PENDING":
        return Verdict("NOT_TESTED", f"P2: {reason}", evidence)

    version_receipt = _find_passing(observation.generation_two_receipts, VERSION, 2)
    if version_receipt is None:
        return Verdict("FAIL", "P3: generation 2 has no passing version receipt for the pending work", evidence)
    evidence["p3_version_receipt"] = version_receipt.__dict__

    fresh = [c for c in observation.candidates_after_claim if str(c.get("attempt_id") or "") == observation.attempt_id]
    if not fresh:
        return Verdict("NOT_TESTED", "no candidate exists for the attempt after the generation-2 claim", evidence)
    candidate = fresh[0]
    evidence["candidate"] = candidate
    for field_name in ("provider", "model", "provider_mode"):
        if not getattr(observation, f"gen2_{field_name}", ""):
            return Verdict("NOT_TESTED", f"gen2 session route {field_name} not observed", evidence)
    if not observation.candidate_sha or not observation.candidate_tree or not observation.candidate_base:
        return Verdict("NOT_TESTED", "candidate sha/tree/base not observed", evidence)
    missing = []
    if observation.gate_outcome != "ACCEPTED":
        missing.append(f"quality gate={observation.gate_outcome or 'missing'}")
    if not observation.campaign_status:
        missing.append("campaign status missing")
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
            "create project/config/profile/policy; post the Lead intake with a positive per-request authorized_micros",
            "verify the approved ActionSpec checks against the exact contract before any intervention",
            "wait for generation-1 status PASS; prove no generation-1 version PASS receipt exists (P2 pending)",
            "confirm generation-1 provider turn/check quiescence from Core facts",
            "kill the Worker, await EXPIRED, restart it, observe worker.claimed generation=2",
            "wait for the generation-2 version PASS receipt and a candidate after the claim",
            "run independent verifier/reviewer/gate per contract; capture evidence; clean up only copies",
        ],
        "p4": "the frozen ProgressSupervisor replay test remains the negative control",
    }


class Api:
    def __init__(self, base: str, token: str) -> None:
        self.base = base.rstrip("/")
        self.token = token

    def _request(self, method: str, path: str, payload: Any | None = None) -> Any:
        data = None if payload is None else json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            self.base + path,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "Origin": self.base,
            },
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            body = response.read().decode("utf-8")
            return json.loads(body) if body else None

    def get(self, path: str) -> Any:
        return self._request("GET", path)

    def post(self, path: str, payload: dict[str, Any]) -> Any:
        return self._request("POST", path, payload)

    def put(self, path: str, payload: dict[str, Any]) -> Any:
        return self._request("PUT", path, payload)

    def post_status(self, path: str, payload: dict[str, Any]) -> tuple[int, Any]:
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            self.base + path,
            data=data,
            method="POST",
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "Origin": self.base,
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body = response.read().decode("utf-8")
                return response.status, (json.loads(body) if body else None)
        except urllib.error.HTTPError as error:
            body = error.read().decode("utf-8")
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
        self.restart_required = False
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
        profile = self.api.post(
            "/execution-profiles",
            {
                "name": f"Real recovery {self.args.authorization}",
                "description": "P1-P4 real generation-2 recovery scenario",
                "category": "BALANCED",
                "roles": {
                    "lead": {"provider": self.args.provider, "model": self.args.model, "reasoning": "medium"},
                    "implementer": {"provider": self.args.provider, "model": self.args.model, "reasoning": "medium"},
                    "semanticReviewer": {"provider": self.args.provider, "model": self.args.model, "reasoning": "medium"},
                },
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
                "auto_start": {"enabled": True, "risk_classes": ["R0"]},
            },
        )
        self.api.post(f"/execution-profiles/{profile['id']}/default", {"scope": "global"})
        created = self.api.post(
            "/intake/requests",
            {
                "project_id": self.args.project_id,
                "objective": OBJECTIVE,
                "execution_profile_id": profile["id"],
                "mode": "AUTONOMOUS",
                "idempotency_key": f"{self.args.authorization}:recovery-intake",
                "authorized_micros": self.args.authorized_micros,
            },
        )
        self.intake = created["id"]
        self.action = created["actionId"]
        self.write("intake.json", created)
        self.log(f"intake={self.intake} action={self.action}")

    def await_campaign(self, timeout: int = 1800) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            projection = self.api.get(f"/intake/requests/{self.intake}")
            campaign_id = projection.get("campaignId") or (projection.get("campaign") or {}).get("id")
            if campaign_id and str(campaign_id) != "None":
                self.campaign = str(campaign_id)
                self.write("intake-projection.json", projection)
                self.log(f"campaign={self.campaign}")
                return
            state = str(projection.get("state") or "")
            if state in {"FAILED", "REJECTED", "WITHDRAWN"}:
                raise ScenarioError(f"intake did not reach a campaign: {state} ({projection.get('stateDetail')})")
            time.sleep(5)
        raise ScenarioError("timed out waiting for the Campaign")

    def verify_contract(self) -> None:
        projection = self.api.get(f"/intake/requests/{self.intake}")
        action = projection.get("action") or {}
        spec = action.get("spec") or action
        checks = action.get("checks") or spec.get("checks") or []
        self.action_checks = checks
        self.restart_required = bool(spec.get("restart_required"))
        self.write("approved-action.json", action)
        mismatch = contract_mismatch(checks)
        if mismatch:
            raise ScenarioError(f"approved ActionSpec mismatch: {mismatch}")

        record = self.api.get(f"/campaigns/{self.campaign}/authorization")
        self.authorization_record = record if isinstance(record, dict) else {}
        self.write("authorization-record.json", self.authorization_record)
        if not self.authorization_record.get("authorization_id"):
            raise ScenarioError("Core has no durable authorization record for the Campaign")
        if self.args.authorized_micros <= 0:
            raise ScenarioError("authorized_micros must be positive")
        if not self.authorization_record.get("monetary_envelope_id"):
            raise ScenarioError("Core authorization has no monetary envelope reference")

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
        self.write("negative-direct-authorize.json", {"status": status, "body": body})
        if status != 403:
            raise ScenarioError(f"direct TRUSTED_LOCAL_PROVIDER authorization was not refused: {status}")

    def campaign_detail(self) -> dict[str, Any]:
        return self.api.get(f"/campaigns/{self.campaign}")

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
        self.events_baseline = max((int(e.get("sequence") or 0) for e in self.events()), default=0)
        while time.monotonic() < deadline:
            events = self.events()
            attempt = self.attempt_id()
            if attempt:
                self.attempt = attempt
            version_outcome = self._version_outcome(1)
            if self._status_passed(1) and version_outcome != "PASS":
                snapshot = self._pre_intervention_snapshot(events)
                self.write("pre-intervention.json", snapshot)
                failures = barrier_failures(snapshot)
                if failures:
                    self.write("result.json", {
                        "status": "NOT_TESTED",
                        "reason": "pre-intervention barrier: " + "; ".join(failures),
                        "snapshot": snapshot,
                        "captured_utc": utc(),
                    })
                    raise ScenarioInvalid("pre-intervention barrier failed: " + "; ".join(failures))
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
                tool_events.extend(self.api.get(f"/provider/sessions/{record_id}/tool_events") or [])
        turn_authorized = [e for e in events if e.get("kind") == "provider.turn_authorized" and str(e.get("subject_id")) == self.attempt]
        turn_settled = [e for e in events if e.get("kind") == "provider.turn_settled" and str(e.get("subject_id")) == self.attempt]
        active_turns = max(0, len(turn_authorized) - len(turn_settled))
        last_turn_event_at = str((turn_settled or turn_authorized or [{}])[-1].get("recorded_at") or "")
        participants = self.api.get(f"/office/participants?campaign_id={self.campaign}&limit=200") or {}
        rows = participants.get("participants") if isinstance(participants, dict) else participants
        implementer = next(
            (row for row in (rows or []) if str(row.get("attempt_id")) == self.attempt and str(row.get("role") or "").upper() == "IMPLEMENTER"),
            {},
        )
        version_events = [
            e for e in tool_events
            if str(((e.get("execution_check") or {}).get("check_id")) or "") == VERSION
        ]
        version_receipts = _receipts_for(self._receipts(1), VERSION, 1)
        version_pass = any(r.outcome == "PASS" for r in version_receipts)
        candidates = [c for c in (self.api.get("/candidates") or []) if str(c.get("attempt_id")) == self.attempt]
        gen1_quiescent = bool(sessions) and all(
            str(s.get("state") or "").upper() in {"ACTIVE", "COMPLETED"} for s in sessions
        ) and active_turns == 0
        lease_state = str(implementer.get("lease_state") or "")
        return {
            "observed_at": utc(),
            "status_pass": True,
            "version_pass_absent": not version_pass,
            "attempt_status": attempt_status,
            "gen1_sessions": sessions,
            "gen1_tool_events": tool_events,
            "last_turn_event_at": last_turn_event_at,
            "active_turns": active_turns,
            "lease_state": lease_state,
            "gen1_quiescent": gen1_quiescent,
            "quiescence_backend_confirmed": gen1_quiescent and lease_state in {"LIVE", "LEASED", "EXPIRED"},
            "p2_check_absent": not version_pass and not version_events and not version_receipts,
            "p2_artifacts_absent": not version_events and not version_receipts,
            "candidate_count": len(candidates),
            "version_outcome": self._version_outcome(1),
        }

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
            "SELECT attempt_id,generation,check_id,outcome,candidate_sha256,receipt_id,received_at,"
            "command_sha256,session_record_id,sequence,call_id,work_item_id "
            "FROM execution_check_receipts WHERE attempt_id=?1 AND generation=?2 ORDER BY rowid",
            (self.attempt, generation),
        )
        return [CheckReceipt(*row) for row in rows]

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

    def _quiescence(self) -> dict[str, Any]:
        sessions = self.api.get("/provider/sessions") or []
        session = next(
            (s for s in sessions if str(s.get("attempt_id")) == self.attempt and int(s.get("generation") or 0) == 1),
            None,
        )
        return {
            "session_state": (session or {}).get("state"),
            "provider_session_id": (session or {}).get("provider_session_id"),
            "events_baseline": self.events_baseline,
        }

    def intervene(self, snapshot: dict[str, Any]) -> None:
        # Revalidate the pending precondition immediately before the kill: a late
        # generation-1 version receipt or check event invalidates the scenario.
        late = self._version_outcome(1)
        version_events = [
            e for e in (self.api.get(f"/provider/sessions/{s.get('session_record_id')}/tool_events") or [])
            for s in snapshot.get("gen1_sessions") or []
            if str(((e.get("execution_check") or {}).get("check_id")) or "") == VERSION
        ]
        if late == "PASS" or version_events:
            self.write("result.json", {
                "status": "NOT_TESTED",
                "reason": "generation-1 P2 changed immediately before the kill",
                "snapshot": snapshot,
                "captured_utc": utc(),
            })
            raise ScenarioInvalid("P2 changed before the kill; scenario invalid")
        result = _podman_compose(self.project, self.root, self.git_dir, ["kill", "worker"])
        self.killed = True
        snapshot["kill_at"] = utc()
        self.write("pre-intervention.json", snapshot)
        self.log(f"kill_worker exit={result.returncode} at={snapshot['kill_at']}")
        if result.returncode != 0:
            raise ScenarioError(f"kill worker failed: {result.stderr.strip()[:160]}")
        # Await the lease expiry from Core projection; never sleep blindly.
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            sessions = self.api.get("/provider/sessions") or []
            session = next(
                (s for s in sessions if str(s.get("attempt_id")) == self.attempt and int(s.get("generation") or 0) == 1),
                None,
            )
            if session and str(session.get("lease_state") or "") == "EXPIRED":
                break
            time.sleep(3)
        result = _podman_compose(self.project, self.root, self.git_dir, ["start", "worker"])
        self.log(f"start_worker exit={result.returncode} at={utc()}")
        if result.returncode != 0:
            raise ScenarioError(f"start worker failed: {result.stderr.strip()[:160]}")

    def watch_gen2(self, snapshot: dict[str, Any], timeout: int = 3600) -> Observation:
        deadline = time.monotonic() + timeout
        claimed = None
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
        kill_at = str(snapshot.get("kill_at") or "")
        if claimed is not None and (not kill_at or claim_at < kill_at):
            self.write("result.json", {
                "status": "NOT_TESTED",
                "reason": "the generation-2 claim did not follow the recorded kill",
                "snapshot": snapshot,
                "claimed_at": claim_at,
                "captured_utc": utc(),
            })
            raise ScenarioInvalid("kill/claim order invalid")
        observation = Observation(
            attempt_id=self.attempt,
            campaign_id=self.campaign,
            claimed_generation_two=claimed is not None,
            claimed_at=str((claimed or {}).get("recorded_at") or ""),
            generation_two_session_state=str(gen2.get("state") or ""),
            generation_two_session_error=str(gen2.get("error") or ""),
            generation_one_receipts=self._receipts(1),
            generation_two_receipts=self._receipts(2),
            pre_intervention_version_outcome=self._pre_intervention_outcome(),
            candidates_after_claim=[
                c for c in (self.api.get("/candidates") or [])
                if str(c.get("attempt_id")) == self.attempt
                and str(c.get("created_at") or "") > str((claimed or {}).get("recorded_at") or "")
            ],
            allocation_conflict=any("allocation conflict" in str(e.get("detail") or "") for e in self.events()),
            gen2_provider=str(gen2.get("provider") or ""),
            gen2_model=str(gen2.get("model") or ""),
            gen2_provider_mode=str(gen2.get("provider_mode") or ""),
            gate_outcome=self._gate_outcome(),
            candidate_sha=str((candidates[0] if candidates else {}).get("commit_sha") or ""),
            candidate_tree=str((candidates[0] if candidates else {}).get("tree_sha") or ""),
            candidate_base=str((candidates[0] if candidates else {}).get("base_commit") or ""),
            campaign_status=str((self.campaign_detail().get("campaign") or {}).get("status") or ""),
            restart_required=self.restart_required,
            restart_observed=False,
            late_effect_risk=classify_late_effects(self.events(), kill_at, claim_at)[0],
            kill_at=kill_at,
        )
        return observation

    def _gate_outcome(self) -> str:
        candidates = [c for c in (self.api.get("/candidates") or []) if str(c.get("attempt_id")) == self.attempt]
        if not candidates:
            return ""
        try:
            gate = self.api.get(f"/quality/candidates/{candidates[0]['id']}/gate")
        except Exception:  # noqa: BLE001 - missing gate is NOT_TESTED, not a crash
            return ""
        return str((gate or {}).get("outcome") or "")

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
    parser.add_argument("--authorization")
    parser.add_argument("--evidence-dir")
    parser.add_argument("--compose-project")
    parser.add_argument("--git-dir")
    parser.add_argument("--core-database")
    parser.add_argument("--authorized-micros", type=int, default=DEFAULT_AUTHORIZED_MICROS)
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
    _require(args.authorization, "--authorization", errors)
    _require(args.evidence_dir, "--evidence-dir", errors)
    _require(args.compose_project, "--compose-project", errors)
    _require(args.git_dir, "--git-dir", errors)
    if args.authorized_micros <= 0:
        errors.append("--authorized-micros must be positive (no zero/unbounded bypass)")
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
        scenario.prepare()
        scenario.await_campaign()
        scenario.verify_contract()
        snapshot = scenario.watch_gen1()
        scenario.intervene(snapshot)
        observation = scenario.watch_gen2(snapshot)
        verdict = classify_recovery(observation)
        scenario.capture(verdict, observation)
        scenario.cleanup()
        print(json.dumps({"status": verdict.status, "reason": verdict.reason}, indent=2))
        if verdict.status == "ACCEPTED":
            return 0
        if verdict.status == "OK_SCOPED":
            return 3
        return 2
    except ScenarioInvalid:
        try:
            scenario.cleanup()
        except Exception:  # noqa: BLE001
            pass
        return 2
    except Exception as error:  # noqa: BLE001 - capture then fail closed
        scenario.write("error.json", {"error": f"{type(error).__name__}: {error}", "at": utc()})
        try:
            scenario.cleanup()
        except Exception:  # noqa: BLE001
            pass
        print(f"scenario failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
