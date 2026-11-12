#!/usr/bin/env bash
# Real committed Devenv profile resolution smoke for the environment builder.
#
# This is a development harness, not acceptance evidence. It uses the existing
# builder image/store, mounts the working-tree builder code, creates a real
# Devenv lock, resolves the committed profile and audits its closure.
# `--privileged` is only the host namespace workaround for Nix sandboxing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${FACTORY_ENVIRONMENT_BUILDER_IMAGE:-localhost/software-factory-environment-builder:dev}"
PIN="${FACTORY_ENVIRONMENT_REAL_NIXPKGS:-github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d}"
DIGEST="sha256:$(podman image inspect --format '{{.Id}}' "$IMAGE")"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/factory-real-devenv.XXXXXX")"
trap 'podman unshare rm -rf "$TMP" 2>/dev/null || rm -rf "$TMP"' EXIT

if ! podman image exists "$IMAGE"; then
  echo "builder image $IMAGE is not present; build it with compose first" >&2
  exit 2
fi

mkdir -p "$TMP/cache" "$TMP/src"
chmod 0777 "$TMP/cache" "$TMP/src"
cat > "$TMP/src/devenv.nix" <<'EOF'
{ pkgs, ... }: { packages = [ pkgs.hello ]; }
EOF
cat > "$TMP/src/devenv.yaml" <<EOF
inputs:
  nixpkgs:
    url: $PIN
EOF
chmod 0666 "$TMP/src"/*

# Generate the real devenv.lock with network, then remove runtime state.
podman run --rm --privileged --security-opt label=disable --entrypoint /bin/sh \
  -v "$TMP/src:/inputs/workspace:Z" "$IMAGE" \
  -c 'cd /inputs/workspace && devenv update >/dev/null 2>&1 && rm -rf .devenv'

python3 - "$PIN" "$DIGEST" "$TMP" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

pin, digest, tmp = sys.argv[1:]
root = Path(tmp)
files = []
for name in ("devenv.nix", "devenv.yaml", "devenv.lock"):
    data = (root / "src" / name).read_bytes()
    files.append(
        {"path": name, "sha256": hashlib.sha256(data).hexdigest(), "origin": "PROJECT"}
    )
plan = {
    "schemaVersion": 1,
    "planId": "real-committed-devenv",
    "projectId": "real-committed-devenv-project",
    "requestId": "real-committed-devenv-request",
    "actionId": "real-committed-devenv-action",
    "baseCommit": "a" * 40,
    "conventionsSha256": "b" * 64,
    "riskClass": "R0",
    "status": "CORE_VALIDATED",
    "revision": 1,
    "source": "PROJECT_COMMITTED",
    "toolchains": [],
    "packages": [],
    "services": [],
    "commands": {
        "test": {
            "argv": ["hello", "--version"],
            "cwd": ".",
            "timeoutSecs": 120,
            "expectExit": 0,
        }
    },
    "browsers": [],
    "files": files,
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
    "commandsRationale": "committed Devenv smoke fixture",
    "planSha256": "e" * 64,
    "createdAt": "2026-11-11T00:00:00Z",
}
(root / "cache" / "request.json").write_text(
    json.dumps({"schemaVersion": 1, "plan": plan, "fetchNetwork": "none", "runDir": "/cache/run"}),
    encoding="utf-8",
)
PY

mkdir -p "$TMP/cache/run"
chmod 0777 "$TMP/cache/run"

podman run --rm --privileged --security-opt label=disable --entrypoint /bin/sh \
  -v "$TMP/cache:/cache:Z" \
  -v "$TMP/src:/inputs/workspace:Z" \
  -v "$ROOT/deploy/environment-builder:/opt/factory-environment-builder:ro,Z" \
  -e FACTORY_ENVIRONMENT_BUILDER_IMAGE="$IMAGE" \
  -e FACTORY_ENVIRONMENT_BUILDER_DIGEST="$DIGEST" \
  -e FACTORY_ENVIRONMENT_BUILDER_PLATFORM=x86_64-linux \
  -e FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE=1 \
  "$IMAGE" -c 'rm -rf /tmp/warm && mkdir -p /tmp/warm && cp /inputs/workspace/* /tmp/warm/ && cd /tmp/warm && devenv shell -- true && /home/factory/.nix-profile/bin/python3 /opt/factory-environment-builder/entrypoint.py resolve --request /cache/request.json --workspace /inputs/workspace --output /cache/run'

podman run --rm --entrypoint /bin/sh -v "$TMP/cache:/cache:Z" "$IMAGE" \
  -c 'cat /cache/run/resolution.json' > "$TMP/resolution.json"

python3 - "$TMP/resolution.json" <<'PY'
import json
import sys
from pathlib import Path

resolution = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
assert any(file["path"] == "devenv.lock" and file["origin"] == "PROJECT" for file in resolution["files"])
assert resolution["closureLicenses"], "no committed Devenv closure licence evidence"
assert all(entry["unfree"] is False for entry in resolution["closureLicenses"])
assert all(entry["license"] for entry in resolution["closureLicenses"])
print(
    "real-committed-devenv: PASS",
    f"closure={len(resolution['closureLicenses'])}",
)
PY
