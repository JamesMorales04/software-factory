"""Contract tests for the declarative licence catalog/profile (slice B)."""

from __future__ import annotations

import json
import unittest
from dataclasses import replace
from pathlib import Path

from environment_builder import license_policy as lp

HERE = Path(__file__).resolve().parent

ALLOWED_IDS = {
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "ISC",
    "LGPL-2.1-only",
    "LGPL-2.1-or-later",
    "LGPL-3.0-only",
    "LGPL-3.0-or-later",
    "MIT",
    "MIT-0",
    "MPL-2.0",
    "Unlicense",
    "Zlib",
}
CLOSURE_ONLY_IDS = {
    "BSD-4-Clause",
    "BSD-4-Clause-UC",
    "GPL-2.0-only",
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "LGPL-2.0-only",
    "LGPL-2.0-or-later",
    "NCSA",
    "OpenSSL",
    "PostgreSQL",
    "Python-2.0",
    "curl",
    "publicDomain",
}
DENIED_IDS = {
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "BUSL-1.1",
    "Commons-Clause",
    "SSPL-1.0",
}
UTIL_LINUX = (
    "GPL-2.0-only AND GPL-2.0-or-later AND GPL-3.0-or-later "
    "AND LGPL-2.1-or-later AND BSD-3-Clause AND BSD-4-Clause-UC AND publicDomain"
)


class LicensePolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.catalog, cls.profile = lp.load_default()

    def decision(self, expression: str, context: lp.LicenseContext) -> str:
        return lp.evaluate(expression, context, self.catalog, self.profile).value

    def test_catalog_is_versioned_hashed_and_unique(self) -> None:
        self.assertEqual(self.catalog.revision, "spdx-catalog-2026-11-16")
        self.assertTrue(self.catalog.spdx_license_list_version)
        self.assertEqual(len(self.catalog.entries), 35)
        self.assertEqual(
            set(self.catalog.entries),
            ALLOWED_IDS | CLOSURE_ONLY_IDS | DENIED_IDS,
        )
        self.assertEqual(
            len(self.catalog.sha256),
            64,
        )

    def test_public_domain_is_a_documented_alias_not_a_new_id(self) -> None:
        self.assertNotIn("Public Domain", self.catalog.entries)
        self.assertEqual(self.catalog.alias_index["Public Domain"], "publicDomain")
        self.assertEqual(
            self.decision("Public Domain", lp.LicenseContext.CLOSURE), "allowed"
        )
        self.assertEqual(
            self.decision("Public Domain", lp.LicenseContext.SELECTED), "unknown"
        )

    def test_catalog_schema_requires_a_strict_integer_one(self) -> None:
        import json
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "catalog.json"
            document = {
                "schema": 1,
                "revision": "probe",
                "spdxLicenseListVersion": "probe",
                "sha256": "0" * 64,
                "entries": [],
            }
            for malformed in (True, False, 1.0, "1", None, -1, 0):
                document["schema"] = malformed
                path.write_text(json.dumps(document), encoding="utf-8")
                with self.assertRaises(RuntimeError, msg=repr(malformed)):
                    lp.load_catalog(path)

    def test_legacy_profile_is_inactive_until_families_are_declared(self) -> None:
        self.assertEqual(self.profile.mode, "legacy-current")
        self.assertFalse(self.profile.families_configured)
        self.assertFalse(lp.b_active(self.profile))
        personal = replace(
            self.profile, mode="personal", families_configured=True
        )
        self.assertTrue(lp.b_active(personal))

    def test_selected_closure_and_unmodelled_contexts(self) -> None:
        matrix = lp.verdict_matrix(self.catalog, self.profile)
        for identifier in ALLOWED_IDS:
            self.assertEqual(matrix["selected"][identifier], "allowed", identifier)
            self.assertEqual(matrix["closure"][identifier], "allowed", identifier)
            self.assertEqual(matrix["build"][identifier], "allowed", identifier)
            self.assertEqual(matrix["devTest"][identifier], "allowed", identifier)
        for identifier in CLOSURE_ONLY_IDS:
            self.assertEqual(matrix["selected"][identifier], "unknown", identifier)
            self.assertEqual(matrix["closure"][identifier], "allowed", identifier)
            self.assertEqual(matrix["build"][identifier], "allowed", identifier)
            self.assertEqual(matrix["devTest"][identifier], "allowed", identifier)
        for identifier in DENIED_IDS:
            for context in ("selected", "closure", "build", "devTest"):
                self.assertEqual(matrix[context][identifier], "denied", identifier)
        for identifier in self.catalog.entries:
            self.assertEqual(
                matrix["runtimeDistributed"][identifier],
                "notEvaluated",
                identifier,
            )

    def test_unknown_denied_and_selected_vs_closure_controls(self) -> None:
        for value in ("PSF-2.0", "LicenseRef-Not-A-Catalogue-Id", "NOASSERTION", "NONE"):
            self.assertEqual(
                self.decision(value, lp.LicenseContext.CLOSURE), "unknown", value
            )
        self.assertEqual(
            self.decision("AGPL-3.0-only", lp.LicenseContext.CLOSURE), "denied"
        )
        for identifier in ("Python-2.0", "BSD-4-Clause-UC"):
            self.assertEqual(
                self.decision(identifier, lp.LicenseContext.SELECTED), "unknown"
            )
            self.assertEqual(
                self.decision(identifier, lp.LicenseContext.CLOSURE), "allowed"
            )
        self.assertEqual(
            self.decision(UTIL_LINUX, lp.LicenseContext.CLOSURE), "allowed"
        )
        self.assertEqual(
            self.decision(UTIL_LINUX, lp.LicenseContext.SELECTED), "unknown"
        )

    def test_real_33_entry_inventory_is_allowed_in_the_closure(self) -> None:
        fixture = json.loads(
            (HERE / "fixtures" / "closure-license-inventory-33.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(fixture["count"], 33)
        for entry in fixture["entries"]:
            self.assertEqual(
                self.decision(entry["license"], lp.LicenseContext.CLOSURE),
                "allowed",
                f"{entry['name']} {entry['version']}",
            )
        selected = {
            entry["license"]: self.decision(
                entry["license"], lp.LicenseContext.SELECTED
            )
            for entry in fixture["entries"]
        }
        self.assertEqual(selected["Python-2.0"], "unknown")
        self.assertEqual(selected[UTIL_LINUX], "unknown")
        self.assertEqual(selected["MIT"], "allowed")

    def test_content_hash_binds_catalog_and_profile_not_labels(self) -> None:
        base = lp.binding_sha256(self.catalog, self.profile)
        changed_entry = replace(
            self.catalog.entries["MIT"], family="strong-copyleft"
        )
        changed_catalog = replace(
            self.catalog,
            entries={**self.catalog.entries, "MIT": changed_entry},
        )
        self.assertNotEqual(base, lp.binding_sha256(changed_catalog, self.profile))
        renamed = replace(self.profile, revision="renamed-label")
        self.assertEqual(base, lp.binding_sha256(self.catalog, renamed))

    def test_cross_language_matrix_fixture_matches_the_shared_data(self) -> None:
        fixture = json.loads(
            (HERE / "fixtures" / "license-context-matrix.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(
            fixture["catalogSha256"], lp.catalog_content_sha256(self.catalog)
        )
        self.assertEqual(
            fixture["profileSha256"], lp.profile_content_sha256(self.profile)
        )
        matrix = lp.verdict_matrix(self.catalog, self.profile)
        for context, row in fixture["contexts"].items():
            self.assertEqual(set(row), set(matrix[context]), context)
            for identifier, expected in row.items():
                self.assertEqual(
                    matrix[context][identifier].value,
                    expected,
                    f"{context}/{identifier}",
                )

    def test_legacy_oracle_4dcd_matches_current_interpreters(self) -> None:
        oracle = json.loads(
            (HERE / "fixtures" / "legacy-policy-oracle-4dcd.json").read_text(
                encoding="utf-8"
            )
        )
        for identifier in oracle["selected"]:
            self.assertEqual(
                self.decision(identifier, lp.LicenseContext.SELECTED),
                "allowed",
                identifier,
            )
        for identifier in oracle["closure"]:
            self.assertEqual(
                self.decision(identifier, lp.LicenseContext.CLOSURE),
                "allowed",
                identifier,
            )
        for identifier in oracle["denied"]:
            self.assertEqual(
                self.decision(identifier, lp.LicenseContext.SELECTED),
                "denied",
                identifier,
            )
            self.assertEqual(
                self.decision(identifier, lp.LicenseContext.CLOSURE),
                "denied",
                identifier,
            )
        for expression, expected in oracle["compounds"].items():
            if "selected" not in expected:
                continue
            self.assertEqual(
                self.decision(expression, lp.LicenseContext.SELECTED),
                expected["selected"],
                expression,
            )
            self.assertEqual(
                self.decision(expression, lp.LicenseContext.CLOSURE),
                expected["closure"],
                expression,
            )

    def test_authorized_f5_denied_atom_precedes_unknown(self) -> None:
        oracle = json.loads(
            (HERE / "fixtures" / "legacy-policy-oracle-4dcd.json").read_text(
                encoding="utf-8"
            )
        )
        for expression, expected in oracle["compounds"].items():
            if "authorizedF5" not in expected:
                continue
            for context in (
                lp.LicenseContext.SELECTED,
                lp.LicenseContext.CLOSURE,
            ):
                self.assertEqual(
                    self.decision(expression, context),
                    expected["authorizedF5"],
                    f"{expression}/{context.value}",
                )

    def test_profile_loader_enforces_types_presence_and_decisive_hash(self) -> None:
        import tempfile

        source = json.loads(
            (Path(lp.__file__).with_name("license_profile.json")).read_text(
                encoding="utf-8"
            )
        )

        def load(document: dict) -> lp.LicenseProfile:
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "profile.json"
                path.write_text(json.dumps(document), encoding="utf-8")
                return lp.load_profile(path)

        self.assertFalse(lp.b_active(load(source)))

        personal = json.loads(json.dumps(source))
        personal["mode"] = "personal"
        personal["familiesConfigured"] = True
        personal["sha256"] = lp._profile_sha(personal)
        self.assertTrue(lp.b_active(load(personal)))

        unrehashed = json.loads(json.dumps(personal))
        unrehashed["sha256"] = source["sha256"]
        with self.assertRaises(RuntimeError):
            load(unrehashed)

        for mutation in (
            {"mode": "unknown"},
            {"familiesConfigured": "false"},
            {"familiesConfigured": 1},
        ):
            candidate = json.loads(json.dumps(source))
            candidate.update(mutation)
            candidate["sha256"] = lp._profile_sha(candidate)
            with self.assertRaises(RuntimeError, msg=repr(mutation)):
                load(candidate)

        unknown_family = json.loads(json.dumps(source))
        unknown_family["contexts"]["selected"]["acceptedFamilies"] = ["not-a-family"]
        unknown_family["sha256"] = lp._profile_sha(unknown_family)
        with self.assertRaises(RuntimeError):
            load(unknown_family)

        for missing in ("mode", "familiesConfigured", "deniedIds", "contexts"):
            candidate = json.loads(json.dumps(source))
            candidate.pop(missing)
            with self.assertRaises(RuntimeError):
                load(candidate)

        personal_profile = replace(
            self.profile, mode="personal", families_configured=True
        )
        self.assertNotEqual(
            lp.profile_content_sha256(personal_profile),
            lp.profile_content_sha256(self.profile),
        )
        denied_change = replace(self.profile, denied_ids=frozenset({"MIT"}))
        self.assertNotEqual(
            lp.profile_content_sha256(denied_change),
            lp.profile_content_sha256(self.profile),
        )


if __name__ == "__main__":
    unittest.main()
