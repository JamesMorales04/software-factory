"""Stable typed builder failures.

The codes mirror ``EnvironmentBlockerCode`` in
``native/src/planning/environment.rs``. They are API values; do not publish a
new code without updating the Core enum and the contract.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Final

from .util import redact

#: Stable codes accepted by Core. Keep this list in sync with
#: ``EnvironmentBlockerCode::as_str``.
CODES: Final[frozenset[str]] = frozenset(
    {
        "ENVIRONMENT_SPEC_INVALID",
        "ENVIRONMENT_BUILD_FAILED",
        "ENVIRONMENT_BUILDER_UNAVAILABLE",
        "ENVIRONMENT_LICENSE_BLOCKED",
        "ENVIRONMENT_PLAN_REQUIRED",
        "ENVIRONMENT_CONVENTIONS_MISSING",
        "ENVIRONMENT_CONVENTIONS_CONFLICT",
        "ENVIRONMENT_CONVENTIONS_STALE",
        "ENVIRONMENT_PLAN_INVALID",
        "ENVIRONMENT_UNSUPPORTED",
        "ENVIRONMENT_PIN_MISSING",
        "ENVIRONMENT_LICENSE_DENIED",
        "ENVIRONMENT_LICENSE_UNKNOWN",
        "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
        "ENVIRONMENT_PAID_SERVICE_REQUIRED",
        "ENVIRONMENT_LEAD_APPROVAL_REQUIRED",
        "ENVIRONMENT_OWNER_APPROVAL_REQUIRED",
        "ENVIRONMENT_SECRET_DETECTED",
        "ENVIRONMENT_SANDBOX_DENIED",
        "ENVIRONMENT_NETWORK_DENIED",
        "ENVIRONMENT_RESOURCE_LIMIT",
        "ENVIRONMENT_BUILD_TIMEOUT",
        "ENVIRONMENT_HASH_MISMATCH",
        "ENVIRONMENT_NOT_FROZEN",
        "ENVIRONMENT_SUPERSEDED",
    }
)

MAX_MESSAGE_CHARS: Final[int] = 1_000


@dataclass
class BuilderError(Exception):
    """A fail-closed builder result with a stable Core blocker code."""

    code: str
    message: str
    exit_code: int = 2

    def __post_init__(self) -> None:
        if self.code not in CODES:
            raise ValueError(f"unknown builder blocker code: {self.code}")
        Exception.__init__(self, redact(self.message)[:MAX_MESSAGE_CHARS])
        self.message = redact(self.message)[:MAX_MESSAGE_CHARS]

    def blocker(self) -> dict[str, str]:
        return {"code": self.code, "message": self.message}


def fail(code: str, message: str) -> None:
    raise BuilderError(code, message)
