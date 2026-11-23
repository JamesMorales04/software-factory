"""Exact builder licence-decision receipt tests."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

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
    proof, _ = license_decision._source_proof_vector(identity)
    assert proof is not None
    return proof

def receipt() -> dict:
    asset = json.loads(license_decision.ASSET_FILE.read_text(encoding="utf-8"))
    identity = asset["identities"][0]
    evidence = asset["evidence"]
    catalog, profile = license_policy.load_default()
    software = catalog.entries[identity["license"]]
    docs = [
        item
        for item in identity["outputFiles"]
        if item.get("purpose") == "documentation"
    ]
    fdl = next(
        entry for entry in identity["licenseEvidence"] if entry["path"] == "doc/fdl.texi"
    )
    copying = next(
        entry for entry in identity["licenseEvidence"] if entry["path"] == "COPYING"
    )
    documentation = [
        {
            "path": item["path"],
            "license": item["license"],
            "outputSha256": item["sha256"],
            "licenseTextPath": (
                fdl["path"] if item["license"] == "GFDL-1.3-or-later" else copying["path"]
            ),
            "licenseTextSha256": (
                fdl["sha256"] if item["license"] == "GFDL-1.3-or-later" else copying["sha256"]
            ),
            "obligations": list(catalog.entries[item["license"]].obligations),
            "verdict": (
                "UNKNOWN_CLASSIFIED"
                if item["license"] == "GFDL-1.3-or-later"
                else "ALLOWED"
            ),
        }
        for item in docs
    ]
    grant = next(
        entry
        for entry in identity["licenseEvidence"]
        if entry["role"] == "licensing" and entry["license"] == identity["license"]
    )
    documentation_grants = []
    for document in documentation:
        text = next(
            entry
            for entry in identity["licenseEvidence"]
            if entry["path"] == document["licenseTextPath"]
        )
        documentation_grants.append(
            {
                "path": document["path"],
                "license": document["license"],
                "outputSha256": document["outputSha256"],
                "licenseTextPath": document["licenseTextPath"],
                "licenseTextSha256": document["licenseTextSha256"],
                "textAssetLicense": text["license"],
                "grantSource": "OUTPUT_FILE_ASSET",
            }
        )
    payload = {
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
        "decisionContentSha256": "d" * 64,
        "contractRevision": license_decision._V3_CONTRACT,
        "fileLabelSource": "ASSET",
        "packageRightsSource": "LICENSING_FACT",
        "documentationGrantSource": "OUTPUT_FILE_ASSET",
        "modelTextLicenseIgnored": True,
        "modelObligationsIgnored": True,
        "fileEvidence": [
            {
                "path": copying["path"],
                "sha256": copying["sha256"],
                "assetLicense": copying["license"],
                "role": "license-text",
            }
        ],
        "packageRights": [
            {
                "atom": identity["license"],
                "license": grant["license"],
                "path": grant["path"],
                "sha256": grant["sha256"],
                "source": identity["source"],
            }
        ],
        "documentationGrants": documentation_grants,
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
    payload["contentSha256"] = license_decision._sha_value(payload)
    return payload


class LicenseDecisionTests(unittest.TestCase):
    def test_exact_decision_is_accepted(self) -> None:
        exact = receipt()
        self.assertNotEqual(
            exact["decisionContentSha256"], exact["contentSha256"]
        )
        identity = license_decision.validate_license_decision_receipt(exact, plan())
        self.assertEqual(identity["name"], "ed")
        self.assertEqual(identity["license"], "GPL-2.0-or-later")

    def test_core_record_identity_and_stage_string_are_accepted(self) -> None:
        record = receipt()
        asset = json.loads(license_decision.ASSET_FILE.read_text(encoding="utf-8"))
        record["identity"] = asset["identities"][0]
        record["stage"] = json.dumps(record["stage"], separators=(",", ":"))
        record["contentSha256"] = license_decision._sha_value(
            {key: value for key, value in record.items() if key != "contentSha256"}
        )
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

    def test_shared_resolution_observation_vector_is_canonical(self) -> None:
        vector = json.loads(
            (
                license_decision.ASSET_FILE.parent
                / "resolution_observation_vector.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(
            license_decision._sha_value(vector["observation"]),
            vector["resolutionKey"],
        )
        self.assertEqual(
            vector["resolutionKey"],
            "645a31aaaa997b55cc63278c5b4f691b5ad8f87895530d98642797d6683ca199",
        )

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
        proof, digest = license_decision._source_proof_vector(identity)
        self.assertIsNotNone(proof)
        self.assertIsNotNone(digest)
        self.assertEqual(proof, vector["sourceProof"])
        self.assertEqual(
            license_decision._sha_value(proof), vector["canonicalSha256"]
        )
        self.assertNotIn("signatureValid", proof)
        self.assertNotIn("sourceInOutputInputDrvs", proof)

    def test_license_gate_failure_writes_only_a_realisation_pointer(self) -> None:
        manifest = [
            {
                "path": "/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1",
                "deriver": "/nix/store/frkdwgpbgh17hz57giw5rm3bs8bsflfb-ed-1.21.1.drv",
                "narHash": "sha256-humgomxjPuMnaa5VWRl8ondk5lDth9N3px977TJOpG4=",
                "references": [],
            }
        ]
        plan = {"planSha256": "a" * 64}
        spec = {"specSha256": "b" * 64}
        with tempfile.TemporaryDirectory() as directory:
            cli._write_realisation(
                Path(directory), plan, spec, manifest[0]["path"], manifest
            )
            self.assertFalse(
                (Path(directory) / "identity-facts-manifest.json").exists()
            )
            payload = json.loads(
                (Path(directory) / "realisation.json").read_text(encoding="utf-8")
            )
        self.assertEqual(payload["status"], "REALISED")
        self.assertEqual(payload["planSha256"], "a" * 64)
        self.assertEqual(payload["specSha256"], "b" * 64)
        self.assertEqual(payload["storePath"], manifest[0]["path"])
        self.assertNotIn("facts", payload)
        self.assertEqual(
            payload["closureManifestSha256"],
            cli.closure_manifest_sha256(manifest),
        )

    def test_missing_full_source_proof_is_stale(self) -> None:
        receipt_value = receipt()
        del receipt_value["sourceEvidence"]
        receipt_value["contentSha256"] = license_decision._sha_value(
            {
                key: value
                for key, value in receipt_value.items()
                if key != "contentSha256"
            }
        )
        with self.assertRaises(license_decision.LicenseDecisionError) as caught:
            license_decision.validate_license_decision_receipt(receipt_value, plan())
        self.assertEqual(
            caught.exception.code, "ENVIRONMENT_SOURCE_EVIDENCE_STALE"
        )

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

    def test_v3_contract_and_fact_projection_is_required(self) -> None:
        missing_contract = receipt()
        del missing_contract["contractRevision"]
        with self.assertRaises(license_decision.LicenseDecisionError) as caught:
            license_decision.validate_license_decision_receipt(
                missing_contract, plan()
            )
        self.assertEqual(
            caught.exception.code, "ENVIRONMENT_LICENSE_DECISION_STALE"
        )

        for field, value in (
            ("fileEvidence", None),
            ("packageRights", []),
            ("documentationGrants", []),
        ):
            missing = receipt()
            missing[field] = value
            with self.assertRaises(license_decision.LicenseDecisionError):
                license_decision.validate_license_decision_receipt(missing, plan())

    def test_v3_file_evidence_is_exact_and_has_no_family_shortcut(self) -> None:
        changed = receipt()
        changed["fileEvidence"][0]["assetLicense"] = "GPL-2.0-or-later"
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())

        changed = receipt()
        changed["fileEvidence"][0]["sha256"] = "f" * 64
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())

    def test_v3_package_rights_are_exact_and_source_bound(self) -> None:
        changed = receipt()
        changed["packageRights"][0]["license"] = "GPL-2.0-only"
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())

        changed = receipt()
        changed["packageRights"][0]["source"]["storePath"] = "/nix/store/wrong"
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())

    def test_v3_documentation_grants_are_exact(self) -> None:
        changed = receipt()
        changed["documentationGrants"][0]["outputSha256"] = "f" * 64
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())

        changed = receipt()
        changed["documentation"][0]["outputSha256"] = "f" * 64
        with self.assertRaises(license_decision.LicenseDecisionError):
            license_decision.validate_license_decision_receipt(changed, plan())

    def test_v3_decision_digest_is_bound_by_the_overlay_digest(self) -> None:
        changed = receipt()
        changed["decisionContentSha256"] = "e" * 64
        with self.assertRaises(license_decision.LicenseDecisionError) as caught:
            license_decision.validate_license_decision_receipt(changed, plan())
        self.assertEqual(
            caught.exception.code, "ENVIRONMENT_LICENSE_DECISION_STALE"
        )

    def test_v3_requires_the_current_source_and_model_authority_markers(self) -> None:
        for field, value in (
            ("fileLabelSource", "MODEL"),
            ("packageRightsSource", "MODEL"),
            ("documentationGrantSource", "MODEL"),
            ("modelTextLicenseIgnored", False),
            ("modelObligationsIgnored", False),
        ):
            changed = receipt()
            changed[field] = value
            with self.assertRaises(license_decision.LicenseDecisionError):
                license_decision.validate_license_decision_receipt(changed, plan())


if __name__ == "__main__":
    unittest.main()
