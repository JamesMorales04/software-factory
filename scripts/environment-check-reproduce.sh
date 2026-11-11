#!/usr/bin/env bash
# Reproduce and verify the environment-check workspace staging failure in the
# real builder image.
#
# Observed live acceptance failure: a campaign's frozen `npm test` check
# returned exit 254 while a manual check with the same spec/receipt and the
# plain base project passed. The Worker workspace is a Git worktree, but the
# Supervisor's environment-check route used the broker workspace volume
# without materializing the current worktree first. Allocation creates an
# empty volume; only generic run/dependency operations refresh it. The builder
# therefore ran `npm test` against a workspace containing only the generated
# environment files and no package.json. Real npm reports exactly:
#
#   npm error enoent Could not read package.json
#   npm error path /cache/check-workspace/package.json
#   exit 254
#
# This harness runs the real builder image, the real `prepare_workspace` and
# `run_check` functions, and real npm. Nix environment realization is bypassed
# only so the reproduction is offline and deterministic; the live campaign had
# already realized its environment before npm failed.
#
# It must NOT be treated as acceptance evidence for a Nix/devenv environment.
set -euo pipefail

IMAGE="${FACTORY_ENVIRONMENT_BUILDER_IMAGE:-localhost/software-factory-environment-builder:dev}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/factory-check-repro.XXXXXX")"
REPO="$WORK/repo"
WORKTREE="$WORK/worktree"
CACHE_VOLUME="factory-check-repro-cache-$$"
EMPTY_VOLUME="factory-check-repro-empty-$$"
RUNNER="$WORK/run_check.py"

cleanup() {
    podman volume rm --force "$CACHE_VOLUME" >/dev/null 2>&1 || true
    podman volume rm --force "$EMPTY_VOLUME" >/dev/null 2>&1 || true
    rm -rf "$WORK"
}
trap cleanup EXIT

if [[ "$(podman info --format '{{.Host.Security.Rootless}}')" != "true" ]]; then
    echo "FAIL: this harness requires rootless Podman (the builder contract)" >&2
    exit 2
fi

podman image exists "$IMAGE" || {
    echo "FAIL: builder image $IMAGE is not present" >&2
    exit 2
}

mkdir -p "$REPO"
git -C "$REPO" init -q
git -C "$REPO" config user.email "fixture@example.invalid"
git -C "$REPO" config user.name "Fixture"
cat >"$REPO/package.json" <<'EOF'
{"name":"factory-check-fixture","version":"1.0.0","scripts":{"test":"node -e \"console.log('tests passed')\""}}
EOF
git -C "$REPO" add package.json
git -C "$REPO" commit -qm base
git -C "$REPO" worktree add -q "$WORKTREE" HEAD

cat >"$RUNNER" <<'PY'
import json
import sys
from pathlib import Path

sys.path.insert(0, "/opt/factory-environment-builder")
import environment_builder.nix as nix
from environment_builder.nix import BuildLog, prepare_workspace, run_check

# The environment has already been realized here. Real npm, the real
# prepare_workspace copy and the real bounded check runner still run.
nix._environment_command = lambda workspace, argv, offline: argv

source = Path("/inputs/workspace")
cache = Path("/cache")
cache.mkdir(parents=True, exist_ok=True)
check_workspace = cache / "check-workspace"
prepare_workspace(source, check_workspace, reuse=False)
resources = {
    "cpu": 2,
    "memoryMiB": 4096,
    "diskMiB": 10240,
    "pids": 512,
    "timeoutSecs": 1800,
}
log = BuildLog(cache / "check.log")
try:
    code, stdout, stderr, stdout_truncated, stderr_truncated = run_check(
        check_workspace,
        cache,
        check_workspace,
        ["npm", "test"],
        timeout=60,
        resources=resources,
        log=log,
        expect_exit=0,
    )
finally:
    log.close()
print(
    json.dumps(
        {
            "exitCode": code,
            "stdout": stdout,
            "stderr": stderr,
            "stdoutTruncated": stdout_truncated,
            "stderrTruncated": stderr_truncated,
            "packageJson": (check_workspace / "package.json").is_file(),
            "gitMetadata": (check_workspace / ".git").exists(),
        },
        sort_keys=True,
    )
)
PY

run_case() {
    local label="$1"
    local source="$2"
    local volume="$3"
    podman run --rm --network=none --read-only \
        --userns=keep-id:uid=10001,gid=10001 --user=10001:10001 \
        --tmpfs=/tmp:rw,noexec,nosuid,size=256m \
        --tmpfs=/run:rw,noexec,nosuid,size=32m \
        --tmpfs=/home/factory/.cache:rw,noexec,nosuid,size=256m \
        --tmpfs=/home/factory/.local:rw,noexec,nosuid,size=128m \
        --security-opt=no-new-privileges \
        --volume="$RUNNER:/run_check.py:ro" \
        --volume="$source:/inputs/workspace:ro" \
        --volume="$volume:/cache:rw" \
        --entrypoint /bin/sh "$IMAGE" -c 'exec python3 /run_check.py'
}

podman volume create "$EMPTY_VOLUME" >/dev/null
podman volume create "$CACHE_VOLUME" >/dev/null

echo "== empty (allocated but never staged) workspace volume"
empty="$(run_case empty "$EMPTY_VOLUME" "$EMPTY_VOLUME")"
echo "$empty"

echo "== git-worktree candidate workspace"
worktree="$(run_case worktree "$WORKTREE" "$CACHE_VOLUME")"
echo "$worktree"

echo "== plain-directory base project (manual comparison)"
plain="$(run_case plain "$REPO" "$CACHE_VOLUME")"
echo "$plain"

python3 - "$empty" "$worktree" "$plain" <<'PY'
import json
import sys

empty, worktree, plain = (json.loads(value) for value in sys.argv[1:4])
assert empty["exitCode"] == 254, empty
assert empty["packageJson"] is False, empty
assert "Could not read package.json" in empty["stderr"], empty
assert worktree["exitCode"] == 0, worktree
assert worktree["packageJson"] is True, worktree
assert worktree["gitMetadata"] is False, "the archive contract excludes .git"
assert plain["exitCode"] == 0, plain
print("PASS: empty workspace reproduces npm exit 254; git worktree and plain project pass")
PY
