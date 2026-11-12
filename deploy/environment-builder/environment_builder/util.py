"""Small, dependency-free helpers shared by the builder modules."""

from __future__ import annotations

import hashlib
import json
import re
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable

SECRET_PATTERNS = (
    re.compile(r"AKIA[0-9A-Z]{16}"),
    re.compile(r"gh[pousr]_[A-Za-z0-9_]{20,}"),
    re.compile(r"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(r"sk-[A-Za-z0-9]{20,}"),
    re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    re.compile(r"(?i)(password|secret|token)\s*=\s*['\"]?[A-Za-z0-9_\-]{12,}"),
)

REDACTED = "[REDACTED]"


def redact(value: str) -> str:
    """Remove secret-shaped values from diagnostics and logs."""
    result = value
    for pattern in SECRET_PATTERNS:
        result = pattern.sub(REDACTED, result)
    return result


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_text(value: str) -> str:
    return sha256_bytes(value.encode("utf-8"))


def canonical_json(value: Any) -> str:
    """Compact, deterministic JSON used for content hashes."""
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
        allow_nan=False,
    )


def pretty_json(value: Any) -> str:
    return json.dumps(
        value,
        ensure_ascii=False,
        indent=2,
        sort_keys=True,
        allow_nan=False,
    ) + "\n"


def read_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, value: Any, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(pretty_json(value), encoding="utf-8")
    path.chmod(mode)


def now_rfc3339() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def receipt_id() -> str:
    return str(uuid.uuid4())


def is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def is_digest(value: Any) -> bool:
    return (
        isinstance(value, str)
        and value.startswith("sha256:")
        and len(value) == 71
        and all(character in "0123456789abcdef" for character in value[7:])
    )


def is_git_revision(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) in (40, 64)
        and all(character in "0123456789abcdef" for character in value)
    )


def bounded_text(value: Any, limit: int = 2_000, *, allow_empty: bool = False) -> str:
    if not isinstance(value, str):
        raise ValueError("expected a string")
    text = value.strip()
    if not allow_empty and not text:
        raise ValueError("expected a non-empty string")
    if len(text) > limit:
        raise ValueError(f"string exceeds {limit} characters")
    if any(ord(character) < 0x20 and character not in "\n\t" for character in text):
        raise ValueError("string contains control characters")
    return text


def safe_relative_path(value: Any, *, limit: int = 512) -> str:
    """Return a normalized relative path or fail without touching the host."""
    text = bounded_text(value, limit)
    path = Path(text)
    if path.is_absolute():
        raise ValueError("path must be relative")
    parts = path.parts
    if not parts or any(part in ("", ".", "..") for part in parts):
        raise ValueError("path must not contain empty, '.' or '..' components")
    if any(part.startswith(".") and part not in (".nvmrc", ".python-version", ".tool-versions") for part in parts):
        raise ValueError("hidden path components are not accepted in environment files")
    normalized = "/".join(parts)
    if normalized != text:
        raise ValueError("path is not normalized")
    return normalized


def symlink_stays_inside(root: Path) -> None:
    """Reject symlinks that escape the copied project workspace."""
    root = root.resolve()
    for entry in root.rglob("*"):
        if not entry.is_symlink():
            continue
        target = entry.resolve()
        try:
            target.relative_to(root)
        except ValueError as error:
            raise ValueError(f"symlink {entry} escapes the workspace") from error


def tail(value: str, limit: int = 16_384) -> str:
    value = redact(value)
    if len(value) <= limit:
        return value
    return "…" + value[-limit:]


def last_lines(value: str, lines: int = 60, limit: int = 8_192) -> str:
    redacted = redact(value)
    selected = "\n".join(redacted.splitlines()[-lines:])
    return tail(selected, limit)


def last_nonempty_line(value: str, limit: int = 512) -> str:
    """Return one redacted, bounded stderr line for a check diagnostic.

    Never return more than a single line: a failed check may print an
    unbounded body that the broker must not copy into a tool result.
    """
    for line in reversed(redact(value).splitlines()):
        line = line.strip()
        if line:
            return line[:limit]
    return ""


def unique(values: Iterable[str]) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        result.append(value)
    return result
