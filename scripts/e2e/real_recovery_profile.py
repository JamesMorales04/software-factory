"""Canonical execution-profile contract for the real generation-2 recovery run.

The profile created by the operator-lane pre-run binding and the profile the
scenario consumes must be the same object. This module is the single source of
the execution-relevant fields; it mirrors the canonical deterministic profile
in ``scripts/e2e/compose_intake.py`` (routes, technical operation limits and
deterministic progress supervision) and deliberately carries no turns,
attempts, cumulative tokens or monetary cap.

The helpers are pure: no network, no filesystem, no environment access.
"""

from __future__ import annotations

from typing import Any

CATEGORY = "BALANCED"
REASONING = "medium"

OPERATIONS: dict[str, int] = {
    "modelRequestTimeoutSecs": 300,
    "toolCallTimeoutSecs": 600,
    "maxRequestBytes": 2_097_152,
    "maxResponseBytes": 2_097_152,
    "maxGenerationTokens": 32_768,
    "maxInFlightOperations": 2,
}

PROGRESS: dict[str, int] = {
    "diagnoseAfterStagnantCheckpoints": 2,
    "escalateAfterUnsuccessfulReplans": 1,
    "observationTimeoutSecs": 300,
}

ROLES = ("lead", "implementer", "semanticReviewer")
ROLE_FIELDS = ("provider", "model", "reasoning")


def canonical_profile(
    *,
    name: str,
    description: str,
    provider: str,
    model: str,
) -> dict[str, Any]:
    """Build the POST /execution-profiles payload accepted by Core.

    ``auto_start`` is the snake_case request field; Core serializes it as
    ``autoStart`` with ``riskClasses`` in the projection.
    """

    role = {"provider": provider, "model": model, "reasoning": REASONING}
    return {
        "name": name,
        "description": description,
        "category": CATEGORY,
        "roles": {role_name: dict(role) for role_name in ROLES},
        "operations": dict(OPERATIONS),
        "progress": dict(PROGRESS),
        "auto_start": {"enabled": False, "risk_classes": []},
    }


def _observed_auto_start(profile: dict[str, Any]) -> dict[str, Any]:
    auto = profile.get("autoStart")
    if auto is None:
        auto = profile.get("auto_start") or {}
    risk = auto.get("riskClasses")
    if risk is None:
        risk = auto.get("risk_classes")
    return {"enabled": auto.get("enabled"), "riskClasses": risk}


def _observed_roles(profile: dict[str, Any]) -> dict[str, Any]:
    roles = profile.get("roles") or {}
    observed: dict[str, Any] = {}
    for role_name in ROLES:
        route = roles.get(role_name) or {}
        observed[role_name] = {field: route.get(field) for field in ROLE_FIELDS}
    return observed


def _observed_operations(profile: dict[str, Any]) -> dict[str, Any]:
    operations = profile.get("operations") or {}
    return {key: operations.get(key) for key in OPERATIONS}


def _observed_progress(profile: dict[str, Any]) -> dict[str, Any]:
    progress = profile.get("progress") or {}
    return {key: progress.get(key) for key in PROGRESS}


def expected_contract(*, provider: str, model: str) -> dict[str, Any]:
    """The execution-relevant contract in the projection's camelCase shape."""

    return {
        "category": CATEGORY,
        "roles": _observed_roles(
            canonical_profile(
                name="",
                description="",
                provider=provider,
                model=model,
            )
        ),
        "operations": dict(OPERATIONS),
        "progress": dict(PROGRESS),
        "autoStart": {"enabled": False, "riskClasses": []},
    }


def observed_contract(profile: dict[str, Any]) -> dict[str, Any]:
    """Normalize GET /execution-profiles/{id} to the comparable contract."""

    return {
        "category": profile.get("category"),
        "roles": _observed_roles(profile),
        "operations": _observed_operations(profile),
        "progress": _observed_progress(profile),
        "autoStart": _observed_auto_start(profile),
    }


def profile_mismatches(
    profile: dict[str, Any], *, provider: str, model: str
) -> list[str]:
    """Return the execution-relevant fields that differ from the canonical one.

    ``executable`` must be true: a profile whose implementer route is not
    configured is not admissible. Name and description are display-only and are
    not part of the contract.
    """

    expected = expected_contract(provider=provider, model=model)
    observed = observed_contract(profile)
    problems = [
        field for field in ("category", "roles", "operations", "progress", "autoStart")
        if observed.get(field) != expected[field]
    ]
    if profile.get("executable") is not True:
        problems.append("executable")
    return problems
