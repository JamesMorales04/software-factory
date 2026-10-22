#!/usr/bin/env python3
"""Validate structural consistency of a Factory certification manifest.

This validates repository records only. It cannot prove that external events
actually occurred; runtime observations need independent corroboration.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any


SHA40 = re.compile(r"^[0-9a-f]{40}$")
SHA64 = re.compile(r"^[0-9a-f]{64}$")
LIVE_LEVEL = "REAL_PASEO_E2E_VERIFIED"


def _get(obj: dict[str, Any], path: str, errors: list[str]) -> Any:
    cur: Any = obj
    for part in path.split("."):
        if not isinstance(cur, dict) or part not in cur:
            errors.append(f"source evidence is missing {path}")
            return None
        cur = cur[part]
    return cur


def _equal(manifest: dict[str, Any], key: str, expected: Any, errors: list[str]) -> None:
    if manifest.get(key) != expected:
        errors.append(f"{key} does not match source evidence")


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
    errors: list[str] = []
    for key in schema.get("required", []):
        if key not in manifest:
            errors.append(f"manifest is missing required field {key}")
    allowed = set(schema.get("properties", {}))
    for key in manifest:
        if key not in allowed:
            errors.append(f"manifest has unknown field {key}")
    for key, rule in schema.get("properties", {}).items():
        if key not in manifest:
            continue
        value = manifest[key]
        if "const" in rule and value != rule["const"]:
            errors.append(f"{key} must be {rule['const']}")
        if "enum" in rule and value not in rule["enum"]:
            errors.append(f"{key} has unsupported value {value!r}")
        expected_type = rule.get("type")
        valid_types = {"object": dict, "string": str, "array": list, "boolean": bool, "integer": int}
        if isinstance(expected_type, list):
            type_ok = any(_is_type(value, valid_types.get(item)) for item in expected_type)
        else:
            type_ok = _is_type(value, valid_types.get(expected_type))
        if expected_type and not type_ok:
            errors.append(f"{key} has the wrong type")
        if isinstance(value, str) and "pattern" in rule and not re.fullmatch(rule["pattern"], value):
            errors.append(f"{key} has an invalid format")
    if manifest.get("schema_version") != 1:
        errors.append("unsupported schema_version")
    return errors


def _is_type(value: Any, typ: type[Any] | None) -> bool:
    if typ is None:
        return True
    if typ is int:
        return isinstance(value, int) and not isinstance(value, bool)
    return isinstance(value, typ)


def validate(manifest_path: Path, repo_root: Path) -> list[str]:
    errors: list[str] = []
    schema_path = repo_root / "docs/quality/certification-manifests.schema.json"
    schema = _load(schema_path, errors, "certification schema")
    manifest = _load(manifest_path, errors, "manifest")
    if errors:
        return errors
    errors.extend(_schema_errors(manifest, schema))

    if manifest.get("status") == "verified" and manifest.get("invalidated_by"):
        errors.append("verified status contradicts invalidated_by")
    if manifest.get("level") != LIVE_LEVEL or manifest.get("status") != "verified":
        return errors

    if manifest.get("external_observation_assurance") != "repository_recorded_not_independently_corroborated":
        errors.append("live manifest must state that external observations are not independently corroborated")
    implementation_sha = manifest.get("implementation_sha", "")
    if not isinstance(implementation_sha, str) or not SHA40.fullmatch(implementation_sha):
        errors.append("implementation_sha must be a full 40-character commit SHA")
    else:
        try:
            subprocess.run(
                ["git", "cat-file", "-e", f"{implementation_sha}^{{commit}}"],
                cwd=repo_root,
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        except (OSError, subprocess.CalledProcessError):
            errors.append("implementation_sha is stale or unavailable in this repository")

    source_ref = manifest.get("source_evidence", "")
    source_path = (repo_root / source_ref).resolve()
    try:
        source_path.relative_to(repo_root.resolve())
    except ValueError:
        errors.append("source_evidence must resolve inside the repository")
        return errors
    source = _load(source_path, errors, "source evidence")
    if errors:
        return errors

    if source.get("status", "").startswith("SIMULATED") or source.get("paseo_mode") in {"simulation", "simulated"}:
        errors.append("simulated evidence cannot support REAL_PASEO_E2E_VERIFIED")
    if source.get("campaign_status") != "accepted" or source.get("decision", {}).get("status") != "ACCEPTED":
        errors.append("source evidence does not record an accepted Quality Gate")
    if source.get("decision", {}).get("status") in {"REJECTED", "REPAIR_REQUIRED"}:
        errors.append("Quality Gate rejection cannot be represented as acceptance")

    for key, source_key in (
        ("implementation_sha", "implementation_sha"),
        ("project_id", "project_id"),
        ("campaign_id", "campaign_id"),
        ("action_id", "action_id"),
        ("spec_id", "spec_id"),
        ("spec_binding_hash", "spec_binding_hash"),
    ):
        _equal(manifest, key, source.get(source_key), errors)

    candidate = source.get("candidate", {})
    for key, source_key in (("candidate_id", "id"), ("candidate_sha", "commitSha")):
        _equal(manifest, key, candidate.get(source_key), errors)
    _equal(manifest, "project_id", candidate.get("projectId"), errors)
    _equal(manifest, "action_id", candidate.get("actionId"), errors)
    _equal(manifest, "spec_id", candidate.get("specId"), errors)
    _equal(manifest, "spec_binding_hash", candidate.get("specBindingHash"), errors)
    if candidate.get("state") != "frozen":
        errors.append("candidate is not frozen")
    if candidate.get("supersededAt") != "0001-01-01T00:00:00Z":
        errors.append("candidate is superseded")

    runs = source.get("runs", [])
    matched_run = next((entry for entry in runs if entry.get("run", {}).get("id") == manifest.get("run_id")), None)
    if matched_run is None:
        errors.append("run_id does not identify a source Run")
        matched_run = {}
    run = matched_run.get("run", {})
    if run.get("projectId") != manifest.get("project_id") or run.get("actionId") != manifest.get("action_id"):
        errors.append("Run project or Action identity does not match the manifest")
    attempt = next((a for a in matched_run.get("attempts", []) if a.get("id") == manifest.get("attempt_id")), None)
    if attempt is None or attempt.get("runId") != manifest.get("run_id"):
        errors.append("attempt_id does not belong to the named Run")

    binding = next((b for b in source.get("session_bindings", []) if b.get("sessionId") == manifest.get("session_id")), None)
    if binding is None:
        errors.append("session_id has no source session binding")
    else:
        for field, expected in (("runId", manifest.get("run_id")), ("attemptId", manifest.get("attempt_id")),
                                ("workspaceId", manifest.get("workspace_id")), ("provider", manifest.get("provider")),
                                ("model", manifest.get("model")), ("mode", manifest.get("mode"))):
            if binding.get(field) != expected:
                errors.append(f"session binding {field} does not match the manifest")
        if binding.get("status") != "succeeded" or binding.get("runStatus") != "succeeded":
            errors.append("session binding is not terminal-success")
    result = run.get("result", {})
    if result.get("sessionId") != manifest.get("session_id") or result.get("workspaceId") != manifest.get("workspace_id"):
        errors.append("Run result session/workspace identity does not match the manifest")
    if result.get("provider") != manifest.get("provider") or result.get("model") != manifest.get("model") or result.get("paseoMode") != manifest.get("mode"):
        errors.append("Run result provider/model/mode does not match the manifest")

    browser = manifest.get("browser", {})
    observed_commands = source.get("browser_certification", {}).get("successful_commands", [])
    if browser.get("successful_commands") != observed_commands or not {"open", "snapshot"}.issubset(set(observed_commands)):
        errors.append("browser observations are missing or disagree with source evidence")
    if not browser.get("managed_test_server") or not result.get("testServerManaged"):
        errors.append("required Factory-managed browser test server observation is missing")
    if result.get("browserInvoked") is not True or result.get("browserSuccessfulCommands", "").split(",") != observed_commands:
        errors.append("Run result does not corroborate successful model-mediated browser commands")

    evidence = source.get("candidate_evidence", {}).get(manifest.get("candidate_id"), {})
    verification = next((v for v in evidence.get("verifications", []) if v.get("id") == manifest.get("verifier", {}).get("id")), None)
    if not verification or verification.get("status") != "passed" or not verification.get("independent"):
        errors.append("independent verifier evidence is missing, failed, or mismatched")
    elif verification.get("candidateSha") != manifest.get("candidate_sha") or verification.get("specId") != manifest.get("spec_id"):
        errors.append("verifier is not bound to the manifest candidate/spec")
    review = next((r for r in evidence.get("reviews", []) if r.get("id") == manifest.get("reviewer", {}).get("id")), None)
    if not review or review.get("verdict") != "APPROVE":
        errors.append("independent reviewer approval is missing, failed, or mismatched")
    elif review.get("candidateSha") != manifest.get("candidate_sha") or review.get("specId") != manifest.get("spec_id"):
        errors.append("reviewer is not bound to the manifest candidate/spec")
    elif review.get("authority") != "reviewer" or (verification and review.get("actor") == verification.get("actor")):
        errors.append("reviewer evidence is not independent from the verifier")
    gate = evidence.get("decision", {})
    if gate.get("id") != manifest.get("quality_gate", {}).get("id") or gate.get("status") != "ACCEPTED" or manifest.get("quality_gate", {}).get("status") != "ACCEPTED":
        errors.append("Quality Gate evidence is missing, rejected, or mismatched")
    if gate.get("candidateSha") != manifest.get("candidate_sha") or gate.get("specId") != manifest.get("spec_id"):
        errors.append("Quality Gate is not bound to the manifest candidate/spec")
    gate_checks = {check.get("id"): check.get("status") for check in gate.get("checks", [])}
    if gate_checks.get("gate-independence") != "PASS" or gate_checks.get("gate-review") != "PASS" or gate_checks.get("gate-capability") != "PASS":
        errors.append("Quality Gate lacks independent-review or browser-capability acceptance checks")

    cleanup = source.get("cleanup", {})
    cleanup_ok = (
        cleanup.get("managed_server_clean") is True
        and cleanup.get("managed_server_port_open") is False
        and cleanup.get("workspace_active") == []
        and cleanup.get("workspace_error") in (None, "")
        and not cleanup.get("managed_server", {}).get("errors")
        and all(s.get("archived") is True and s.get("status") == "closed" for s in cleanup.get("session_cleanup", []))
        and any(s.get("sessionId") == manifest.get("session_id") for s in cleanup.get("session_cleanup", []))
    )
    if not cleanup_ok or manifest.get("cleanup", {}).get("status") != "confirmed":
        errors.append("cleanup is missing, incomplete, or contradictory")
    if manifest.get("cleanup", {}).get("contradictions"):
        errors.append("cleanup manifest lists contradictory observations")
    if source.get("certification", {}).get("gate_accepted") is not True:
        errors.append("source certification does not record gate acceptance")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", nargs="?", default="docs/evidence/f1-s4c-07-certification-manifest.json")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    root = args.root.resolve()
    path = Path(args.manifest)
    if not path.is_absolute():
        path = root / path
    errors = validate(path, root)
    if errors:
        print("INVALID certification manifest:")
        for error in errors:
            print(f"- {error}")
        return 1
    print("STRUCTURALLY_VALID repository certification evidence; external runtime observations remain uncorroborated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
