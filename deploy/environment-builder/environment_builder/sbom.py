"""Deterministic CycloneDX SBOM and license evidence.

The SBOM covers the substrate, every package selected by the frozen plan and
the realised Nix closure. Only plan-selected packages participate in the
License Gate; transitive closure entries are recorded for provenance and are
marked ``NOASSERTION`` so they are never misrepresented as reviewed licenses.
"""

from __future__ import annotations

import re
import uuid
from typing import Any

from . import BUILDER_POLICY_REVISION, LICENSE_POLICY_REVISION, SCHEMA_VERSION
from .util import canonical_json, pretty_json, sha256_bytes, sha256_text

STORE_NAME = re.compile(r"^[a-z0-9]{32}-(.+)$")
VERSION = re.compile(r"^(?P<name>.+?)-(?P<version>[0-9][A-Za-z0-9._+-]*)$")

SUBSTRATE = (
    {
        "name": "nix",
        "version": "pinned by builder receipt",
        "license": "LGPL-2.1-only",
        "source": "https://github.com/NixOS/nix/blob/master/COPYING",
        "attribution": (
            "Nix is invoked as a separate LGPL-2.1 tool; Factory does not link Nix "
            "libraries. Preserve the license, notices and source/relink offer when "
            "distributing a modified Nix binary."
        ),
    },
    {
        "name": "nixpkgs",
        "version": "pinned by the environment plan",
        "license": "MIT",
        "source": "https://github.com/NixOS/nixpkgs/blob/master/COPYING",
        "attribution": (
            "Copyright 2003-2026 Eelco Dolstra and the Nixpkgs/NixOS contributors. "
            "Preserve the MIT copyright and permission notice. Each selected package "
            "retains its own license."
        ),
    },
    {
        "name": "devenv",
        "version": "pinned by builder receipt",
        "license": "Apache-2.0",
        "source": "https://github.com/cachix/devenv/blob/main/LICENSE",
        "attribution": (
            "Apache License 2.0. Retain notices and any NOTICE text, mark modified "
            "files and comply with the patent-termination terms. The repository "
            "LICENSE and Cargo.toml are authoritative."
        ),
    },
)


def _split_store_path(path: str) -> tuple[str, str]:
    base = path.rsplit("/", 1)[-1]
    match = STORE_NAME.match(base)
    if not match:
        return base, "unknown"
    name = match.group(1)
    version_match = VERSION.match(name)
    if version_match:
        return version_match.group("name"), version_match.group("version")
    return name, "unknown"


def _component(
    *,
    name: str,
    version: str,
    license_value: str,
    scope: str,
    source: str = "",
    sha256: str = "",
    attribution: str = "",
    version_constraint: str = "",
    purl: str | None = None,
) -> dict[str, Any]:
    component: dict[str, Any] = {
        "type": "library",
        "bom-ref": f"pkg:{name}@{version}",
        "name": name,
        "version": version,
        "scope": scope,
        "licenses": [{"license": {"id": license_value}}]
        if license_value != "NOASSERTION"
        else [],
        "properties": [
            {"name": "factory:license-attribution", "value": attribution[:2_000]},
            {"name": "factory:source", "value": source[:2_000]},
        ],
    }
    if version_constraint:
        component["properties"].append(
            {"name": "factory:version-constraint", "value": version_constraint[:64]}
        )
    if license_value == "NOASSERTION":
        component["licenses"] = []
        component["properties"].append(
            {"name": "factory:license-status", "value": "NOASSERTION"}
        )
    if purl:
        component["purl"] = purl
    if sha256:
        component["hashes"] = [{"alg": "SHA-256", "content": sha256}]
    return component


