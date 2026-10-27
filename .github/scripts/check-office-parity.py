#!/usr/bin/env python3
"""Validate the Office feature-parity matrix against real repository artifacts.

This is a shape and reference check, not proof that a UI workflow exists or
that a browser test passed. An independent reviewer remains responsible for the
semantic claim and the real acceptance evidence.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

CAPABILITY = re.compile(r"^- id: (CAP-\d+)$", re.MULTILINE)
API = re.compile(r"^(GET|POST|PUT|DELETE) /")
PLACEHOLDERS = ("TODO", "TBD", "FIXME", "...", "<", ">")


def fail(errors: list[str], message: str) -> None:
    errors.append(message)


def office_routes(root: Path) -> set[str]:
    path = root / "office/src/App.tsx"
    if not path.is_file():
        return set()
    return set(re.findall(r'path="([^"]+)"', path.read_text(encoding="utf-8")))


def core_api_paths(root: Path) -> set[str]:
    path = root / "native/src/bin/core.rs"
    if not path.is_file():
        return set()
    return set(re.findall(r'\.route\(\s*"([^"]+)"', path.read_text(encoding="utf-8"), re.DOTALL))


def acceptance_has_browser_provenance(root: Path, items: list[str]) -> bool:
    for item in items:
        path = root / item
        if not path.is_file():
            continue
        try:
            content = path.read_text(encoding="utf-8").lower()
        except OSError:
            continue
        if "playwright" in content or "chromium" in content:
            return True
    return False


def text_list(entry: dict, field: str, errors: list[str], name: str) -> list[str]:
    value = entry.get(field)
    if not isinstance(value, list) or not value:
        fail(errors, f"{name}: {field} must be a non-empty list")
        return []
    output = []
    for item in value:
        if not isinstance(item, str) or item.strip() == "":
            fail(errors, f"{name}: {field} contains an empty value")
            continue
        if any(marker in item.upper() for marker in PLACEHOLDERS):
            fail(errors, f"{name}: {field} contains a placeholder")
            continue
        output.append(item.strip())
    return output


def validate(root: Path) -> list[str]:
    errors: list[str] = []
    parity_path = root / "docs/product/office-parity.json"
    registry_path = root / "docs/product/capabilities.yaml"
    try:
        parity = json.loads(parity_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return [f"cannot read {parity_path}: {exc}"]
    try:
        registry = registry_path.read_text(encoding="utf-8")
    except OSError as exc:
        return [f"cannot read {registry_path}: {exc}"]
    capability_ids = set(CAPABILITY.findall(registry))
    if parity.get("schema_version") != 1:
        fail(errors, "office-parity schema_version must be 1")
    capabilities = parity.get("capabilities")
    if not isinstance(capabilities, dict) or not capabilities:
        return errors + ["office-parity capabilities must be a non-empty object"]

    for capability_id, entry in capabilities.items():
        name = f"office-parity {capability_id}"
        if capability_id not in capability_ids:
            fail(errors, f"{name}: unknown capability id")
        if not isinstance(entry, dict):
            fail(errors, f"{name}: entry must be an object")
            continue
        if not isinstance(entry.get("user_facing"), bool):
            fail(errors, f"{name}: user_facing must be a boolean")
            continue
        if entry["user_facing"] is not True:
            exemption = entry.get("exemption")
            if not isinstance(exemption, str) or exemption.strip() == "":
                fail(errors, f"{name}: user_facing false requires a documented exemption")
            continue
        for required in ("name", "job", "notes"):
            value = entry.get(required)
            if not isinstance(value, str) or value.strip() == "":
                fail(errors, f"{name}: {required} is required")
        routes = text_list(entry, "routes", errors, name)
        known_routes = office_routes(root)
        for route in routes:
            if not route.startswith("/"):
                fail(errors, f"{name}: route {route!r} must start with /")
            elif route not in known_routes:
                fail(errors, f"{name}: route {route!r} is not declared in office/src/App.tsx")
        apis = text_list(entry, "core_api", errors, name)
        known_apis = core_api_paths(root)
        for api in apis:
            match = API.match(api)
            if match is None:
                fail(errors, f"{name}: API {api!r} must be METHOD /path")
                continue
            path = api.split(" ", 1)[1]
            if path not in known_apis:
                fail(errors, f"{name}: API path {path!r} is not declared in native/src/bin/core.rs")
        text_list(entry, "user_actions", errors, name)
        text_list(entry, "states", errors, name)
        acceptance = text_list(entry, "acceptance", errors, name)
        for path in acceptance:
            if not (root / path).exists():
                fail(errors, f"{name}: acceptance path is missing: {path}")
        if acceptance and not acceptance_has_browser_provenance(root, acceptance):
            fail(errors, f"{name}: acceptance must include browser-test provenance (playwright/chromium)")

    deferred = parity.get("deferred")
    if not isinstance(deferred, list):
        fail(errors, "office-parity deferred must be a list")
    else:
        for index, item in enumerate(deferred):
            if not isinstance(item, dict):
                fail(errors, f"office-parity deferred[{index}] must be an object")
                continue
            for field in ("module", "planned_slice", "reason"):
                value = item.get(field)
                if not isinstance(value, str) or value.strip() == "":
                    fail(errors, f"office-parity deferred[{index}] missing {field}")
                elif any(marker in value.upper() for marker in PLACEHOLDERS):
                    fail(errors, f"office-parity deferred[{index}] has a placeholder")
    return errors


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".", help="repository root")
    args = parser.parse_args(argv)
    errors = validate(Path(args.root).resolve())
    if errors:
        for error in errors:
            print(f"office-parity: {error}", file=sys.stderr)
        return 1
    print("office-parity: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
