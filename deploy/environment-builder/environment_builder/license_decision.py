"""Exact builder-side licence decision receipt validation.

The plan carries a Core-written ``licenseDecisions`` receipt list. The builder
does not approve a licence by name, family or package metadata; it accepts only
an ACTIVE exact decision whose identity, evidence, stage and obligations match
the shared evidence and the real plan/inventory. A mismatch is STALE and fails
closed.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

from . import license_policy as policy

ASSET_FILE = Path(__file__).with_name("versioned_source_evidence.json")


class LicenseDecisionError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n"


def _sha_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _sha_value(value: Any) -> str:
    return _sha_text(_canonical(value))


def _asset() -> dict[str, Any]:
    try:
        return json.loads(ASSET_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_EVIDENCE_MISSING",
            f"the shared versioned source evidence is unreadable: {error}",
        ) from error


def _identity(asset: dict[str, Any], receipt: dict[str, Any]) -> dict[str, Any]:
    identity = receipt.get("identity")
    if not isinstance(identity, dict):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID", "the decision has no identity"
        )
    for entry in asset.get("identities", []):
        if (
            entry.get("pin") == identity.get("pin")
            and entry.get("name") == identity.get("name")
            and entry.get("version") == identity.get("version")
            and entry.get("output", {}).get("storePath")
            == identity.get("outputStorePath")
            and entry.get("output", {}).get("narHash")
            == identity.get("outputNarHash")
        ):
            return entry
    raise LicenseDecisionError(
        "ENVIRONMENT_LICENSE_DECISION_INVALID",
        "the decision identity is not the exact shared identity",
    )


def _stage(receipt: dict[str, Any]) -> dict[str, Any]:
    stage = receipt.get("stage")
    if isinstance(stage, str):
        try:
            stage = json.loads(stage)
        except ValueError as error:
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID", "the decision stage is malformed"
            ) from error
    if not isinstance(stage, dict):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID", "the decision has no stage"
        )
    return stage


def _plan_stage(plan: dict[str, Any]) -> dict[str, Any]:
    inventory = plan.get("licenseInventory")
    if not isinstance(inventory, dict):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_INVENTORY_MISSING",
            "the plan has no real licence inventory for the exact stage",
        )
    return {
        "planSha256": plan.get("planSha256"),
        "planRevision": plan.get("revision"),
        "inventorySha256": plan.get("licenseInventorySha256")
        or inventory.get("inventorySha256"),
        "evidenceBundleSha256": inventory.get("sbomSha256"),
        "baseCommit": plan.get("baseCommit"),
    }


def _exact_list(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID", f"{label} is malformed"
        )
    return value


def validate_license_decision_receipt(
    receipt: dict[str, Any], plan: dict[str, Any]
) -> dict[str, Any]:
    """Validate one plan receipt; return the exact identity for the closure."""
    if not isinstance(receipt, dict):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID", "the decision receipt is not an object"
        )
    if receipt.get("status") != "ACTIVE" or receipt.get("revokedAt") is not None:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_REVOKED", "the licence decision is not active"
        )
    if receipt.get("decision") != "ACCEPT":
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_DENIED", "the licence decision did not accept"
        )
    asset = _asset()
    identity = _identity(asset, receipt)
    stage = _stage(receipt)
    plan_stage = _plan_stage(plan)
    for key in ("planSha256", "planRevision", "inventorySha256", "evidenceBundleSha256"):
        if stage.get(key) != plan_stage.get(key):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_STALE",
                f"the decision stage {key} differs from the current plan",
            )
    candidate = stage.get("candidate")
    if not isinstance(candidate, dict):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE", "the decision candidate binding is missing"
        )
    base_commit = candidate.get("baseCommit")
    if candidate.get("kind") == "candidate" or (
        candidate.get("candidateSha256") is not None and base_commit is None
    ):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            "an executable candidate decision requires the current candidate binding",
        )
    if candidate.get("kind") != "base" or base_commit != plan_stage.get("baseCommit"):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE", "the decision base commit differs from the plan"
        )
    evidence = asset.get("evidence", {})
    expected_evidence = evidence.get("licenseEvidenceCanonicalSha256")
    if (
        not isinstance(expected_evidence, str)
        or len(expected_evidence) != 64
        or receipt.get("evidenceSha256") != expected_evidence
    ):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the decision evidence digest does not match the shared evidence",
        )
    if _sha_value(identity.get("licenseEvidence")) != expected_evidence:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            "the shared licence evidence is not canonical",
        )
    expected_outputs = evidence.get("outputFilesCanonicalSha256")
    if (
        not isinstance(expected_outputs, str)
        or len(expected_outputs) != 64
        or _sha_value(identity.get("outputFiles")) != expected_outputs
    ):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            "the shared output evidence is not canonical",
        )
    if receipt.get("licenseExpression") != identity.get("license"):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the licence expression is not the exact identity licence",
        )
    if receipt.get("selectedBranch") != receipt.get("licenseExpression"):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the selected branch is not the exact identity branch",
        )
    software_obligations = _exact_list(
        receipt.get("softwareObligations"), "software obligations"
    )
    catalog, profile = policy.load_default()
    entry = catalog.entries.get(receipt["licenseExpression"])
    if entry is None or software_obligations != list(entry.obligations):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the software obligations are not exact for the selected branch",
        )
    documentation = receipt.get("documentation")
    if not isinstance(documentation, list):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID", "the documentation evidence is missing"
        )
    output_by_path = {
        item.get("path"): item
        for item in identity.get("outputFiles", [])
        if isinstance(item, dict)
    }
    for item in documentation:
        if not isinstance(item, dict):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID", "a documentation entry is malformed"
            )
        path = item.get("path")
        output = output_by_path.get(path)
        if output is None or output.get("purpose") != "documentation":
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "documentation does not match an exact output file",
            )
        if item.get("license") != output.get("license"):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation licence does not match the exact output",
            )
        if item.get("verdict") not in ("ALLOWED", "UNKNOWN_CLASSIFIED"):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation verdict is missing or not accepted",
            )
        text_entry = next(
            (
                evidence_entry
                for evidence_entry in identity.get("licenseEvidence", [])
                if evidence_entry.get("path") == "doc/fdl.texi"
            ),
            None,
        )
        if text_entry is not None and item.get("licenseTextSha256") != text_entry.get("sha256"):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation text digest does not match the exact source",
            )
        obligations = _exact_list(item.get("obligations"), "documentation obligations")
        doc_entry = catalog.entries.get(item["license"])
        if doc_entry is None or obligations != list(doc_entry.obligations):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation obligations are not exact",
            )
    if documentation:
        documentation_obligations = _exact_list(
            receipt.get("documentationObligations"), "documentation obligations"
        )
        if documentation_obligations != list(documentation[0]["obligations"]):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation obligations are not exact for the identity",
            )
    if receipt.get("catalogSha256") != catalog.sha256 or receipt.get(
        "profileSha256"
    ) != profile.sha256:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            "the decision policy binding differs from the current policy",
        )
    if receipt.get("policySha256") != policy.binding_sha256(catalog, profile):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE", "the decision policy digest is stale"
        )
    return identity
