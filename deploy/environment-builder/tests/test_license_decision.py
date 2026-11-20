"""Exact builder licence-decision receipt tests."""

from __future__ import annotations

import json
import unittest
from pathlib import Path

from environment_builder import license_decision, license_policy


EV = Path(
    "/home/james/.local/share/software-factory/identity-facts-evidence-20261119/"
    "uat-ed-source-0fa4990e"
)


def plan() -> dict:
    return {
        "planSha256": "c" * 64,
        "revision": 1,
        "licenseInventorySha256": "1" * 64,
        "licenseInventory": {
            "inventorySha256": "1" * 64,
            "sbomSha256": "2" * 64,
        },
        "baseCommit": "4fdf1b15f862b2564642443a71ef3e25a2785013",
    }


def receipt() -> dict:
    asset = json.loads(license_decision.ASSET_FILE.read_text(encoding="utf-8"))
    identity = asset["identities"][0]
    evidence = asset["evidence"]
    catalog, profile = license_policy.load_default()
    software = catalog.entries[identity["license"]]
    docs = [
        item
        for item in identity["outputFiles"]
        if item.get("purpose") == "documentation" and item.get("license") == "GFDL-1.3-or-later"
    ]
    documentation = [
        {
            "path": item["path"],
            "license": item["license"],
            "licenseTextSha256": next(
                entry["sha256"]
                for entry in identity["licenseEvidence"]
                if entry["path"] == "doc/fdl.texi"
            ),
            "obligations": list(catalog.entries[item["license"]].obligations),
            "verdict": "UNKNOWN_CLASSIFIED",
        }
        for item in docs
    ]
    return {
        "decision": "ACCEPT",
        "status": "ACTIVE",
        "revokedAt": None,
        "identity": {
            "pin": identity["pin"],
            "name": identity["name"],
            "version": identity["version"],
            "outputStorePath": identity["output"]["storePath"],
            "outputNarHash": identity["output"]["narHash"],
        },
        "licenseExpression": identity["license"],
        "selectedBranch": identity["license"],
        "softwareObligations": list(software.obligations),
        "documentation": documentation,
        "documentationObligations": list(catalog.entries["GFDL-1.3-or-later"].obligations),
        "evidenceSha256": evidence["licenseEvidenceCanonicalSha256"],
        "identityFactsReceiptDigest": "f" * 64,
        "catalogSha256": catalog.sha256,
        "profileSha256": profile.sha256,
        "policySha256": license_policy.binding_sha256(catalog, profile),
        "stage": {
            "stage": "planInventoryEvidence",
            "planSha256": "c" * 64,
            "planRevision": 1,
            "inventorySha256": "1" * 64,
            "evidenceBundleSha256": "2" * 64,
            "identityFactsReceiptDigest": "f" * 64,
            "candidate": {
                "kind": "base",
                "baseCommit": "4fdf1b15f862b2564642443a71ef3e25a2785013",
            },
        },
    }


class LicenseDecisionTests(unittest.TestCase):
    def test_exact_decision_is_accepted(self) -> None:
        identity = license_decision.validate_license_decision_receipt(receipt(), plan())
        self.assertEqual(identity["name"], "ed")
        self.assertEqual(identity["license"], "GPL-2.0-or-later")

    def test_core_record_identity_and_stage_string_are_accepted(self) -> None:
        record = receipt()
        asset = json.loads(license_decision.ASSET_FILE.read_text(encoding="utf-8"))
        record["identity"] = asset["identities"][0]
        record["stage"] = json.dumps(record["stage"], separators=(",", ":"))
        identity = license_decision.validate_license_decision_receipt(record, plan())
        self.assertEqual(identity["name"], "ed")

    def test_changed_plan_stage_is_stale(self) -> None:
        changed = plan()
        changed["revision"] = 2
        with self.assertRaises(license_decision.LicenseDecisionError) as caught:
            license_decision.validate_license_decision_receipt(receipt(), changed)
        self.assertEqual(caught.exception.code, "ENVIRONMENT_LICENSE_DECISION_STALE")

    def test_revoked_or_denied_decision_fails_closed(self) -> None:
        revoked = receipt()
        revoked["status"] = "REVOKED"
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(revoked, plan())
        denied = receipt()
        denied["decision"] = "DENY"
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(denied, plan())

    def test_wrong_identity_or_evidence_is_invalid(self) -> None:
        wrong = receipt()
        wrong["identity"]["outputStorePath"] = "/nix/store/" + "a" * 32 + "-ed-1.21.1"
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(wrong, plan())
        evidence = receipt()
        evidence["evidenceSha256"] = "0" * 64
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(evidence, plan())

    def test_documentation_obligations_are_exact(self) -> None:
        changed = receipt()
        changed["documentation"][0]["obligations"] = ["notice"]
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())


if __name__ == "__main__":
    unittest.main()
