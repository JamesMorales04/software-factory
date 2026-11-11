#!/usr/bin/env bash
# Real Nix resolution smoke for the current environment-builder code.
#
# This is a development harness, not the product topology and not acceptance
# evidence. It uses the already-built builder image and its Nix store, mounts
# the working-tree builder code over the image copy, and performs a real
# `nix flake lock` + `nix develop` + closure-license audit against the pinned
# nixpkgs. `--privileged` is required only because this developer host does not
# grant the nested kernel namespaces that Nix sandboxing uses; the product runs
# through the broker with its own container policy.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FACTORY_ENVIRONMENT_BUILDER_IMAGE:-localhost/software-factory-environment-builder:dev}"
PIN="${FACTORY_ENVIRONMENT_REAL_NIXPKGS:-github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d}"
CARGO_VERSION="${FACTORY_ENVIRONMENT_REAL_CARGO_VERSION:-1.86.0}"
DIGEST="sha256:$(podman image inspect --format '{{.Id}}' "$IMAGE")"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-real-nix-resolve.XXXXXX")"
trap 'podman unshare rm -rf "$TMP" 2>/dev/null || rm -rf "$TMP"' EXIT

if ! podman image exists "$IMAGE"; then
  echo "builder image $IMAGE is not present; build it with compose first" >&2
  exit 2
fi

mkdir -p "$TMP/cache" "$TMP/workspace"
chmod 0777 "$TMP/cache" "$TMP/workspace"

python3 - "$PIN" "$CARGO_VERSION" "$DIGEST" "$TMP" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

pin, cargo_version, digest, tmp = sys.argv[1:]
root = Path(tmp)
flake = f'''{{
  description = "Factory real resolution smoke";
  inputs.nixpkgs.url = "{pin}";
  outputs = {{ self, nixpkgs }}:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {{ inherit system; }};
    in {{
      devShells.${{system}}.default = pkgs.mkShell {{ buildInputs = [ pkgs.cargo ]; }};
    }};
}}
'''
plan = {
    "schemaVersion": 1,
    "planId": "real-nix-plan",
    "projectId": "real-nix-project",
    "requestId": "real-nix-request",
    "actionId": "real-nix-action",
    "baseCommit": "a" * 40,
    "conventionsSha256": "b" * 64,
    "riskClass": "R0",
    "status": "CORE_VALIDATED",
    "revision": 1,
    "source": "GENERATED",
    "toolchains": [{"id": "rust", "version": cargo_version, "source": "fixture", "evidence": []}],
    "packages": [{
        "name": "cargo",
        "version": cargo_version,
        "license": "MIT AND Apache-2.0",
        "source": "nixpkgs",
        "attr": "cargo",
        "sha256": "",
        "unfree": False,
        "attribution": "Cargo is MIT AND Apache-2.0 licensed.",
    }],
    "services": [],
    "commands": {"test": {"argv": ["cargo", "--version"], "cwd": ".", "timeoutSecs": 120, "expectExit": 0}},
    "browsers": [],
    "files": [{
        "path": "flake.nix",
        "sha256": hashlib.sha256(flake.encode()).hexdigest(),
        "origin": "GENERATED",
        "content": flake,
    }],
    "nix": {
        "nixpkgs": pin,
        "inputs": [],
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
    "resources": {"cpu": 2, "memoryMiB": 4096, "diskMiB": 10240, "pids": 512, "timeoutSecs": 1800},
    "network": {"build": "NONE", "fetch": "BROKER", "fetchAllowlist": []},
    "licenseInventory": {
        "schemaVersion": 1,
        "policyRevision": "nix-devenv-license-v1",
        "allowUnfree": False,
        "entries": [],
        "overrides": [],
        "blockers": [],
        "inventorySha256": "c" * 64,
        "sbomSha256": "d" * 64,
    },
    "licenseInventorySha256": "c" * 64,
    "builder": {
        "image": "localhost/software-factory-environment-builder:dev",
        "digest": digest,
        "platform": "x86_64-linux",
        "policyRevision": "nix-devenv-environment-v1",
        "defaultNixpkgs": pin,
        "status": "READY",
    },
    "resolution": None,
    "leadReview": None,
    "blockers": [],
    "commandsRationale": "real nix smoke fixture",
    "planSha256": "e" * 64,
    "createdAt": "2026-11-11T00:00:00Z",
}
request = {"schemaVersion": 1, "plan": plan, "fetchNetwork": "none", "runDir": "/cache/run"}
(root / "cache" / "request.json").write_text(json.dumps(request), encoding="utf-8")
(root / "workspace" / "flake.nix").write_text(flake, encoding="utf-8")
PY

podman run --rm --privileged --security-opt label=disable --entrypoint /bin/sh \
  -v "$TMP/cache:/cache:Z" \
  -v "$TMP/workspace:/inputs/workspace:Z" \
  -v "$ROOT/deploy/environment-builder:/opt/factory-environment-builder:ro,Z" \
  -e FACTORY_ENVIRONMENT_BUILDER_IMAGE="$IMAGE" \
  -e FACTORY_ENVIRONMENT_BUILDER_DIGEST="$DIGEST" \
  -e FACTORY_ENVIRONMENT_BUILDER_PLATFORM=x86_64-linux \
  -e FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE=1 \
  "$IMAGE" -c 'mkdir -p /tmp/warm && cp /inputs/workspace/flake.nix /tmp/warm/ && cd /tmp/warm && nix develop --command true && rm -f /inputs/workspace/flake.lock && /home/factory/.nix-profile/bin/python3 /opt/factory-environment-builder/entrypoint.py resolve --request /cache/request.json --workspace /inputs/workspace --output /cache/run'

podman run --rm --entrypoint /bin/sh -v "$TMP/cache:/cache:Z" "$IMAGE" -c 'cat /cache/run/resolution.json' > "$TMP/resolution.json"

python3 - "$TMP/resolution.json" <<'PY'
import json
import sys
from pathlib import Path

resolution = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
assert resolution["packages"], "no independently observed package metadata"
assert resolution["packages"][0]["name"] == "cargo"
assert resolution["closureLicenses"], "no closure license evidence"
assert any(entry["name"] == "cargo" for entry in resolution["closureLicenses"])
assert all(entry["unfree"] is False for entry in resolution["closureLicenses"])
assert all(entry["license"] for entry in resolution["closureLicenses"])
assert any(file["path"] == "flake.lock" and file["origin"] == "GENERATED" for file in resolution["files"])
print(
    "real-nix-resolve: PASS",
    f"packages={len(resolution['packages'])}",
    f"closure={len(resolution['closureLicenses'])}",
)
PY
