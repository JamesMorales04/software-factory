"""Exact builder-side licence decision receipt validation.

The plan carries a Core-written ``licenseDecisions`` receipt list. The builder
does not approve a licence by name, family or package metadata; it accepts only
an ACTIVE exact decision whose identity, evidence, stage and obligations match
the shared evidence and the real plan/inventory. A mismatch is STALE and fails
closed.
"""

from __future__ import annotations

import hashlib
import base64
import json
from pathlib import Path
from typing import Any

from . import license_policy as policy

ASSET_FILE = Path(__file__).with_name("versioned_source_evidence.json")
SOURCE_VECTOR_FILE = Path(__file__).with_name("source_evidence_proof_vector.json")


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
    output = identity.get("output") if isinstance(identity.get("output"), dict) else {}
    store_path = identity.get("outputStorePath") or output.get("storePath")
    nar_hash = identity.get("outputNarHash") or output.get("narHash")
    for entry in asset.get("identities", []):
        if (
            entry.get("pin") == identity.get("pin")
            and entry.get("name") == identity.get("name")
            and entry.get("version") == identity.get("version")
            and entry.get("output", {}).get("storePath") == store_path
            and entry.get("output", {}).get("narHash") == nar_hash
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
        "evidenceBundleSha256": _sha_value(
            {"entries": inventory.get("entries", [])}
        ),
        "baseCommit": plan.get("baseCommit"),
    }


def _exact_list(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID", f"{label} is malformed"
        )
    return value


_V3_CONTRACT = "license-review-v3-file-package-rights"


def _is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def _file_text_supports(asset_license: Any, right: Any) -> bool:
    """Exact typed text/grant relation; never a family or suffix shortcut.

    Only the known `X-only` text may accompany the exact `X-or-later` grant.
    """
    if not isinstance(asset_license, str) or not isinstance(right, str):
        return False
    if asset_license == right:
        return True
    if right.endswith("-or-later"):
        return asset_license == f"{right[:-len('-or-later')]}-only"
    return False


def _validate_v3_rights(
    receipt: dict[str, Any], identity: dict[str, Any]
) -> None:
    """Validate the Core file/package/document rights projection exactly.

    The model's text label is diagnostic; only the asset labels and exact
    licensing/output facts participate. The builder does not read the decision
    database and does not recompute ``decisionContentSha256``; it binds that
    digest into the overlay content digest and verifies the exact facts.
    """
    if receipt.get("contractRevision") != _V3_CONTRACT:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            "the decision does not carry the current file/package/document contract",
        )
    for key, expected in (
        ("fileLabelSource", "ASSET"),
        ("packageRightsSource", "LICENSING_FACT"),
        ("documentationGrantSource", "OUTPUT_FILE_ASSET"),
    ):
        if receipt.get(key) != expected:
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                f"the decision {key} is not the exact Core source",
            )
    if (
        receipt.get("modelTextLicenseIgnored") is not True
        or receipt.get("modelObligationsIgnored") is not True
    ):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the decision does not isolate model text from authority",
        )
    if not _is_sha256(receipt.get("decisionContentSha256")):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the decision content digest is missing or malformed",
        )

    entries = identity.get("licenseEvidence")
    if not isinstance(entries, list):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the exact licence evidence is missing",
        )
    text_by_path: dict[str, dict[str, Any]] = {}
    asset_by_path: dict[str, dict[str, Any]] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a licence evidence entry is malformed",
            )
        path = entry.get("path")
        license_id = entry.get("license")
        sha = entry.get("sha256")
        if not all(isinstance(value, str) and value for value in (path, license_id, sha)):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a licence evidence entry lacks exact path/licence/digest",
            )
        asset_by_path[path] = entry
        if (
            entry.get("purpose") == "license-text"
            and entry.get("role") == "license-text"
        ):
            text_by_path[path] = entry

    file_evidence = receipt.get("fileEvidence")
    if not isinstance(file_evidence, list):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the file evidence projection is missing",
        )
    seen: set[tuple[Any, Any]] = set()
    for item in file_evidence:
        if not isinstance(item, dict):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID", "a file evidence entry is malformed"
            )
        path = item.get("path")
        sha = item.get("sha256")
        asset_license = item.get("assetLicense")
        role = item.get("role")
        entry = text_by_path.get(path) if isinstance(path, str) else None
        if (
            role != "license-text"
            or entry is None
            or entry.get("sha256") != sha
            or entry.get("license") != asset_license
        ):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the file evidence is not an exact authenticated licence text",
            )
        key = (path, sha)
        if key in seen:
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the file evidence repeats a licence text",
            )
        seen.add(key)

    try:
        atoms = policy.branch_atoms(receipt.get("selectedBranch"))
    except ValueError as error:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            f"the selected licence branch is not exact: {error}",
        ) from error
    rights = receipt.get("packageRights")
    if not isinstance(rights, list) or not rights:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the package rights projection is missing",
        )
    if [
        item.get("atom") if isinstance(item, dict) else None for item in rights
    ] != atoms:
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the package rights do not match the selected branch atoms",
        )
    for item, atom in zip(rights, atoms, strict=True):
        if not isinstance(item, dict) or item.get("license") != atom:
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a package right is not the exact selected atom",
            )
        path = item.get("path")
        sha = item.get("sha256")
        grant = next(
            (
                entry
                for entry in entries
                if entry.get("role") == "licensing"
                and entry.get("license") == atom
                and entry.get("path") == path
                and entry.get("sha256") == sha
            ),
            None,
        )
        if grant is None or item.get("source") != identity.get("source"):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a package right is not the exact authenticated licensing fact and source",
            )

    documentation = receipt.get("documentation")
    grants = receipt.get("documentationGrants")
    if not isinstance(documentation, list) or not isinstance(grants, list):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the documentation grant projection is missing",
        )
    output_by_path = {
        entry.get("path"): entry
        for entry in identity.get("outputFiles", [])
        if isinstance(entry, dict)
    }
    expected: dict[str, dict[str, Any]] = {}
    for document in documentation:
        if not isinstance(document, dict):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID", "a documentation entry is malformed"
            )
        path = document.get("path")
        output = output_by_path.get(path)
        if (
            output is None
            or output.get("purpose") != "documentation"
            or output.get("license") != document.get("license")
            or output.get("sha256") != document.get("outputSha256")
        ):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a documentation grant is not an exact output file",
            )
        text_entry = asset_by_path.get(document.get("licenseTextPath"))
        if (
            text_entry is None
            or text_entry.get("sha256") != document.get("licenseTextSha256")
            or not _file_text_supports(
                text_entry.get("license"), document.get("license")
            )
        ):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a documentation grant text is not the exact supporting asset",
            )
        expected[path] = {
            "path": path,
            "license": document.get("license"),
            "outputSha256": document.get("outputSha256"),
            "licenseTextPath": document.get("licenseTextPath"),
            "licenseTextSha256": document.get("licenseTextSha256"),
            "textAssetLicense": text_entry.get("license"),
            "grantSource": "OUTPUT_FILE_ASSET",
        }
    if len(grants) != len(expected):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_INVALID",
            "the documentation grants are incomplete or duplicated",
        )
    for grant in grants:
        if not isinstance(grant, dict):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a documentation grant is malformed",
            )
        reference = expected.get(grant.get("path"))
        if reference is None or grant != reference:
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "a documentation grant is not the exact Core projection",
            )


