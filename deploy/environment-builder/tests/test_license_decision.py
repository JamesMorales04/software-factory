"""Exact builder licence-decision receipt tests."""

from __future__ import annotations

import json
import unittest
from environment_builder import cli, license_decision, license_policy


ENTRIES = [{"path": "/nix/store/example", "license": "MIT"}]


def plan() -> dict:
    return {
        "planSha256": "c" * 64,
        "revision": 1,
        "licenseInventorySha256": "1" * 64,
        "licenseInventory": {
            "inventorySha256": "1" * 64,
            "sbomSha256": "2" * 64,
            "entries": ENTRIES,
        },
        "baseCommit": "4fdf1b15f862b2564642443a71ef3e25a2785013",
    }


def source_proof(identity: dict, evidence: dict) -> dict:
    """Derive the exact source proof from the signed identity asset.

    The proof is evidence assembled from cache.nixos.org/pathinfo and the
    retained store; it is validated by the backend, not granted here.
    """
    return license_decision._exact_source_proof(identity, evidence)

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
    fdl = next(
        entry for entry in identity["licenseEvidence"] if entry["path"] == "doc/fdl.texi"
    )
    documentation = [
        {
            "path": item["path"],
            "license": item["license"],
            "outputSha256": item["sha256"],
            "licenseTextPath": fdl["path"],
            "licenseTextSha256": fdl["sha256"],
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
        "sourceEvidence": source_proof(identity, evidence),
        "sourceEvidenceSha256": license_decision._sha_value(
            source_proof(identity, evidence)
        ),
        "identityFactsReceiptDigest": "f" * 64,
        "catalogSha256": catalog.sha256,
        "profileSha256": profile.sha256,
        "policySha256": license_policy.binding_sha256(catalog, profile),
        "stage": {
            "stage": "planInventoryEvidence",
            "planSha256": "c" * 64,
            "planRevision": 1,
            "inventorySha256": "1" * 64,
            "evidenceBundleSha256": license_decision._sha_value({"entries": ENTRIES}),
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

    def test_missing_decision_and_document_association_fail_closed(self) -> None:
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt({}, plan())
        missing_output = receipt()
        del missing_output["documentation"][0]["outputSha256"]
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(missing_output, plan())
        missing_text = receipt()
        del missing_text["documentation"][0]["licenseTextPath"]
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(missing_text, plan())

    def test_shared_source_proof_vector_is_the_single_canonical_representation(self) -> None:
        vector = json.loads(
            (
                license_decision.ASSET_FILE.parent
                / "source_evidence_proof_vector.json"
            ).read_text(encoding="utf-8")
        )
        asset = json.loads(license_decision.ASSET_FILE.read_text(encoding="utf-8"))
        selector = vector["identity"]
        identity = next(
            entry
            for entry in asset["identities"]
            if entry["pin"] == selector["pin"]
            and entry["name"] == selector["name"]
            and entry["version"] == selector["version"]
        )
        proof = license_decision._exact_source_proof(identity, asset["evidence"])
        self.assertEqual(proof, vector["sourceProof"])
        self.assertEqual(
            license_decision._sha_value(proof), vector["canonicalSha256"]
        )
        self.assertNotIn("signatureValid", proof)
        self.assertNotIn("sourceInOutputInputDrvs", proof)

    def test_license_gate_failure_manifest_is_bounded_exact_evidence(self) -> None:
        manifest = [
            {
                "path": "/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1",
                "deriver": "/nix/store/frkdwgpbgh17hz57giw5rm3bs8bsflfb-ed-1.21.1.drv",
                "narHash": "sha256-humgomxjPuMnaa5VWRl8ondk5lDth9N3px977TJOpG4=",
                "references": [],
            }
        ]
        payload = cli._failure_identity_facts_manifest(manifest, manifest[0]["path"])
        self.assertEqual(payload["observedRoot"], manifest[0]["path"])
        self.assertEqual(payload["factCount"], 1)
        self.assertFalse(payload["truncated"])
        self.assertEqual(payload["facts"][0]["outputs"], [manifest[0]["path"]])
        self.assertEqual(
            payload["closureManifestSha256"],
            cli.hashlib.sha256(
                b'[{"storePath":"/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1","deriver":"/nix/store/frkdwgpbgh17hz57giw5rm3bs8bsflfb-ed-1.21.1.drv","narHash":"sha256-humgomxjPuMnaa5VWRl8ondk5lDth9N3px977TJOpG4=","outputs":["/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1"]}]'
            ).hexdigest(),
        )

    def test_source_digest_without_full_proof_is_accepted(self) -> None:
        receipt_value = receipt()
        del receipt_value["sourceEvidence"]
        accepted = license_decision.validate_license_decision_receipt(
            receipt_value, plan()
        )
        self.assertEqual(accepted["name"], "ed")
        self.assertEqual(accepted["source"]["storePath"], "/nix/store/wa88qds9615j2r5imfryzbyg86c97iga-ed-1.21.1.tar.lz")

    def test_source_proof_missing_or_tampered_is_stale(self) -> None:
        missing = receipt()
        del missing["sourceEvidenceSha256"]
        with self.assertRaises(license_decision.LicenseDecisionError) as caught:
            license_decision.validate_license_decision_receipt(missing, plan())
        self.assertEqual(caught.exception.code, "ENVIRONMENT_SOURCE_EVIDENCE_STALE")
        tampered = receipt()
        tampered["sourceEvidence"]["flatSha256"] = "0" * 64
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(tampered, plan())

    def test_builder_overlay_is_exact_and_absent_by_default(self) -> None:
        from environment_builder.cli import _with_license_decisions

        decisions = [{"id": "decision-1"}]
        merged = _with_license_decisions({"licenseDecisions": decisions}, {"plan": 1})
        self.assertEqual(merged["licenseDecisions"], decisions)
        self.assertEqual(merged["plan"], 1)
        unchanged = _with_license_decisions({}, {"plan": 1})
        self.assertNotIn("licenseDecisions", unchanged)

    def test_documentation_obligations_are_exact(self) -> None:
        changed = receipt()
        changed["documentation"][0]["obligations"] = ["notice"]
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())


if __name__ == "__main__":
    unittest.main()
