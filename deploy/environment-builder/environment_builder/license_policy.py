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
    aliases: tuple[str, ...]


@dataclass(frozen=True)
class LicenseCatalog:
    revision: str
    spdx_license_list_version: str
    sha256: str
    entries: dict[str, CatalogEntry]
    alias_index: dict[str, str]


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


def _profile_sha(document: dict[str, Any]) -> str:
    payload = {
        "mode": document["mode"],
        "familiesConfigured": document["familiesConfigured"],
        "deniedIds": document["deniedIds"],
        "contexts": document["contexts"],
    }
    return hashlib.sha256(_canonical(payload).encode("utf-8")).hexdigest()


def load_catalog(path: Path) -> LicenseCatalog:
    document = json.loads(path.read_text(encoding="utf-8"))
    if type(document.get("schema")) is not int or document.get("schema") != 1:
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
        raw_aliases = raw.get("aliases", [])
        if not isinstance(raw_aliases, list) or not all(
            isinstance(alias, str) and alias for alias in raw_aliases
        ):
            raise RuntimeError(f"licence catalog aliases are invalid for {identifier}")
        entries[identifier] = CatalogEntry(
            id=identifier,
            family=family,
            state=str(raw.get("state", "")),
            spdx=bool(raw.get("spdx", False)),
            obligations=tuple(raw.get("obligations", [])),
            provenance=tuple(raw.get("provenance", [])),
            source_url=str(raw.get("sourceUrl", "")),
            aliases=tuple(raw_aliases),
        )
    alias_index: dict[str, str] = {}
    for identifier, entry in entries.items():
        for name in (identifier, *entry.aliases):
            if name in alias_index:
                raise RuntimeError(f"licence catalog alias {name!r} is duplicated")
            alias_index[name] = identifier
    return LicenseCatalog(
        revision=str(document.get("revision", "")),
        spdx_license_list_version=str(document.get("spdxLicenseListVersion", "")),
        sha256=str(document["sha256"]),
        entries=entries,
        alias_index=alias_index,
    )


_PROFILE_MODES = {"legacy-current", "personal"}
_PROFILE_CONTEXTS = {"selected", "closure", "build", "devTest", "runtimeDistributed"}
_PROFILE_FAMILIES = {item.value for item in LicenseFamily}


def _require_profile_field(document: dict[str, Any], key: str) -> Any:
    if key not in document:
        raise RuntimeError(f"licence profile is missing {key}")
    return document[key]


def load_profile(path: Path) -> LicenseProfile:
    document = json.loads(path.read_text(encoding="utf-8"))
    if type(document.get("schema")) is not int or document.get("schema") != 1:
        raise RuntimeError("unsupported licence profile schema")
    mode = _require_profile_field(document, "mode")
    families = _require_profile_field(document, "familiesConfigured")
    denied = _require_profile_field(document, "deniedIds")
    contexts = _require_profile_field(document, "contexts")
    if not isinstance(mode, str) or mode not in _PROFILE_MODES:
        raise RuntimeError("invalid licence profile mode")
    if type(families) is not bool:
        raise RuntimeError("licence profile familiesConfigured must be boolean")
    if not isinstance(denied, list) or not all(
        isinstance(item, str) and item for item in denied
    ):
        raise RuntimeError("invalid licence profile deniedIds")
    if not isinstance(contexts, dict) or not contexts:
        raise RuntimeError("licence profile contexts are missing")
    for name, config in contexts.items():
        if name not in _PROFILE_CONTEXTS or not isinstance(config, dict):
            raise RuntimeError(f"invalid licence profile context {name!r}")
        for key in ("allowedIds", "acceptedFamilies", "obligations"):
            values = config.get(key, [])
            if not isinstance(values, list) or not all(
                isinstance(item, str) and item for item in values
            ):
                raise RuntimeError(f"invalid licence profile {name}.{key}")
        for family in config.get("acceptedFamilies", []):
            if family not in _PROFILE_FAMILIES:
                raise RuntimeError(f"unknown licence family {family!r}")
        if "inherits" in config and (
            not isinstance(config["inherits"], str)
            or config["inherits"] not in _PROFILE_CONTEXTS
        ):
            raise RuntimeError(f"invalid licence profile inheritance in {name}")
        if "evaluated" in config and type(config["evaluated"]) is not bool:
            raise RuntimeError(f"licence profile {name}.evaluated must be boolean")
    if _profile_sha(document) != document.get("sha256"):
        raise RuntimeError("licence profile hash is inconsistent")
    return LicenseProfile(
        revision=str(document.get("revision", "")),
        mode=mode,
        families_configured=families,
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
            "aliases": list(entry.aliases),
        }
        for entry in sorted(catalog.entries.values(), key=lambda item: item.id)
    ]
    return _catalog_sha(entries)


def profile_content_sha256(profile: LicenseProfile) -> str:
    """Canonical hash of the loaded context policy content."""
    return _profile_sha(
        {
            "mode": profile.mode,
            "familiesConfigured": profile.families_configured,
            "deniedIds": sorted(profile.denied_ids),
            "contexts": profile.contexts,
        }
    )


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