_NIX32_ALPHABET = "0123456789abcdfghijklmnpqrsvwxyz"


def _nix32_from_sri(value: str) -> str:
    raw = base64.b64decode(value.split("-", 1)[1])
    length = (len(raw) * 8 - 1) // 5 + 1
    encoded = []
    for index in range(length - 1, -1, -1):
        bit = index * 5
        low = bit // 8
        shift = bit % 8
        current = raw[low] >> shift
        if low + 1 < len(raw):
            current |= raw[low + 1] << (8 - shift)
        encoded.append(_NIX32_ALPHABET[current & 31])
    return "".join(encoded)


def _source_proof_vector(
    identity: dict[str, Any],
) -> tuple[dict[str, Any] | None, str | None]:
    """Return the single canonical source proof for this exact identity."""
    if not SOURCE_VECTOR_FILE.is_file():
        return None, None
    vector = json.loads(SOURCE_VECTOR_FILE.read_text(encoding="utf-8"))
    selector = vector.get("identity", {})
    if (
        selector.get("pin") != identity.get("pin")
        or selector.get("name") != identity.get("name")
        or selector.get("version") != identity.get("version")
    ):
        return None, None
    return vector.get("sourceProof"), vector.get("canonicalSha256")


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
        if item.get("outputSha256") != output.get("sha256"):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation output digest does not match the exact output",
            )
        text_entry = next(
            (
                evidence_entry
                for evidence_entry in identity.get("licenseEvidence", [])
                if evidence_entry.get("path") == item.get("licenseTextPath")
            ),
            None,
        )
        if (
            text_entry is None
            or text_entry.get("sha256") != item.get("licenseTextSha256")
            or not _file_text_supports(
                text_entry.get("license"), item.get("license")
            )
        ):
            raise LicenseDecisionError(
                "ENVIRONMENT_LICENSE_DECISION_INVALID",
                "the documentation text path/licence/digest does not match the exact source",
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
    _validate_v3_rights(receipt, identity)
    expected_source, expected_source_sha = _source_proof_vector(identity)
    if expected_source is None or expected_source_sha is None:
        raise LicenseDecisionError(
            "ENVIRONMENT_SOURCE_EVIDENCE_STALE",
            "the decision identity has no canonical source proof vector",
        )
    source_evidence = receipt.get("sourceEvidence")
    if source_evidence != expected_source:
        raise LicenseDecisionError(
            "ENVIRONMENT_SOURCE_EVIDENCE_STALE",
            "the delivered source evidence does not match the canonical identity proof",
        )
    if (
        receipt.get("sourceEvidenceSha256") != expected_source_sha
        or _sha_value(source_evidence) != expected_source_sha
    ):
        raise LicenseDecisionError(
            "ENVIRONMENT_SOURCE_EVIDENCE_STALE",
            "the source evidence digest does not match its exact proof",
        )
    content = {
        key: value for key, value in receipt.items() if key != "contentSha256"
    }
    if receipt.get("contentSha256") != _sha_value(content):
        raise LicenseDecisionError(
            "ENVIRONMENT_LICENSE_DECISION_STALE",
            "the licence decision content digest is not exact",
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
