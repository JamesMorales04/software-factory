#!/usr/bin/env python3
"""Create a builder request JSON from a committed environment fixture.

This helper is for engineering tests only. Core produces the authoritative
EnvironmentPlan/EnvironmentSpec; this script mirrors the fields the builder
consumes so a fixture can exercise the same validation and receipt path.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

POLICY = "nix-devenv-environment-v1"
LICENSE_POLICY = "nix-devenv-license-v1"
PLATFORM = "x86_64-linux"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value) -> str:
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def sha256_json(value) -> str:
    return sha256_bytes(canonical(value).encode("utf-8"))


def substrate() -> list[dict]:
    return [
        {
            "name": "nix",
            "version": "2.28.3",
            "license": "LGPL-2.1-only",
            "source": "https://github.com/NixOS/nix/blob/master/COPYING",
            "sha256": "",
            "unfree": False,
            "ownerOverrideId": None,
            "attribution": "LGPL-2.1 tool; Factory invokes Nix as a separate executable.",
        },
        {
            "name": "nixpkgs",
            "version": "pinned",
            "license": "MIT",
            "source": "https://github.com/NixOS/nixpkgs/blob/master/COPYING",
            "sha256": "",
            "unfree": False,
            "ownerOverrideId": None,
            "attribution": "Copyright 2003-2026 Eelco Dolstra and the Nixpkgs/NixOS contributors.",
        },
        {
            "name": "devenv",
            "version": "1.11.2",
            "license": "Apache-2.0",
            "source": "https://github.com/cachix/devenv/blob/main/LICENSE",
            "sha256": "",
            "unfree": False,
            "ownerOverrideId": None,
            "attribution": "Apache-2.0; repository LICENSE and Cargo.toml are authoritative.",
        },
    ]


def selected_packages(nixpkgs: str) -> list[dict]:
    return [
        {
            "name": "nodejs",
            "version": "22.20.0",
            "license": "MIT",
            "source": nixpkgs,
            "attr": "nodejs_22",
            "sha256": "",
            "unfree": False,
            "attribution": "Node.js is MIT licensed.",
        },
        {
            "name": "playwright",
            "version": "1.64.1",
            "license": "Apache-2.0",
            "source": nixpkgs,
            "attr": "playwright-driver",
            "sha256": "",
            "unfree": False,
            "attribution": "Playwright is Apache-2.0 licensed.",
        },
        {
            "name": "chromium",
            "version": "1.64.1",
            "license": "BSD-3-Clause",
            "source": nixpkgs,
            "attr": "playwright-driver.browsers",
            "sha256": "",
            "unfree": False,
            "attribution": "Chromium browser build distributed by the pinned Playwright driver.",
        },
        {
            "name": "rust",
            "version": "1.90.0",
            "license": "MIT",
            "source": nixpkgs,
            "attr": "rust-bin",
            "sha256": "",
            "unfree": False,
            "attribution": "Rust toolchain is MIT/Apache-2.0 dual licensed.",
        },
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--image", required=True)
    parser.add_argument("--digest", required=True)
    parser.add_argument("--nixpkgs", required=True)
    parser.add_argument("--nix-version", default="2.28.3")
    parser.add_argument("--devenv-version", default="1.11.2")
    args = parser.parse_args()

    workspace = Path(args.workspace).resolve()
    names = ["devenv.nix", "devenv.yaml", "devenv.lock", "flake.nix", "flake.lock"]
    files = []
    for name in names:
        path = workspace / name
        if path.is_file():
            data = path.read_bytes()
            files.append(
                {
                    "path": name,
                    "sha256": sha256_bytes(data),
                    "origin": "PROJECT",
                    "content": None,
                }
            )
    if not any(entry["path"] in ("devenv.nix", "flake.nix") for entry in files):
        raise SystemExit("fixture must contain devenv.nix or flake.nix")
    if not any(entry["path"] in ("devenv.lock", "flake.lock") for entry in files):
        raise SystemExit("fixture must contain a lock file")

    packages = selected_packages(args.nixpkgs)
    entries = substrate()
    for package in packages:
        entries.append(
            {
                "name": package["name"],
                "version": package["version"],
                "license": package["license"],
                "source": package["source"],
                "sha256": package["sha256"],
                "unfree": False,
                "ownerOverrideId": None,
                "attribution": package["attribution"],
            }
        )
    inventory_sha = sha256_json(entries)
    sbom_sha = sha256_json(entries)
    inventory = {
        "schemaVersion": 1,
        "policyRevision": LICENSE_POLICY,
        "allowUnfree": False,
        "entries": entries,
        "overrides": [],
        "blockers": [],
        "inventorySha256": inventory_sha,
        "sbomSha256": sbom_sha,
    }
    inputs = []
    devenv_yaml = workspace / "devenv.yaml"
    if devenv_yaml.is_file():
        for line in devenv_yaml.read_text(encoding="utf-8").splitlines():
            stripped = line.strip()
            if stripped.startswith("url:") and "github:" in stripped:
                url = stripped.split("url:", 1)[1].strip().strip('"').strip("'")
                if "NixOS/nixpkgs" not in url:
                    inputs.append({"name": "rust-overlay", "url": url, "sha256": ""})
                    break
    resources = {"cpu": 2, "memoryMiB": 4096, "diskMiB": 10240, "pids": 512, "timeoutSecs": 1800}
    network = {
        "build": "NONE",
        "fetch": "BROKER",
        "fetchAllowlist": [
            "https://cache.nixos.org",
            "https://github.com",
            "https://codeload.github.com",
            "https://devenv.cachix.org",
            "https://registry.npmjs.org",
        ],
    }
    plan = {
        "schemaVersion": 1,
        "planId": "fixture-plan",
        "projectId": "fixture-project",
        "requestId": "fixture-request",
        "actionId": "fixture-action",
        "campaignId": None,
        "baseCommit": "a" * 40,
        "conventionsSha256": "b" * 64,
        "conventions": {
            "schemaVersion": 1,
            "projectId": "fixture-project",
            "baseCommit": "a" * 40,
            "conventionsSha256": "b" * 64,
            "scannedAt": "2026-11-10T00:00:00Z",
            "files": [],
            "commands": [],
            "conflicts": [],
            "truncated": False,
            "skippedEntries": 0,
            "provenance": "fixture",
            "existingEnvironment": "DEVENV",
        },
        "status": "FROZEN",
        "revision": 1,
        "source": "PROJECT_COMMITTED",
        "toolchains": [
            {"id": "node", "version": "22.20.0", "source": "fixture", "evidence": []},
            {"id": "rust", "version": "1.90.0", "source": "fixture", "evidence": []},
        ],
        "packages": packages,
        "services": [],
        "commands": {
            "test": {"argv": ["npm", "test"], "cwd": "projects/node", "timeoutSecs": 300, "expectExit": 0}
        },
        "browsers": [{"name": "chromium", "version": "1.64.1", "source": "playwright-driver"}],
        "files": files,
        "nix": {
            "nixpkgs": args.nixpkgs,
            "inputs": inputs,
            "allowUnfree": False,
            "sandbox": {
                "sandbox": True,
                "impure": False,
                "hostMounts": [],
                "privileged": False,
                "hostNetwork": False,
                "hostSockets": [],
            },
        },
        "resources": resources,
        "network": network,
        "licenseInventory": inventory,
        "licenseInventorySha256": inventory_sha,
        "builder": {
            "image": args.image,
            "digest": args.digest,
            "platform": PLATFORM,
            "policyRevision": POLICY,
            "status": "READY",
        },
        "leadReview": None,
        "blockers": [],
        "commandsRationale": "Fixture command for builder evidence.",
        "planSha256": "",
        "createdAt": "2026-11-10T00:00:00Z",
    }
    plan["planSha256"] = sha256_json(plan)
    spec = {
        "schemaVersion": 1,
        "specId": "fixture-spec",
        "specSha256": "",
        "projectId": "fixture-project",
        "requestId": "fixture-request",
        "actionId": "fixture-action",
        "campaignId": None,
        "baseCommit": "a" * 40,
        "planId": "fixture-plan",
        "planSha256": plan["planSha256"],
        "conventionsSha256": "b" * 64,
        "reviewId": "fixture-review",
        "platform": PLATFORM,
        "nixVersion": args.nix_version,
        "devenvVersion": args.devenv_version,
        "flakeLockSha256": next((entry["sha256"] for entry in files if entry["path"] == "flake.lock"), ""),
        "devenvLockSha256": next((entry["sha256"] for entry in files if entry["path"] == "devenv.lock"), ""),
        "licenseInventorySha256": inventory_sha,
        "sbomSha256": sbom_sha,
        "builderImage": args.image,
        "builderImageDigest": args.digest,
        "resourcePolicy": resources,
        "networkPolicy": network,
        "status": "FROZEN",
        "frozenAt": "2026-11-10T00:00:00Z",
    }
    spec["specSha256"] = sha256_json(spec)
    request = {
        "schemaVersion": 1,
        "requestId": "fixture-request",
        "spec": spec,
        "plan": plan,
    }
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    Path(args.output).write_text(json.dumps(request, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(args.output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