def build_sbom(
    *,
    spec: dict[str, Any],
    plan: dict[str, Any],
    manifest: list[dict[str, Any]],
    closure_licenses: list[dict[str, Any]] | None = None,
    packages: list[dict[str, Any]] | None = None,
    nix_version: str,
    devenv_version: str,
) -> dict[str, Any]:
    components: list[dict[str, Any]] = []
    for entry in SUBSTRATE:
        components.append(
            _component(
                name=entry["name"],
                version=entry["version"],
                license_value=entry["license"],
                scope="required",
                source=entry["source"],
                attribution=entry["attribution"],
            )
        )
    plan_entries = {
        str(entry.get("name", "")).lower(): entry
        for entry in plan.get("licenseInventory", {}).get("entries", [])
        if isinstance(entry, dict)
    }
    observed_packages = {
        (str(entry.get("name", "")).lower(), str(entry.get("attr", ""))): entry
        for entry in (packages or [])
        if isinstance(entry, dict)
    }
    for package in plan.get("packages", []):
        if not isinstance(package, dict):
            continue
        name = str(package.get("name", ""))
        constraint = str(package.get("version", ""))
        attr = str(package.get("attr", "")).strip() or name
        observed = observed_packages.get((name.lower(), attr), {})
        # The builder-observed exact version is authoritative in the SBOM; the
        # plan's value is retained only as the resolved version constraint.
        version = str(observed.get("version", "")).strip() or constraint
        selected = plan_entries.get(name.lower(), {})
        components.append(
            _component(
                name=name,
                version=version,
                license_value=str(package.get("license", "NOASSERTION")),
                scope="required",
                source=str(package.get("source", "")),
                sha256=str(package.get("sha256", "")),
                attribution=str(package.get("attribution", "")),
                version_constraint=constraint,
            )
        )
        if selected and selected.get("license") != package.get("license"):
            raise ValueError("contradictory selected-package license")
    for browser in plan.get("browsers", []):
        if not isinstance(browser, dict):
            continue
        name = str(browser.get("name", ""))
        version = str(browser.get("version", ""))
        selected = plan_entries.get(name.lower(), {})
        components.append(
            _component(
                name=name,
                version=version,
                license_value=str(selected.get("license", "NOASSERTION")),
                scope="required",
                source=str(browser.get("source", "")),
                attribution=str(selected.get("attribution", "")),
            )
        )
    closure_by_path = {
        str(entry.get("path", "")): entry
        for entry in (closure_licenses or [])
        if isinstance(entry, dict)
    }
    for entry in manifest:
        path = str(entry.get("path", ""))
        name, version = _split_store_path(path)
        if not name:
            continue
        observed = closure_by_path.get(path, {})
        observed_name = str(observed.get("name", "")).strip()
        observed_version = str(observed.get("version", "")).strip()
        observed_license = str(observed.get("license", "")).strip()
        if observed_name:
            name = observed_name
        if observed_version:
            version = observed_version
        components.append(
            _component(
                name=name,
                version=version,
                license_value=observed_license or "NOASSERTION",
                scope="excluded",
                sha256="",
                attribution=(
                    "Transitive Nix closure entry with builder-observed package "
                    "metadata; not a License Gate approval."
                    if observed_license
                    else "Transitive Nix closure entry recorded for SBOM provenance; "
                    "not a License Gate approval."
                ),
                purl=None,
            )
        )
    # Stable identity and ordering keep the SBOM reproducible for the same spec.
    components.sort(key=lambda component: (component["name"], component["version"], component["bom-ref"]))
    serial = uuid.uuid5(uuid.NAMESPACE_URL, f"software-factory:environment:{spec.get('specSha256')}")
    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": f"urn:uuid:{serial}",
        "version": 1,
        "metadata": {
            "timestamp": spec.get("frozenAt", "1970-01-01T00:00:00Z"),
            "tools": {
                "components": [
                    {
                        "type": "application",
                        "name": "factory-environment-builder",
                        "version": BUILDER_POLICY_REVISION,
                    }
                ]
            },
            "component": {
                "type": "application",
                "bom-ref": f"spec:{spec.get('specSha256')}",
                "name": "factory-environment",
                "version": str(spec.get("specSha256", ""))[:12],
                "properties": [
                    {"name": "factory:spec-sha256", "value": str(spec.get("specSha256", ""))},
                    {"name": "factory:plan-sha256", "value": str(plan.get("planSha256", ""))},
                    {"name": "factory:nix-version", "value": nix_version},
                    {"name": "factory:devenv-version", "value": devenv_version},
                    {"name": "factory:platform", "value": str(spec.get("platform", ""))},
                    {"name": "factory:license-policy-revision", "value": LICENSE_POLICY_REVISION},
                ],
            },
        },
        "components": components,
    }
    return sbom


def sbom_text(sbom: dict[str, Any]) -> str:
    return pretty_json(sbom)


def sbom_sha256(sbom: dict[str, Any]) -> str:
    return sha256_bytes(sbom_text(sbom).encode("utf-8"))


def license_inventory_copy(plan: dict[str, Any]) -> dict[str, Any]:
    """Return exactly the Core-shaped inventory without adding fields."""
    inventory = plan.get("licenseInventory")
    if not isinstance(inventory, dict):
        raise ValueError("the plan has no license inventory")
    return {
        "schemaVersion": inventory.get("schemaVersion", SCHEMA_VERSION),
        "policyRevision": inventory.get("policyRevision", LICENSE_POLICY_REVISION),
        "allowUnfree": False,
        "entries": inventory.get("entries", []),
        "overrides": inventory.get("overrides", []),
        "blockers": inventory.get("blockers", []),
        "inventorySha256": inventory.get("inventorySha256", ""),
        "sbomSha256": inventory.get("sbomSha256", ""),
    }


def inventory_sha256(entries: list[dict[str, Any]]) -> str:
    return sha256_text(canonical_json(entries))
