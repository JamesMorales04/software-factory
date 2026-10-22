#!/usr/bin/env python3
"""Validate repository correlation for a real Factory campaign manifest.

Schema and repository records are not independent proof that external provider
or runtime events occurred. See docs/quality/live-certification.md.
"""

from __future__ import annotations

import argparse
from datetime import datetime
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

try:
    from jsonschema import Draft202012Validator
except ImportError:  # give a useful message instead of a traceback
    Draft202012Validator = None  # type: ignore[assignment,misc]


SHA40 = re.compile(r"^[0-9a-f]{40}$")
SHA64 = re.compile(r"^[0-9a-f]{64}$")
LIVE_LEVEL = "REAL_PASEO_E2E_VERIFIED"
NON_LIVE_MODES = {"simulation", "simulated", "stub", "stubbed", "synthetic", "mock", "mocked"}


def _load(path: Path, errors: list[str], label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        errors.append(f"cannot load {label}: {exc}")
        return {}
    if not isinstance(value, dict):
        errors.append(f"{label} must be a JSON object")
        return {}
    return value


def _schema_errors(manifest: dict[str, Any], schema: dict[str, Any]) -> list[str]:
    if Draft202012Validator is None:
        return ["missing dependency jsonschema; install scripts/requirements-certification.txt"]
    try:
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
    except Exception as exc:  # invalid schema is a repository configuration failure
        return [f"certification schema is invalid: {exc}"]
    errors = []
    for error in sorted(validator.iter_errors(manifest), key=lambda item: list(map(str, item.absolute_path))):
        location = ".".join(str(part) for part in error.absolute_path) or "manifest"
        errors.append(f"schema {location}: {error.message}")
    return errors


def _same(manifest: dict[str, Any], key: str, expected: Any, errors: list[str]) -> None:
    if manifest.get(key) != expected:
        errors.append(f"{key} does not match source evidence")


def _identity(value: Any) -> bool:
    return isinstance(value, str) and bool(value) and not any(char.isspace() for char in value)


def _git_commit_exists(repo_root: Path, sha: str) -> bool:
    try:
        subprocess.run(
            ["git", "cat-file", "-e", f"{sha}^{{commit}}"], cwd=repo_root,
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        return True
    except (OSError, subprocess.CalledProcessError):
        return False


def validate_candidate_git(candidate_repo: Path, candidate_sha: str, expected_tree_sha: str) -> list[str]:
    """Check an external Project candidate commit and tree when available."""
    try:
        top = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], cwd=candidate_repo,
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        commit = subprocess.run(
            ["git", "rev-parse", f"{candidate_sha}^{{commit}}"], cwd=top,
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        tree = subprocess.run(
            ["git", "rev-parse", f"{candidate_sha}^{{tree}}"], cwd=top,
            check=True, capture_output=True, text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        return [f"candidate commit is unavailable in candidate repository: {exc}"]
    if commit != candidate_sha:
        return ["candidate repository resolved a different commit SHA"]
    if tree != expected_tree_sha:
        return ["candidate commit tree does not match source evidence treeSha"]
    return []


def _check_capability_evidence(
    manifest: dict[str, Any], source: dict[str, Any], run: dict[str, Any],
    evidence: dict[str, Any], spec: dict[str, Any], errors: list[str],
) -> None:
    """Enforce capability requirements from the correlated frozen spec."""
    preflight = source.get("preflight", {}).get("frozen-spec-contracts", {}).get("detail", {})
    preflight_caps = set(preflight.get("capabilities", [])) if isinstance(preflight, dict) else set()
    frozen_caps = set(spec.get("capabilities", [])) or preflight_caps
    run_caps = set(run.get("capabilities", []))
    if not frozen_caps:
        errors.append("frozen ActionSpec capability profile is missing")
    if frozen_caps != run_caps:
        errors.append("Run capabilities do not match the frozen ActionSpec")
    if preflight_caps != frozen_caps:
        errors.append("preflight capability profile does not match the frozen ActionSpec")
    if isinstance(preflight, dict) and (
        preflight.get("specId") != manifest.get("spec_id")
        or preflight.get("bindingHash") != manifest.get("spec_binding_hash")
    ):
        errors.append("preflight capability profile is not bound to the manifest frozen ActionSpec")

    browser_required = "browser" in frozen_caps
    browser_manifest = manifest.get("browser")
    result = run.get("result", {})
    observed = source.get("browser_certification", {})
    gate_checks = {check.get("id"): check.get("status") for check in source.get("decision", {}).get("checks", [])}
    verifier = next((v for v in evidence.get("verifications", []) if v.get("id") == manifest.get("verifier", {}).get("id")), {})
    verifier_checks = {check.get("id"): check.get("status") for check in verifier.get("checks", [])}
    if browser_required:
        successful = observed.get("successful_commands", [])
        if not browser_manifest:
            errors.append("frozen ActionSpec requires browser evidence but manifest omits it")
        if not {"open", "snapshot"}.issubset(set(successful)):
            errors.append("frozen browser capability lacks successful open and snapshot observations")
        if browser_manifest and browser_manifest.get("successful_commands") != successful:
            errors.append("browser observations disagree with source evidence")
        if not observed.get("testServerManaged") or not result.get("testServerManaged"):
            errors.append("frozen browser capability lacks managed test-server evidence")
        if result.get("browserInvoked") is not True or result.get("browserSuccessfulCommands", "").split(",") != successful:
            errors.append("Run result does not corroborate model-mediated browser commands")
        if browser_manifest and browser_manifest.get("managed_test_server") is not True:
            errors.append("manifest does not record the required managed test server")
        if gate_checks.get("gate-capability") != "PASS":
            errors.append("Quality Gate lacks the required browser capability acceptance check")
        if not any(c.get("kind") == "browser" for c in spec.get("acceptanceCriteria", [])):
            errors.append("frozen browser ActionSpec has no browser acceptance criterion")
    else:
        if browser_manifest:
            errors.append("manifest claims browser certification but frozen ActionSpec does not require browser")
    accepted = {"test", "functional", "integration", "api", "service", "build", "cli", "browser"}
    required_ids = {
        c.get("id") for c in spec.get("acceptanceCriteria", [])
        if c.get("kind") in accepted and isinstance(c.get("id"), str)
    }
    if not required_ids:
        errors.append("frozen ActionSpec has no supported executable acceptance criteria")
    for criterion_id in sorted(required_ids):
        if gate_checks.get(criterion_id) != "PASS":
            errors.append(f"Quality Gate lacks passing frozen ActionSpec criterion {criterion_id}")
        if verifier_checks.get(criterion_id) != "PASS":
            errors.append(f"independent verifier lacks passing frozen ActionSpec criterion {criterion_id}")


def validate(manifest_path: Path, repo_root: Path, candidate_repo: Path | None = None) -> list[str]:
    errors: list[str] = []
    schema = _load(repo_root / "docs/quality/certification-manifests.schema.json", errors, "certification schema")
    manifest = _load(manifest_path, errors, "manifest")
    if errors:
        return errors
    errors.extend(_schema_errors(manifest, schema))
    if errors:
        return errors
    if manifest.get("schema_version") not in (1, 2):
        errors.append("unsupported schema_version")
    if manifest.get("level") != LIVE_LEVEL or manifest.get("status") != "verified":
        errors.append(f"this validator accepts only verified {LIVE_LEVEL} manifests")
    if manifest.get("invalidated_by"):
        errors.append("verified status contradicts invalidated_by")
    if manifest.get("external_observation_assurance") != "repository_recorded_not_independently_corroborated":
        errors.append("repository manifest cannot claim independent external observation proof")
    if errors:
        return errors

    implementation_sha = manifest["implementation_sha"]
    if not SHA40.fullmatch(implementation_sha) or not _git_commit_exists(repo_root, implementation_sha):
        errors.append("implementation_sha is stale or unavailable in this repository")

    source_ref = manifest["source_evidence"]
    source_path = (repo_root / source_ref).resolve()
    try:
        source_path.relative_to(repo_root.resolve())
    except ValueError:
        return errors + ["source_evidence must resolve inside the repository"]
    source = _load(source_path, errors, "source evidence")
    if errors:
        return errors

    if str(source.get("status", "")).upper().startswith(("SIMULATED", "STUB", "SYNTHETIC")) or str(source.get("paseo_mode", "")).lower() in NON_LIVE_MODES:
        errors.append("simulated evidence cannot support REAL_PASEO_E2E_VERIFIED")
    if source.get("campaign_status") != "accepted" or source.get("decision", {}).get("status") != "ACCEPTED":
        errors.append("source evidence does not record an accepted Quality Gate")
    if source.get("decision", {}).get("status") in {"REJECTED", "REPAIR_REQUIRED"}:
        errors.append("Quality Gate rejection cannot be represented as acceptance")

    for key in ("implementation_sha", "project_id", "campaign_id", "action_id", "spec_id", "spec_binding_hash"):
        _same(manifest, key, source.get(key), errors)
    if not SHA64.fullmatch(manifest["spec_binding_hash"]):
        errors.append("spec_binding_hash must be a full SHA-256 digest")

    candidate = source.get("candidate", {})
    for key, source_key in (("candidate_id", "id"), ("candidate_sha", "commitSha")):
        _same(manifest, key, candidate.get(source_key), errors)
    for key, source_key in (("project_id", "projectId"), ("action_id", "actionId"),
                            ("spec_id", "specId"), ("spec_binding_hash", "specBindingHash")):
        _same(manifest, key, candidate.get(source_key), errors)
    if candidate.get("state") != "frozen" or candidate.get("supersededAt") != "0001-01-01T00:00:00Z":
        errors.append("candidate is not frozen and current")
    if candidate.get("implementerRunId") != manifest["run_id"] or candidate.get("implementerAttemptId") != manifest["attempt_id"]:
        errors.append("candidate lineage does not match the manifest Run/Attempt")
    if candidate_repo is not None:
        errors.extend(validate_candidate_git(candidate_repo, manifest["candidate_sha"], candidate.get("treeSha", "")))

    evidence = source.get("candidate_evidence", {}).get(manifest["candidate_id"], {})
    spec = evidence.get("spec", {})
    if (spec.get("id") != manifest["spec_id"] or spec.get("actionId") != manifest["action_id"]
            or spec.get("projectId") != manifest["project_id"]
            or spec.get("contentHash") != evidence.get("candidate", {}).get("specHash")):
        errors.append("frozen ActionSpec identity or content hash is missing or mismatched")
    if spec.get("bindingHash") != manifest["spec_binding_hash"]:
        errors.append("frozen ActionSpec binding hash does not match manifest")

    runs = source.get("runs", [])
    matched = next((entry for entry in runs if entry.get("run", {}).get("id") == manifest["run_id"]), None)
    if matched is None:
        errors.append("run_id does not identify a source Run")
        matched = {}
    run = matched.get("run", {})
    if run.get("status") != "succeeded":
        errors.append("Run is not terminal-success")
    if run.get("projectId") != manifest["project_id"] or run.get("actionId") != manifest["action_id"]:
        errors.append("Run project or Action identity does not match manifest")
    if run.get("specId") != manifest["spec_id"] or run.get("specBindingHash") != manifest["spec_binding_hash"]:
        errors.append("Run frozen spec identity does not match manifest")
    attempt = next((a for a in matched.get("attempts", []) if a.get("id") == manifest["attempt_id"]), None)
    if attempt is None or attempt.get("runId") != manifest["run_id"] or attempt.get("status") != "succeeded":
        errors.append("attempt_id is missing, mismatched, or not successful")
    result = run.get("result", {})

    session_id = manifest.get("session_id")
    if run.get("workloadKind") != "paseo-task":
        errors.append("REAL_PASEO_E2E_VERIFIED requires a Factory-managed paseo-task Run")
    for key, source_key in (("provider", "provider"), ("model", "model"), ("mode", "paseoMode"),
                            ("session_id", "sessionId"), ("workspace_id", "workspaceId")):
        if not _identity(manifest.get(key)) or not _identity(result.get(source_key)):
            errors.append(f"real Paseo execution requires valid {key} identities in manifest and Run result")
        _same(manifest, key, result.get(source_key), errors)
    for key, source_key in (("provider", "provider"), ("model", "model"), ("mode", "paseoMode")):
        if not _identity(run.get(source_key)) or run.get(source_key) != manifest.get(key):
            errors.append(f"authoritative Run {source_key} does not match the manifest")
    if str(result.get("paseoMode", "")).lower() in NON_LIVE_MODES:
        errors.append("stubbed or simulated Run mode cannot support REAL_PASEO_E2E_VERIFIED")
    bindings = source.get("session_bindings", [])
    binding = next((b for b in bindings if b.get("sessionId") == session_id), None)
    if not binding:
        errors.append("session_id has no source session binding")
    else:
        for field, expected in (("runId", manifest["run_id"]), ("attemptId", manifest["attempt_id"]),
                                ("workspaceId", manifest.get("workspace_id")), ("provider", manifest.get("provider")),
                                ("model", manifest.get("model")), ("mode", manifest.get("mode"))):
            if binding.get(field) != expected:
                errors.append(f"session binding {field} does not match manifest")
        if binding.get("status") != "succeeded" or binding.get("runStatus") != "succeeded":
            errors.append("session binding is not terminal-success")
    recorded_budget = source.get("authorization", {}).get("max_paid_implementer_sessions")
    spent = source.get("paid_sessions_spent")
    if type(recorded_budget) is int and type(spent) is int and spent > recorded_budget:
        errors.append("recorded session use exceeds source campaign authorization")
    if source.get("provider") and source.get("provider") != manifest.get("provider"):
        errors.append("manifest provider differs from the source campaign route")
    if source.get("model") and source.get("model") != manifest.get("model"):
        errors.append("manifest model differs from the source campaign route")
    if manifest["schema_version"] == 2:
        authorization = manifest.get("authorization")
        if authorization is None:
            errors.append("version 2 model execution requires explicit campaign authorization")
        else:
            route = authorization.get("route", {})
            if authorization.get("campaign_id") != manifest.get("campaign_id"):
                errors.append("authorization campaign identity does not match manifest")
            if route.get("provider") != manifest.get("provider") or route.get("model") != manifest.get("model"):
                errors.append("authorized provider/model route does not match manifest")
            if route.get("provider") != source.get("provider") or route.get("model") != source.get("model"):
                errors.append("authorized route does not match source campaign route")
            if type(recorded_budget) is not int or recorded_budget <= 0:
                errors.append("source campaign authorization has no positive session budget")
            elif authorization.get("max_sessions", 0) > recorded_budget:
                errors.append("manifest authorized budget exceeds source campaign authorization")
            if type(spent) is not int or spent < 0:
                errors.append("source campaign does not record session spend")
            elif spent > authorization.get("max_sessions", 0):
                errors.append("recorded session use exceeds manifest authorized budget")

    _check_capability_evidence(manifest, source, run, evidence, spec, errors)

    verification = next((v for v in evidence.get("verifications", []) if v.get("id") == manifest["verifier"].get("id")), None)
    if manifest["verifier"].get("independent") is not True or manifest["verifier"].get("status") != "passed":
        errors.append("manifest falsely claims independent verifier success")
    if not verification or verification.get("status") != "passed" or not verification.get("independent"):
        errors.append("independent verifier evidence is missing, failed, or mismatched")
    elif verification.get("candidateSha") != manifest["candidate_sha"] or verification.get("specId") != manifest["spec_id"]:
        errors.append("verifier is not bound to the manifest candidate/spec")
    review = next((r for r in evidence.get("reviews", []) if r.get("id") == manifest["reviewer"].get("id")), None)
    if manifest["reviewer"].get("verdict") != "APPROVE" or not review or review.get("verdict") != "APPROVE":
        errors.append("independent reviewer approval is missing, failed, or mismatched")
    elif review.get("candidateSha") != manifest["candidate_sha"] or review.get("specId") != manifest["spec_id"]:
        errors.append("reviewer is not bound to the manifest candidate/spec")
    elif review.get("authority") != "reviewer" or (verification and review.get("actor") == verification.get("actor")):
        errors.append("reviewer evidence is not independent from the verifier")

    gate = evidence.get("decision", {})
    if gate.get("id") != manifest["quality_gate"].get("id") or gate.get("status") != "ACCEPTED" or manifest["quality_gate"].get("status") != "ACCEPTED":
        errors.append("Quality Gate evidence is missing, rejected, or mismatched")
    if gate.get("candidateSha") != manifest["candidate_sha"] or gate.get("specId") != manifest["spec_id"]:
        errors.append("Quality Gate is not bound to the manifest candidate/spec")
    frozen_at = candidate.get("frozenAt")
    for label, record in (("verifier", verification), ("reviewer", review), ("Quality Gate", gate)):
        timestamp = record.get("createdAt") if record else None
        try:
            frozen_time = datetime.fromisoformat(frozen_at.replace("Z", "+00:00"))
            evidence_time = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
        except (AttributeError, ValueError):
            errors.append(f"{label} evidence has no valid freshness timestamp")
        else:
            if evidence_time < frozen_time:
                errors.append(f"{label} evidence predates the frozen candidate")
    gate_checks = {check.get("id"): check.get("status") for check in gate.get("checks", [])}
    if gate_checks.get("gate-independence") != "PASS" or gate_checks.get("gate-review") != "PASS":
        errors.append("Quality Gate lacks independent-verification or review acceptance checks")

    cleanup = source.get("cleanup", {})
    cleanup_ok = (
        cleanup.get("workspace_active") == []
        and cleanup.get("workspace_error") in (None, "")
        and all(s.get("archived") is True and s.get("status") == "closed" for s in cleanup.get("session_cleanup", []))
        and (not session_id or any(s.get("sessionId") == session_id for s in cleanup.get("session_cleanup", [])))
    )
    browser_required = "browser" in set(spec.get("capabilities", [])) or "browser" in set(
        source.get("preflight", {}).get("frozen-spec-contracts", {}).get("detail", {}).get("capabilities", [])
    )
    if browser_required:
        cleanup_ok = cleanup_ok and cleanup.get("managed_server_clean") is True
        cleanup_ok = cleanup_ok and cleanup.get("managed_server_port_open") is False
        cleanup_ok = cleanup_ok and not cleanup.get("managed_server", {}).get("errors")
    if not cleanup_ok or manifest["cleanup"].get("status") != "confirmed" or manifest["cleanup"].get("contradictions"):
        errors.append("cleanup is missing, incomplete, or contradictory")
    if source.get("certification", {}).get("gate_accepted") is not True:
        errors.append("source certification does not record gate acceptance")

    authorization = manifest.get("authorization")
    if authorization and manifest["schema_version"] == 1:
        recorded_authorization = source.get("authorization", {})
        route = authorization.get("route", {})
        if authorization.get("campaign_id") != manifest.get("campaign_id"):
            errors.append("authorization campaign identity does not match manifest")
        if route.get("provider") != manifest.get("provider") or route.get("model") != manifest.get("model"):
            errors.append("authorized provider/model route does not match manifest")
        if authorization.get("max_sessions", 0) > recorded_authorization.get("max_paid_implementer_sessions", -1):
            errors.append("manifest authorized budget exceeds source campaign authorization")
        if source.get("paid_sessions_spent", 0) > authorization.get("max_sessions", -1):
            errors.append("recorded session use exceeds manifest authorized budget")

    recovery = manifest.get("recovery")
    if recovery and recovery.get("claimed"):
        failed_run_id = recovery.get("failed_run_id")
        failed_attempt_id = recovery.get("failed_attempt_id")
        corrective_run_id = recovery.get("corrective_run_id")
        wrappers = {item.get("run", {}).get("id"): item for item in source.get("runs", [])}
        failed_wrapper = wrappers.get(failed_run_id, {})
        failed_attempt = next((a for a in failed_wrapper.get("attempts", []) if a.get("id") == failed_attempt_id), None)
        if failed_run_id == corrective_run_id or failed_run_id not in source.get("failed_implementer_runs", []):
            errors.append("recovery claim does not identify a distinct failed Implementer Run")
        if not failed_attempt or failed_attempt.get("runId") != failed_run_id or failed_attempt.get("status") != "failed":
            errors.append("recovery claim lacks the identified failed Attempt")
        failed_run = failed_wrapper.get("run", {})
        if failed_run.get("status") != "failed":
            errors.append("recovery claim's earlier Run is not recorded as failed")
        if any(failed_run.get(field) != expected for field, expected in (
            ("projectId", manifest["project_id"]), ("actionId", manifest["action_id"]),
            ("specId", manifest["spec_id"]), ("specBindingHash", manifest["spec_binding_hash"]),
        )):
            errors.append("failed Run is outside the certified project/Action/frozen spec")
        recovery_row = next((item for item in source.get("recoveries", [])
                             if item.get("previousRunId") == failed_run_id), None)
        if not recovery_row or recovery_row.get("previousAttemptId") != failed_attempt_id:
            errors.append("source recovery record does not bind the failed Run and Attempt")
        if not recovery_row or recovery_row.get("newRunId") != corrective_run_id:
            errors.append("source recovery record does not identify the corrective Run")
        if recovery_row and recovery_row.get("campaignId") != manifest["campaign_id"]:
            errors.append("source recovery record belongs to another Campaign")
        if recovery_row and recovery_row.get("failureClass") not in {
            "RECOVERABLE_IMPLEMENTATION_FAILURE", "RECOVERABLE_EXECUTION_FAILURE",
        }:
            errors.append("source recovery record has a non-recoverable failure class")
        corrective_wrapper = wrappers.get(corrective_run_id, {})
        corrective_run = corrective_wrapper.get("run", {})
        if corrective_run_id != manifest.get("run_id") or corrective_run.get("status") != "succeeded":
            errors.append("recovery claim does not identify the accepted corrective Run")
        if not corrective_wrapper or not any(a.get("runId") == corrective_run_id and a.get("status") == "succeeded"
                                             for a in corrective_wrapper.get("attempts", [])):
            errors.append("corrective Run has no successful Attempt record")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", nargs="?", default="docs/evidence/f1-s4c-07-certification-manifest.json")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--candidate-repo", type=Path, help="optional external Project checkout for exact candidate Git verification")
    args = parser.parse_args()
    root = args.root.resolve()
    path = Path(args.manifest)
    if not path.is_absolute():
        path = root / path
    errors = validate(path, root, args.candidate_repo)
    if errors:
        print("INVALID certification manifest:")
        for error in errors:
            print(f"- {error}")
        return 1
    candidate_note = "candidate commit/tree independently checked in supplied Project checkout" if args.candidate_repo else "external candidate Git commit/tree NOT_CORROBORATED"
    print(
        "SCHEMA_VALID; REPOSITORY_IDENTITIES_CORRELATED; " + candidate_note
        + "; EXTERNAL_PROVIDER_SESSION_NOT_INDEPENDENTLY_CORROBORATED"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
