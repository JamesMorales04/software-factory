#!/usr/bin/env python3
"""Disk-overflow cleanup for the builder wrapper.

A post-hoc disk bound must never leave a stale success receipt or a PASSED
check result. This helper removes every success artifact from the run and
output directories and writes a typed failure record instead.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
from pathlib import Path

SUCCESS_ARTIFACTS = (
    "result.json",
    "builder-evidence.json",
    "check-result.json",
    "sbom.cdx.json",
    "sbom-digest.json",
    "closure.json",
    "license-inventory.json",
    "fetch-result.json",
)


def purge_success(run_dir: Path, output_dir: Path) -> None:
    for base in (run_dir, output_dir):
        for name in SUCCESS_ARTIFACTS:
            target = base / name
            if target.exists() or target.is_symlink():
                target.unlink()
            # Remove the check/result variants used by older evidence naming.
            for alias in (f"check-{name}", f"final-{name}"):
                alias_path = base / alias
                if alias_path.exists() or alias_path.is_symlink():
                    alias_path.unlink()


def write_failure(
    run_dir: Path,
    output_dir: Path,
    phase: str,
    *,
    code: str = "ENVIRONMENT_RESOURCE_LIMIT",
    message: str = "observed builder disk usage exceeds the frozen disk bound",
    name: str = "disk-failure.json",
) -> dict:
    usage_path = run_dir / "disk-usage.json"
    usage = {}
    if usage_path.is_file():
        usage = json.loads(usage_path.read_text(encoding="utf-8"))
    failure = {
        "schemaVersion": 1,
        "status": "FAILED",
        "phase": phase,
        "blocker": {"code": code, "message": message},
        "diskUsage": usage,
    }
    for base in (run_dir, output_dir):
        base.mkdir(parents=True, exist_ok=True)
        path = base / name
        path.write_text(json.dumps(failure, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        path.chmod(0o600)
    return failure


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("purge", "failure"))
    parser.add_argument("--run-dir", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--phase", default="post")
    parser.add_argument("--code", default="ENVIRONMENT_RESOURCE_LIMIT")
    parser.add_argument("--message", default="observed builder disk usage exceeds the frozen disk bound")
    parser.add_argument("--name", default="disk-failure.json")
    args = parser.parse_args()
    run_dir = Path(args.run_dir)
    output_dir = Path(args.output)
    if args.action == "purge":
        purge_success(run_dir, output_dir)
    else:
        write_failure(
            run_dir,
            output_dir,
            args.phase,
            code=args.code,
            message=args.message,
            name=args.name,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
