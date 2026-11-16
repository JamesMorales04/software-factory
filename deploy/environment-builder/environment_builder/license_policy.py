"""Declarative licence catalog and context policy for the environment builder.

Two artifacts, deliberately separate:

* ``license_catalog.json`` classifies each known SPDX/identifier atom
  (family, state, obligations, provenance). Classification is **not**
  acceptance.
* ``license_profile.json`` is the Owner/versioned verdict per context
  (selected, closure, build, devTest, runtimeDistributed). The transitional
  ``legacy-current`` profile reproduces the pre-existing verdicts exactly.

The evaluator is deterministic and never consumes model output. It preserves
the current implicit-AND semantics: every atom of a compound expression must be
allowed; no ``OR``/``WITH`` parser is introduced here.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any


class LicenseFamily(str, Enum):
    PERMISSIVE = "permissive"
    WEAK_COPYLEFT = "weak-copyleft"
    STRONG_COPYLEFT = "strong-copyleft"
    NETWORK_COPYLEFT = "network-copyleft"
    SOURCE_AVAILABLE = "source-available"
    PUBLIC_DOMAIN = "public-domain"


class LicenseContext(str, Enum):
    SELECTED = "selected"
    CLOSURE = "closure"
    BUILD = "build"
    DEV_TEST = "devTest"
    RUNTIME_DISTRIBUTED = "runtimeDistributed"


class LicenseDecision(str, Enum):
    ALLOWED = "allowed"
    DENIED = "denied"
    UNKNOWN = "unknown"
    NOT_EVALUATED = "notEvaluated"


_UNKNOWN_VALUES = {"NONE", "NOASSERTION", "UNLICENSED", "UNKNOWN"}


@dataclass(frozen=True)
class CatalogEntry:
    id: str
    family: str
    state: str
    spdx: bool
    obligations: tuple[str, ...]
    provenance: tuple[str, ...]
    source_url: str


@dataclass(frozen=True)
class LicenseCatalog:
    revision: str
    spdx_license_list_version: str
    sha256: str
    entries: dict[str, CatalogEntry]


@dataclass(frozen=True)
class LicenseProfile:
    revision: str
    mode: str
    families_configured: bool
    sha256: str
    denied_ids: frozenset[str]
    contexts: dict[str, dict[str, Any]]


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n"


def _catalog_sha(entries: list[dict[str, Any]]) -> str:
    return hashlib.sha256(_canonical(entries).encode("utf-8")).hexdigest()


def _profile_sha(denied_ids: list[str], contexts: dict[str, Any]) -> str:
    return hashlib.sha256(
        _canonical({"deniedIds": denied_ids, "contexts": contexts}).encode("utf-8")
    ).hexdigest()


def load_catalog(path: Path) -> LicenseCatalog:
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != 1:
        raise RuntimeError("unsupported licence catalog schema")
    raw_entries = document.get("entries")
    if not isinstance(raw_entries, list) or not raw_entries:
        raise RuntimeError("licence catalog entries are missing")
    if _catalog_sha(raw_entries) != document.get("sha256"):
        raise RuntimeError("licence catalog hash is inconsistent")
    entries: dict[str, CatalogEntry] = {}
    for raw in raw_entries:
        identifier = raw.get("id")
        if not isinstance(identifier, str) or not identifier or identifier in entries:
            raise RuntimeError("licence catalog ids are invalid or duplicated")
        family = raw.get("family")
        if family not in {item.value for item in LicenseFamily}:
            raise RuntimeError(f"licence catalog family is invalid for {identifier}")
        entries[identifier] = CatalogEntry(
            id=identifier,
            family=family,
            state=str(raw.get("state", "")),
            spdx=bool(raw.get("spdx", False)),
            obligations=tuple(raw.get("obligations", [])),
            provenance=tuple(raw.get("provenance", [])),
            source_url=str(raw.get("sourceUrl", "")),
        )
    return LicenseCatalog(
        revision=str(document.get("revision", "")),
        spdx_license_list_version=str(document.get("spdxLicenseListVersion", "")),
        sha256=str(document["sha256"]),
        entries=entries,
    )


def load_profile(path: Path) -> LicenseProfile:
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("schema") != 1:
        raise RuntimeError("unsupported licence profile schema")
    denied = document.get("deniedIds")
    contexts = document.get("contexts")
    if not isinstance(denied, list) or not isinstance(contexts, dict):
        raise RuntimeError("licence profile is missing deniedIds/contexts")
    if _profile_sha(denied, contexts) != document.get("sha256"):
        raise RuntimeError("licence profile hash is inconsistent")
    return LicenseProfile(
        revision=str(document.get("revision", "")),
        mode=str(document.get("mode", "")),
        families_configured=bool(document.get("familiesConfigured", False)),
        sha256=str(document["sha256"]),
        denied_ids=frozenset(denied),
        contexts=contexts,
    )


def load_default() -> tuple[LicenseCatalog, LicenseProfile]:
    base = Path(__file__).resolve().parent
    return (
        load_catalog(base / "license_catalog.json"),
        load_profile(base / "license_profile.json"),
    )


def catalog_content_sha256(catalog: LicenseCatalog) -> str:
    """Canonical hash of the loaded classification content."""
    entries = [
        {
            "id": entry.id,
            "family": entry.family,
            "state": entry.state,
            "spdx": entry.spdx,
            "obligations": list(entry.obligations),
            "provenance": list(entry.provenance),
            "sourceUrl": entry.source_url,
        }
        for entry in sorted(catalog.entries.values(), key=lambda item: item.id)
    ]
    return _catalog_sha(entries)


def profile_content_sha256(profile: LicenseProfile) -> str:
    """Canonical hash of the loaded context policy content."""
    return _profile_sha(sorted(profile.denied_ids), profile.contexts)


def binding_sha256(catalog: LicenseCatalog, profile: LicenseProfile) -> str:
    """Content binding for ``LicenseInventory``; labels are not identity."""
    payload = _canonical(
        {
            "catalogSha256": catalog_content_sha256(catalog),
            "profileSha256": profile_content_sha256(profile),
        }
    )
    return hashlib.sha256(payload.encode("utf-8")).hexdigest()


def b_active(profile: LicenseProfile) -> bool:
    """B is inactive until a personal profile declares its families."""
    return profile.mode != "legacy-current" and profile.families_configured


def _resolve_context(
    context: LicenseContext, profile: LicenseProfile
) -> dict[str, Any]:
    config = profile.contexts.get(context.value)
    if config is None:
        raise RuntimeError(f"licence profile has no context {context.value!r}")
    if "inherits" in config:
        parent = LicenseContext(config["inherits"])
        return _resolve_context(parent, profile)
    return config


def _atoms(expression: str) -> list[str]:
    normalized = (
        expression.replace("(", " ")
        .replace(")", " ")
        .replace(" AND ", " ")
        .replace(" OR ", " ")
        .replace(" WITH ", " ")
    )
    return normalized.split()


def evaluate(
    expression: str,
    context: LicenseContext,
    catalog: LicenseCatalog,
    profile: LicenseProfile,
) -> LicenseDecision:
    """Deterministic verdict for one context. No model input is accepted."""
    if not isinstance(expression, str) or not expression.strip():
        return LicenseDecision.UNKNOWN
    value = expression.strip()
    if value in _UNKNOWN_VALUES:
        return LicenseDecision.UNKNOWN
    config = _resolve_context(context, profile)
    if config.get("evaluated") is False:
        return LicenseDecision.NOT_EVALUATED
    if value in profile.denied_ids:
        return LicenseDecision.DENIED
    allowed_ids = set(config.get("allowedIds", []))
    accepted_families = set(config.get("acceptedFamilies", []))
    if value in allowed_ids:
        return LicenseDecision.ALLOWED
    atoms = _atoms(value)
    if not atoms:
        return LicenseDecision.UNKNOWN
    for atom in atoms:
        if atom.startswith("LLVM-exception"):
            continue
        if atom in allowed_ids:
            continue
        entry = catalog.entries.get(atom)
        if entry is None or entry.state != "active":
            return LicenseDecision.UNKNOWN
        if accepted_families and entry.family in accepted_families:
            continue
        return LicenseDecision.UNKNOWN
    return LicenseDecision.ALLOWED


def verdict_matrix(
    catalog: LicenseCatalog, profile: LicenseProfile
) -> dict[str, dict[str, LicenseDecision]]:
    """Full known-ID matrix for contract tests and review."""
    return {
        context.value: {
            identifier: evaluate(identifier, context, catalog, profile)
            for identifier in sorted(catalog.entries)
        }
        for context in LicenseContext
    }
