#!/usr/bin/env python3
"""Validate and inspect provider trajectory cassettes.

A cassette is recorded model/tool traffic used for deterministic replay. It is
not a live provider session and it never grants authority. The validator is
deliberately strict about shape and redaction so a cassette cannot silently
carry credentials or drift into an unreadable transcript.

Replay matches the exact canonical request/response pair. `toolCalls` is
retained as advisory trajectory metadata for human review; it is not replayed as
a tool executor and must never be treated as authority or as proof that a tool
actually ran.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

SCHEMA = "factory-provider-cassette-v1"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
SECRET_KEYS = re.compile(
    r"^(authorization|proxy-authorization|cookie|set-cookie|password|secret|"
    r"credential|api[_-]?key|x[_-]?api[_-]?key|access[_-]?token|"
    r"refresh[_-]?token|id[_-]?token|token|client[_-]?secret|"
    r"aws[_-]?secret[_-]?access[_-]?key|secret[_-]?access[_-]?key|"
    r"private[_-]?key|signing[_-]?key|encryption[_-]?key|"
    r"webhook[_-]?secret|session[_-]?key|master[_-]?key|"
    r"x[_-]?(auth|access|api|github|gitlab|token|key|secret)[a-z0-9_-]*)$",
    re.IGNORECASE,
)
SECRET_VALUES = re.compile(
    r"(bearer\s+[a-z0-9._~+/=-]{12,}|sk-[a-z0-9]{12,}|gh[pousr]_[a-z0-9]{12,}|"
    r"github_pat_[a-z0-9_]{20,}|glpat-[a-z0-9_-]{20,}|xox[baprs]-[a-z0-9-]{10,}|"
    r"AKIA[0-9A-Z]{16})",
    re.IGNORECASE,
)


class CassetteError(ValueError):
    pass


def canonical_request_sha256(request: Any) -> str:
    payload = json.dumps(request, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def _scan_secrets(value: Any, path: str = "") -> list[str]:
    findings: list[str] = []
    if isinstance(value, dict):
        for key, item in value.items():
            child = f"{path}.{key}" if path else str(key)
            if SECRET_KEYS.search(str(key)):
                findings.append(f"secret-shaped key at {child}")
            findings.extend(_scan_secrets(item, child))
    elif isinstance(value, list):
        for index, item in enumerate(value):
            findings.extend(_scan_secrets(item, f"{path}[{index}]"))
    elif isinstance(value, str) and SECRET_VALUES.search(value):
        findings.append(f"secret-shaped value at {path or '<root>'}")
    return findings


def validate(payload: Any) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise CassetteError("cassette root must be an object")
    if payload.get("schema") != SCHEMA:
        raise CassetteError(f"cassette schema must be {SCHEMA}")
    for field in ("provider", "model", "mode", "recordedAt"):
        if not isinstance(payload.get(field), str) or not payload[field].strip():
            raise CassetteError(f"cassette field {field!r} must be a non-empty string")
    turns = payload.get("turns")
    if not isinstance(turns, list) or not turns:
        raise CassetteError("cassette.turns must be a non-empty array")
    if len(turns) > 256:
        raise CassetteError("cassette has more than 256 turns")
    secrets = _scan_secrets(payload)
    if secrets:
        raise CassetteError("cassette contains secret-shaped material: " + "; ".join(secrets))
    indexes: list[int] = []
    for position, turn in enumerate(turns):
        if not isinstance(turn, dict):
            raise CassetteError(f"turn {position} must be an object")
        index = turn.get("index")
        if not isinstance(index, int) or index < 1:
            raise CassetteError(f"turn {position} has an invalid index")
        indexes.append(index)
        digest = turn.get("requestSha256")
        if not isinstance(digest, str) or not SHA256.fullmatch(digest):
            raise CassetteError(f"turn {position} has an invalid requestSha256")
        request = turn.get("request")
        if request is None:
            raise CassetteError(f"turn {position} is missing request")
        if canonical_request_sha256(request) != digest:
            raise CassetteError(f"turn {position} requestSha256 does not match request")
        response = turn.get("response")
        if response is None:
            raise CassetteError(f"turn {position} is missing response")
        tool_calls = turn.get("toolCalls", [])
        if not isinstance(tool_calls, list):
            raise CassetteError(f"turn {position} toolCalls must be an array")
        for call in tool_calls:
            if not isinstance(call, dict) or not isinstance(call.get("name"), str):
                raise CassetteError(f"turn {position} has an invalid tool call")
    expected = list(range(1, len(indexes) + 1))
    if indexes != expected:
        raise CassetteError(f"turn indexes must be sequential from 1: {indexes}")
    return payload


def load(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise CassetteError(f"cassette does not exist: {path}") from error
    except json.JSONDecodeError as error:
        raise CassetteError(f"cassette is not valid JSON: {error}") from error
    return validate(payload)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Validate a provider cassette.")
    parser.add_argument("path", type=Path)
    args = parser.parse_args(argv)
    try:
        cassette = load(args.path)
    except CassetteError as error:
        print(f"INVALID: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "schema": cassette["schema"],
                "provider": cassette["provider"],
                "model": cassette["model"],
                "mode": cassette["mode"],
                "turns": len(cassette["turns"]),
                "valid": True,
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
