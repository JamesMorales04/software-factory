#!/usr/bin/env bash
# Real Nix AST rejection smoke for dynamic nixpkgs config construction.
#
# This is a development harness, not acceptance evidence. It submits committed
# flakes containing (1) a long-whitespace/nested-let merged config key and (2) a
# let-bound variable alias to the builder and requires ENVIRONMENT_SANDBOX_DENIED
# before any Nix realisation. `--privileged` is only the host namespace
# workaround for Nix sandboxing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FACTORY_ENVIRONMENT_BUILDER_IMAGE:-localhost/software-factory-environment-builder:dev}"
PIN="${FACTORY_ENVIRONMENT_REAL_NIXPKGS:-github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d}"
DIGEST="sha256:$(podman image inspect --format '{{.Id}}' "$IMAGE")"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-real-config-bypass.XXXXXX")"
trap 'podman unshare rm -rf "$TMP" 2>/dev/null || rm -rf "$TMP"' EXIT

if ! podman image exists "$IMAGE"; then
  echo "builder image $IMAGE is not present; build it with compose first" >&2
  exit 2
fi

mkdir -p "$TMP/cache" "$TMP/src"
chmod 0777 "$TMP/cache" "$TMP/src"

python3 - "$PIN" "$DIGEST" "$TMP" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

pin, digest, tmp = sys.argv[1:]
root = Path(tmp)
flakes = {
    "merged": (
        "{\n"
        "  outputs = { self, nixpkgs }:\n"
        "    let\n"
        '      key = "allowUn" + "freePredicate";\n'
        "      pkgs = import nixpkgs {\n"
        "        config = {} //" + " " * 4000 + " (let x = 1; in { ${key} = _: true; });\n"
        "      };\n"
        "    in { devShells.x86_64-linux.default = pkgs.mkShell { }; };\n"
        "}\n"
    ),
    "alias": (
        "{\n"
        "  outputs = { self, nixpkgs }:\n"
        "    let\n"
        '      allow = "allowUn" + "freePredicate";\n'
        '      settings = { "${allow}" = _: true; };\n'
        "    in import nixpkgs { config = settings; };\n"
        "}\n"
    ),
    "computed": (
        "{\n"
        "  outputs = { self, nixpkgs }:\n"
        "    let\n"
        '      configKey = "con" + "fig";\n'
        '      allow = "allowUn" + "freePredicate";\n'
        '      settings = { "${allow}" = _: true; };\n'
        "    in import nixpkgs { ${configKey} = settings; };\n"
        "}\n"
    ),
}
for name, flake in flakes.items():
    plan = {
        "schemaVersion": 1,
        "planId": f"bypass-{name}",
        "projectId": "bypass-project",
        "requestId": f"bypass-request-{name}",
        "actionId": "bypass-action",
        "baseCommit": "a" * 40,
        "conventionsSha256": "b" * 64,
        "riskClass": "R0",
        "status": "CORE_VALIDATED",
        "revision": 1,
        "source": "PROJECT_COMMITTED",
        "toolchains": [],
        "packages": [],
        "services": [],
        "commands": {},
        "browsers": [],
        "files": [
            {
                "path": "flake.nix",
                "sha256": hashlib.sha256(flake.encode()).hexdigest(),
                "origin": "PROJECT",
            }
        ],
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
        "commandsRationale": "bypass fixture",
        "planSha256": "e" * 64,
        "createdAt": "2026-11-11T00:00:00Z",
    }
    source = root / f"src-{name}"
    source.mkdir(parents=True, exist_ok=True)
    (source / "flake.nix").write_text(flake, encoding="utf-8")
    (root / "cache" / f"{name}.json").write_text(
        json.dumps(
            {
                "schemaVersion": 1,
                "plan": plan,
                "fetchNetwork": "none",
                "runDir": f"/cache/run-{name}",
            }
        ),
        encoding="utf-8",
    )
PY

chmod -R 0777 "$TMP/cache"
for case in merged alias computed; do
  mkdir -p "$TMP/cache/run-$case"
  chmod 0777 "$TMP/cache/run-$case"
  set +e
  output="$(podman run --rm --privileged --security-opt label=disable \
    --entrypoint /home/factory/.nix-profile/bin/python3 \
    -v "$TMP/cache:/cache:Z" \
    -v "$TMP/src-$case:/inputs/workspace:Z" \
    -v "$ROOT/deploy/environment-builder:/opt/factory-environment-builder:ro,Z" \
    -e FACTORY_ENVIRONMENT_BUILDER_IMAGE="$IMAGE" \
    -e FACTORY_ENVIRONMENT_BUILDER_DIGEST="$DIGEST" \
    -e FACTORY_ENVIRONMENT_BUILDER_PLATFORM=x86_64-linux \
    "$IMAGE" /opt/factory-environment-builder/entrypoint.py resolve \
    --request "/cache/$case.json" \
    --workspace /inputs/workspace \
    --output "/cache/run-$case" 2>&1)"
  status=$?
  set -e
  if [ "$status" -eq 0 ]; then
    echo "bypass case $case unexpectedly resolved" >&2
    echo "$output" >&2
    exit 1
  fi
  if ! printf '%s' "$output" | grep -q 'ENVIRONMENT_SANDBOX_DENIED'; then
    echo "bypass case $case did not fail with ENVIRONMENT_SANDBOX_DENIED" >&2
    echo "$output" >&2
    exit 1
  fi
  echo "real-config-bypass[$case]: PASS (ENVIRONMENT_SANDBOX_DENIED)"
done
