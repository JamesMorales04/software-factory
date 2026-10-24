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


def evaluate(
    paths: list[str], body: str, base: str = "", head: str = ""
) -> tuple[bool, str, str]:
    protected = [path for path in paths if is_protected(path)]
    if not protected:
        return True, "ordinary-change", "No protected architectural subsystem changed."
    assessment = assessment_from_body(body, base, head)
    if assessment is None:
        return (
            False,
            "assessment-required",
            "Protected files changed; add a valid architecture-impact assessment to the PR description.",
        )
    category, _ = assessment
    return True, category, "Protected changes have a PR-scoped impact assessment."


def main() -> int:
    event_path = os.environ.get("GITHUB_EVENT_PATH", "")
    if not event_path:
        print("GITHUB_EVENT_PATH is required", file=sys.stderr)
        return 2
    try:
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        pr = event["pull_request"]
        base = pr["base"]["sha"]
        head = pr["head"]["sha"]
        body = pr.get("body") or ""
        paths = changed_paths(base, head)
    except (OSError, KeyError, TypeError, json.JSONDecodeError, ValueError, subprocess.CalledProcessError) as exc:
        print(f"unable to establish PR impact evidence: {exc}", file=sys.stderr)
        return 2

    ok, classification, message = evaluate(paths, body, base, head)
    declaration = declaration_section(body) or ""
    report = {
        "pull_request": pr.get("number"),
        "base_sha": base,
        "head_sha": head,
        "changed_paths": paths,
        "protected_paths": [path for path in paths if is_protected(path)],
        "classification": classification,
        "declaration_sha256": hashlib.sha256(declaration.encode("utf-8")).hexdigest() if declaration else None,
        "result": "pass" if ok else "fail",
        "message": message,
        "semantic_review": "required for protected changes; this check validates declaration presence and diff binding only",
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
                    "\nAdd this section to the PR description and replace the placeholders:\n\n"
                    "```text\n<!-- architecture-impact:start -->\n"
                    "category: runtime\n"
                    "rationale: Explain the affected guarantee and the intended behavior change.\n"
                    f"base_sha: {base}\nhead_sha: {head}\n"
                    "<!-- architecture-impact:end -->\n```\n"
                )
    if not ok:
        print(message, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
