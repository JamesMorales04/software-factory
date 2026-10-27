#!/usr/bin/env python3
"""Require an explicit impact assessment for protected pull request changes.

The assessment is carried by the PR description, while this check binds its
result to the exact base/head comparison. It is intentionally a routing check;
an independent reviewer remains responsible for semantic accuracy.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path


START = "<!-- architecture-impact:start -->"
END = "<!-- architecture-impact:end -->"
OFFICE_START = "<!-- office-impact:start -->"
OFFICE_END = "<!-- office-impact:end -->"
CONTAINER_START = "<!-- container-native-impact:start -->"
CONTAINER_END = "<!-- container-native-impact:end -->"
ALLOWED_CATEGORIES = {
    "architecture",
    "authorization",
    "security",
    "quality",
    "evidence",
    "delivery",
    "ci",
    "runtime",
    "skills",
    "governance",
}
MIN_RATIONALE_LENGTH = 40
FULL_SHA = re.compile(r"^[0-9a-fA-F]{40}$")
PLACEHOLDER_TOKENS = ("TODO", "TBD", "FIXME", "...", "<", ">")
OFFICE_EXACT = {
    "docs/product/factory-office.md",
    "docs/product/office-parity.json",
    "docs/product/office-design-system.md",
}
CONTAINER_EXACT = {
    "compose.yaml",
    "compose.e2e.yaml",
    "compose.dev.yaml",
    "Cargo.toml",
    "Cargo.lock",
    "native/Cargo.toml",
    "office/package.json",
    "office/package-lock.json",
    "office/e2e-browser/package.json",
    "office/e2e-browser/package-lock.json",
    ".github/workflows/native-rust.yml",
    ".github/workflows/compose-e2e.yml",
    ".github/workflows/native-rust-podman.yml",
    ".github/workflows/native-rust-provider-compat.yml",
}


def is_protected(path: str) -> bool:
    """Classify changes that can alter product or engineering guarantees."""
    path = path.removeprefix("./")
    prefixes = (
        ".agents/",
        ".github/",
        "native/",
        "migrations/",
        "docs/architecture/",
        "docs/contracts/",
        "docs/engineering/",
        "docs/implementation/",
        "docs/observability/",
        "docs/product/",
        "docs/quality/",
        "docs/roadmap/",
        "docs/security/",
        "docs/evidence/",
        "scripts/",
    )
    exact = {
        "AGENTS.md",
        "README.md",
        "docs/README.md",
        "docs/product/capabilities.yaml",
        "docs/roadmap/ROADMAP.md",
    }
    return path in exact or path.startswith(prefixes)


def is_office(path: str) -> bool:
    path = path.removeprefix("./")
    return (
        path.startswith("office/")
        or path in OFFICE_EXACT
        or path.startswith("scripts/compose-office")
        or path.startswith("scripts/compose-onboarding")
        or path.startswith("scripts/compose-provider")
    )


def is_container_native(path: str) -> bool:
    path = path.removeprefix("./")
    return (
        path in CONTAINER_EXACT
        or path.startswith("compose.")
        and path.endswith((".yaml", ".yml"))
        or path.startswith("deploy/")
        or path.startswith(".devcontainer/")
        or path.startswith("native/src/bin/")
        or path.startswith("native/src/provider_")
        or path in {
            "native/src/execution.rs",
            "native/src/runtime_broker.rs",
            "native/src/sandbox.rs",
            "native/src/service_network.rs",
            "native/src/workspace.rs",
            "native/src/workspace_archive.rs",
            "native/src/db/provider_sessions.rs",
            "native/src/db/provider_settings.rs",
            "native/src/db/workers.rs",
        }
        or path.startswith("scripts/compose-")
    )


def changed_paths(base: str, head: str) -> list[str]:
    if not FULL_SHA.fullmatch(base) or not FULL_SHA.fullmatch(head):
        raise ValueError("pull request base/head must be full 40-character commit SHAs")
    result = subprocess.run(
        ["git", "diff", "--name-only", "--diff-filter=ACDMRTUXB", f"{base}...{head}"],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return sorted(path for path in result.stdout.splitlines() if path)


def declaration_section(body: str) -> str | None:
    if body.count(START) != 1 or body.count(END) != 1:
        return None
    after_start = body.split(START, 1)[1]
    if END not in after_start:
        return None
    return after_start.split(END, 1)[0].strip()


def assessment_from_body(
    body: str, expected_base: str, expected_head: str
) -> tuple[str, str] | None:
    section = declaration_section(body)
    if section is None or not FULL_SHA.fullmatch(expected_base) or not FULL_SHA.fullmatch(expected_head):
        return None
    fields: dict[str, str] = {}
    for line in section.splitlines():
        if not line.strip() or ":" not in line:
            continue
        key, value = line.split(":", 1)
        key = key.strip().lower()
        if key in fields:
            return None
        fields[key] = value.strip()
    category = fields.get("category", "").lower()
    rationale = fields.get("rationale", "")
    if set(fields) != {"category", "rationale", "base_sha", "head_sha"}:
        return None
    if category not in ALLOWED_CATEGORIES:
        return None
    if len(rationale) < MIN_RATIONALE_LENGTH:
        return None
    if "TODO" in rationale.upper() or "TBD" in rationale.upper():
        return None
    if fields["base_sha"].lower() != expected_base.lower():
        return None
    if fields["head_sha"].lower() != expected_head.lower():
        return None
    return category, rationale


def declared_fields(body: str, start: str, end: str) -> dict[str, str] | None:
    if body.count(start) != 1 or body.count(end) != 1:
        return None
    after_start = body.split(start, 1)[1]
    if end not in after_start:
        return None
    section = after_start.split(end, 1)[0].strip()
    fields: dict[str, str] = {}
    for line in section.splitlines():
        if not line.strip() or ":" not in line:
            continue
        key, value = line.split(":", 1)
        key = key.strip().lower()
        if key in fields:
            return None
        fields[key] = value.strip()
    return fields


def bound_to_head(fields: dict[str, str], base: str, head: str) -> bool:
    if not FULL_SHA.fullmatch(base) or not FULL_SHA.fullmatch(head):
        return False
    return (
        fields.get("base_sha", "").lower() == base.lower()
        and fields.get("head_sha", "").lower() == head.lower()
    )


def has_placeholder(value: str) -> bool:
    upper = value.upper()
    return any(token in upper for token in PLACEHOLDER_TOKENS)


def csv_values(fields: dict[str, str], key: str) -> list[str]:
    return [item.strip() for item in fields.get(key, "").split(",") if item.strip()]


def known_capabilities() -> set[str]:
    path = Path("docs/product/capabilities.yaml")
    if not path.is_file():
        return set()
    return set(re.findall(r"^- id: (CAP-\d+)$", path.read_text(encoding="utf-8"), re.MULTILINE))


def office_route_paths() -> set[str]:
    path = Path("office/src/App.tsx")
    if not path.is_file():
        return set()
    return set(re.findall(r'path="([^"]+)"', path.read_text(encoding="utf-8")))


def core_api_paths() -> set[str]:
    path = Path("native/src/bin/core.rs")
    if not path.is_file():
        return set()
    return set(re.findall(r'\.route\(\s*"([^"]+)"', path.read_text(encoding="utf-8"), re.DOTALL))


def acceptance_has_browser_provenance(items: list[str]) -> bool:
    for item in items:
        path = Path(item)
        if not path.is_file():
            continue
        try:
            content = path.read_text(encoding="utf-8").lower()
        except OSError:
            continue
        if "playwright" in content or "chromium" in content:
            return True
    return False


def office_assessment(body: str, base: str, head: str) -> str | None:
    fields = declared_fields(body, OFFICE_START, OFFICE_END)
    if fields is None or not bound_to_head(fields, base, head):
        return None
    impact = fields.get("impact", "").lower()
    common = {"impact", "rationale", "base_sha", "head_sha"}
    if impact == "none":
        if set(fields) != common:
            return None
        rationale = fields.get("rationale", "")
        if len(rationale) < MIN_RATIONALE_LENGTH or has_placeholder(rationale):
            return None
        return "none"
    if impact != "changed":
        return None
    required = common | {
        "capabilities",
        "routes",
        "core_api",
        "user_actions",
        "states",
        "acceptance",
    }
    if set(fields) != required:
        return None
    rationale = fields.get("rationale", "")
    if len(rationale) < MIN_RATIONALE_LENGTH or has_placeholder(rationale):
        return None
    for key in ("capabilities", "routes", "core_api", "user_actions", "states", "acceptance"):
        if not fields.get(key) or has_placeholder(fields[key]):
            return None
    capabilities = csv_values(fields, "capabilities")
    known = known_capabilities()
    if not capabilities or not known:
        return None
    if any(not re.fullmatch(r"CAP-\d+", item) or item not in known for item in capabilities):
        return None
    routes = csv_values(fields, "routes")
    known_routes = office_route_paths()
    if not routes or any(not item.startswith("/") or item not in known_routes for item in routes):
        return None
    apis = csv_values(fields, "core_api")
    known_apis = core_api_paths()
    if not apis or not known_apis:
        return None
    for item in apis:
        match = re.match(r"^(GET|POST|PUT|DELETE) (/\S+)$", item)
        if match is None or match.group(2) not in known_apis:
            return None
    acceptance = csv_values(fields, "acceptance")
    if not acceptance or any(not Path(item).exists() for item in acceptance):
        return None
    if not acceptance_has_browser_provenance(acceptance):
        return None
    return "changed"


def container_assessment(body: str, base: str, head: str) -> str | None:
    fields = declared_fields(body, CONTAINER_START, CONTAINER_END)
    if fields is None or not bound_to_head(fields, base, head):
        return None
    impact = fields.get("impact", "").lower()
    common = {"impact", "rationale", "base_sha", "head_sha"}
    if impact == "none":
        if set(fields) != common:
            return None
        rationale = fields.get("rationale", "")
        if len(rationale) < MIN_RATIONALE_LENGTH or has_placeholder(rationale):
            return None
        return "none"
    if impact != "changed":
        return None
    required = common | {
        "services",
        "commands",
        "real_podman",
        "negative_tests",
        "not_tested",
        "rationale",
    }
    if set(fields) != required:
        return None
    rationale = fields.get("rationale", "")
    if len(rationale) < MIN_RATIONALE_LENGTH or has_placeholder(rationale):
        return None
    for key in ("services", "commands", "negative_tests", "not_tested"):
        if not fields.get(key) or has_placeholder(fields[key]):
            return None
    real_podman = fields.get("real_podman", "")
    if not real_podman or has_placeholder(real_podman):
        return None
    if "podman" not in real_podman.lower() and "compose" not in real_podman.lower():
        return None
    if "podman" not in fields.get("commands", "").lower() and "compose" not in fields.get("commands", "").lower():
        return None
    return "changed"


def evaluate(
    paths: list[str], body: str, base: str = "", head: str = ""
) -> tuple[bool, str, str]:
    protected = [path for path in paths if is_protected(path)]
    office = [path for path in paths if is_office(path)]
    container = [path for path in paths if is_container_native(path)]
    if not protected and not office and not container:
        return True, "ordinary-change", "No protected, Office or container-native subsystem changed."

    classifications: list[str] = []
    if protected:
        assessment = assessment_from_body(body, base, head)
        if assessment is None:
            return (
                False,
                "assessment-required",
                "Protected files changed; add a valid architecture-impact assessment to the PR description.",
            )
        category, _ = assessment
        classifications.append(category)
    if office and office_assessment(body, base, head) is None:
        return (
            False,
            "office-impact-required",
            "Office files changed; add a valid office-impact declaration with capability, route, API, action, state and acceptance references.",
        )
    if container and container_assessment(body, base, head) is None:
        return (
            False,
            "container-native-impact-required",
            "Container-native files changed; add a valid container-native-impact declaration with services, commands, real Podman test, negative tests and not-tested scope.",
        )
    if office:
        classifications.append("office")
    if container:
        classifications.append("container-native")
    return (
        True,
        "+".join(classifications),
        "Protected, Office and container-native changes have scoped impact declarations.",
    )


def resolved_head(event_head: str, fetched_head: str) -> str:
    """Prefer the branch's current head so stale edited events do not fail.

    A PR body edit triggers an `edited` event whose payload still names the old
    head. The workflow fetches the pull request ref and passes its current head
    here; body binding and the diff are then evaluated against the same commit.
    """
    return fetched_head if FULL_SHA.fullmatch(fetched_head) else event_head


def fetch_current_head(number: int, event_head: str) -> str:
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    if not repository or not number:
        return event_head
    result = subprocess.run(
        ["git", "fetch", "--quiet", "origin", f"pull/{number}/head"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if result.returncode != 0:
        return event_head
    fetched = subprocess.run(
        ["git", "rev-parse", "FETCH_HEAD"],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout.strip()
    return resolved_head(event_head, fetched)


def main() -> int:
    event_path = os.environ.get("GITHUB_EVENT_PATH", "")
    if not event_path:
        print("GITHUB_EVENT_PATH is required", file=sys.stderr)
        return 2
    try:
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        pr = event["pull_request"]
        base = pr["base"]["sha"]
        event_head = pr["head"]["sha"]
        head = fetch_current_head(pr.get("number", 0), event_head)
        body = pr.get("body") or ""
        paths = changed_paths(base, head)
    except (OSError, KeyError, TypeError, json.JSONDecodeError, ValueError, subprocess.CalledProcessError) as exc:
        print(f"unable to establish PR impact evidence: {exc}", file=sys.stderr)
        return 2

    ok, classification, message = evaluate(paths, body, base, head)
    declaration = declaration_section(body) or ""
    office_declaration = declared_fields(body, OFFICE_START, OFFICE_END)
    container_declaration = declared_fields(body, CONTAINER_START, CONTAINER_END)
    report = {
        "pull_request": pr.get("number"),
        "base_sha": base,
        "event_head_sha": event_head,
        "head_sha": head,
        "changed_paths": paths,
        "protected_paths": [path for path in paths if is_protected(path)],
        "office_paths": [path for path in paths if is_office(path)],
        "container_native_paths": [path for path in paths if is_container_native(path)],
        "classification": classification,
        "office_impact": office_assessment(body, base, head),
        "container_native_impact": container_assessment(body, base, head),
        "declaration_sha256": hashlib.sha256(declaration.encode("utf-8")).hexdigest() if declaration else None,
        "office_declaration_sha256": hashlib.sha256(json.dumps(office_declaration, sort_keys=True).encode("utf-8")).hexdigest() if office_declaration else None,
        "container_native_declaration_sha256": hashlib.sha256(json.dumps(container_declaration, sort_keys=True).encode("utf-8")).hexdigest() if container_declaration else None,
        "result": "pass" if ok else "fail",
        "message": message,
        "semantic_review": "required for protected, Office and container-native changes; this check validates declaration presence, shape and diff binding only",
    }
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as output:
            output.write("## PR architecture impact check\n\n")
            output.write(f"Result: **{report['result'].upper()}** — {message}\n\n")
            output.write(f"Bound to base `{base}` and head `{head}`.\n\n")
            output.write("```json\n" + rendered + "\n```\n")
            if not ok:
                output.write(
                    "\nAdd or repair the required PR impact declarations. Replace every placeholder:\n\n"
                    "```text\n<!-- architecture-impact:start -->\n"
                    "category: runtime\n"
                    "rationale: Explain the affected guarantee and the intended behavior change.\n"
                    f"base_sha: {base}\nhead_sha: {head}\n"
                    "<!-- architecture-impact:end -->\n"
                    "<!-- office-impact:start -->\n"
                    "impact: changed\n"
                    "capabilities: CAP-011\n"
                    "routes: /actions,/campaigns/:campaignId\n"
                    "core_api: GET /actions,GET /campaigns/{id}\n"
                    "user_actions: create an Action,inspect the gate\n"
                    "states: loading,empty,error,offline,unauthorized\n"
                    "acceptance: office/e2e-browser/rust-journey.mjs\n"
                    "rationale: Explain the user journey and the evidence.\n"
                    f"base_sha: {base}\nhead_sha: {head}\n"
                    "<!-- office-impact:end -->\n"
                    "<!-- container-native-impact:start -->\n"
                    "impact: changed\n"
                    "services: core,worker\n"
                    "commands: podman compose -f compose.yaml -f compose.dev.yaml run --rm --no-deps dev cargo xtask check\n"
                    "real_podman: bash scripts/compose-e2e.sh\n"
                    "negative_tests: compose-policy,provider egress\n"
                    "not_tested: Windows,SELinux\n"
                    "rationale: Explain the container evidence and trust boundary.\n"
                    f"base_sha: {base}\nhead_sha: {head}\n"
                    "<!-- container-native-impact:end -->\n```\n"
                )
    if not ok:
        print(message, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
