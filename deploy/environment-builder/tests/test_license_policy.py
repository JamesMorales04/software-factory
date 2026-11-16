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
    "Public Domain",
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
        self.assertEqual(len(self.catalog.entries), 36)
        self.assertEqual(
            set(self.catalog.entries),
            ALLOWED_IDS | CLOSURE_ONLY_IDS | DENIED_IDS,
        )
        self.assertEqual(
            len(self.catalog.sha256),
            64,
        )

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

    def test_legacy_profile_matches_the_current_gate_for_every_known_id(self) -> None:
        from environment_builder import policy
        from environment_builder.errors import BuilderError

        def map_error(code: str) -> str:
            return "denied" if code == "ENVIRONMENT_LICENSE_DENIED" else "unknown"

        def selected_verdict(value: str) -> str:
            try:
                policy.validate_license_expression(value)
                return "allowed"
            except BuilderError as error:
                return map_error(error.code)

        def closure_verdict(value: str) -> str:
            entry = {
                "path": "/nix/store/" + "a" * 32 + "-probe",
                "name": "probe",
                "version": "1",
                "license": value,
                "unfree": False,
            }
            try:
                policy.validate_closure_licenses([entry])
                return "allowed"
            except BuilderError as error:
                return map_error(error.code)

        for identifier in sorted(self.catalog.entries):
            self.assertEqual(
                selected_verdict(identifier),
                self.decision(identifier, lp.LicenseContext.SELECTED),
                identifier,
            )
            self.assertEqual(
                closure_verdict(identifier),
                self.decision(identifier, lp.LicenseContext.CLOSURE),
                identifier,
            )


if __name__ == "__main__":
    unittest.main()