def _tokenize(expression: str) -> list[str]:
    tokens: list[str] = []
    current: list[str] = []
    for character in expression:
        if character in "()":
            if current:
                tokens.append("".join(current))
                current = []
            tokens.append(character)
        elif character.isspace():
            if current:
                tokens.append("".join(current))
                current = []
        else:
            current.append(character)
    if current:
        tokens.append("".join(current))
    return tokens


class _ExpressionParser:
    def __init__(self, value: str) -> None:
        self.tokens = _tokenize(value)
        self.position = 0

    def peek(self) -> str | None:
        return self.tokens[self.position] if self.position < len(self.tokens) else None

    def take(self) -> str | None:
        token = self.peek()
        if token is not None:
            self.position += 1
        return token

    def parse(self) -> tuple:
        expression = self.parse_or()
        if self.position != len(self.tokens):
            raise ValueError("trailing licence expression tokens")
        return expression

    def parse_or(self) -> tuple:
        branches = [self.parse_and()]
        while self.peek() == "OR":
            self.position += 1
            branches.append(self.parse_and())
        return branches[0] if len(branches) == 1 else ("or", branches)

    def parse_and(self) -> tuple:
        factors = [self.parse_atom()]
        while True:
            token = self.peek()
            if token == "AND":
                self.position += 1
                factors.append(self.parse_atom())
            elif token == "WITH":
                self.position += 1
                # A WITH exception is a conjunction, never an ignored token.
                factors.append(self.parse_atom())
            elif token in (None, "OR", ")"):
                break
            else:
                # Adjacent SPDX atoms are implicitly AND.
                factors.append(self.parse_atom())
        return factors[0] if len(factors) == 1 else ("and", factors)

    def parse_atom(self) -> tuple:
        token = self.take()
        if token == "(":
            expression = self.parse_or()
            if self.take() != ")":
                raise ValueError("unbalanced licence expression parentheses")
            return expression
        if token in (None, ")", "AND", "OR"):
            raise ValueError("invalid licence expression atom")
        return ("atom", token)


def _atom_decision(
    atom: str,
    catalog: LicenseCatalog,
    profile: LicenseProfile,
    config: dict[str, Any],
) -> LicenseDecision:
    if atom == "LLVM-exception":
        return LicenseDecision.ALLOWED
    if atom.endswith("-exception") or atom.endswith("-exception-2.0"):
        # Unrecognised SPDX exceptions are not silently ignored.
        return LicenseDecision.UNKNOWN
    identifier = catalog.alias_index.get(atom, atom)
    if identifier in profile.denied_ids:
        return LicenseDecision.DENIED
    if identifier in set(config.get("allowedIds", [])):
        return LicenseDecision.ALLOWED
    entry = catalog.entries.get(identifier)
    if entry is None or entry.state != "active":
        return LicenseDecision.UNKNOWN
    if entry.family in set(config.get("acceptedFamilies", [])):
        return LicenseDecision.ALLOWED
    return LicenseDecision.UNKNOWN


def _evaluate_expression(
    expression: tuple,
    catalog: LicenseCatalog,
    profile: LicenseProfile,
    config: dict[str, Any],
) -> LicenseDecision:
    kind = expression[0]
    if kind == "atom":
        return _atom_decision(expression[1], catalog, profile, config)
    if kind == "and":
        unknown = False
        not_evaluated = False
        for factor in expression[1]:
            decision = _evaluate_expression(factor, catalog, profile, config)
            if decision is LicenseDecision.DENIED:
                return LicenseDecision.DENIED
            if decision is LicenseDecision.UNKNOWN:
                unknown = True
            elif decision is LicenseDecision.NOT_EVALUATED:
                not_evaluated = True
        if unknown:
            return LicenseDecision.UNKNOWN
        if not_evaluated:
            return LicenseDecision.NOT_EVALUATED
        return LicenseDecision.ALLOWED
    if kind == "or":
        denied = False
        unknown = False
        not_evaluated = False
        for branch in expression[1]:
            decision = _evaluate_expression(branch, catalog, profile, config)
            if decision is LicenseDecision.ALLOWED:
                return LicenseDecision.ALLOWED
            if decision is LicenseDecision.DENIED:
                denied = True
            elif decision is LicenseDecision.UNKNOWN:
                unknown = True
            else:
                not_evaluated = True
        if denied:
            return LicenseDecision.DENIED
        if unknown:
            return LicenseDecision.UNKNOWN
        if not_evaluated:
            return LicenseDecision.NOT_EVALUATED
        return LicenseDecision.UNKNOWN
    return LicenseDecision.UNKNOWN


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
    if value in catalog.alias_index:
        return _atom_decision(value, catalog, profile, config)
    try:
        parsed = _ExpressionParser(value).parse()
    except ValueError:
        return LicenseDecision.UNKNOWN
    return _evaluate_expression(parsed, catalog, profile, config)


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
