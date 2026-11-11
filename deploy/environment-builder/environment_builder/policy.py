"""Deterministic policy validation for one frozen environment build.

Everything in this module is intentionally fail-closed: a missing pin, an
unpinned input, a forbidden Nix construct, a secret-shaped value, an unfree
package or a sandbox/network disagreement is a typed blocker. The builder never
falls back to a host or language image.
"""

from __future__ import annotations

import json
import os
import re
from pathlib import Path
from typing import Any, Final, Iterable

from . import (
    BUILDER_POLICY_REVISION,
    DEFAULT_PLATFORM,
    LICENSE_POLICY_REVISION,
    SCHEMA_VERSION,
)
from .errors import BuilderError, fail
from .util import (
    SECRET_PATTERNS,
    bounded_text,
    canonical_json,
    is_digest,
    is_git_revision,
    is_sha256,
    safe_relative_path,
    sha256_bytes,
    sha256_text,
)

MAX_ENVIRONMENT_FILES: Final[int] = 64
MAX_ENVIRONMENT_FILE_BYTES: Final[int] = 512 * 1024
MAX_PACKAGES: Final[int] = 512
MAX_INPUTS: Final[int] = 64
MAX_ARGV: Final[int] = 64
MAX_ARG_CHARS: Final[int] = 2_000
MIN_CPU: Final[int] = 1
MAX_CPU: Final[int] = 16
MIN_MEMORY_MIB: Final[int] = 512
MAX_MEMORY_MIB: Final[int] = 65_536
MIN_DISK_MIB: Final[int] = 1_024
MAX_DISK_MIB: Final[int] = 262_144
MIN_PIDS: Final[int] = 64
MAX_PIDS: Final[int] = 4_096
MIN_TIMEOUT_SECS: Final[int] = 30
MAX_TIMEOUT_SECS: Final[int] = 7_200
NIXPKGS_PREFIX: Final[str] = "github:NixOS/nixpkgs/"

ALLOWED_SPDX: Final[frozenset[str]] = frozenset(
    {
        "0BSD",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "BSL-1.0",
        "CC0-1.0",
        "ISC",
        "LGPL-2.1-only",
        "LGPL-2.1-or-later",
        "LGPL-3.0-only",
        "LGPL-3.0-or-later",
        "MIT",
        "MIT-0",
        "MPL-2.0",
        "Unlicense",
        "Zlib",
    }
)
DENIED_SPDX: Final[frozenset[str]] = frozenset(
    {
        "AGPL-3.0-only",
        "AGPL-3.0-or-later",
        "BUSL-1.1",
        "Commons-Clause",
        "SSPL-1.0",
    }
)

# Closure-wide licensing accepts common copyleft licences that are not denied
# by product policy, while still rejecting every unfree/denied/unknown package.
CLOSURE_ALLOWED_SPDX: Final[frozenset[str]] = ALLOWED_SPDX | frozenset(
    {
        "GPL-2.0-only",
        "GPL-2.0-or-later",
        "GPL-3.0-only",
        "GPL-3.0-or-later",
        "BSD-4-Clause",
        "OpenSSL",
        "PostgreSQL",
        "Public Domain",
    }
)
MAX_CLOSURE_PACKAGES: Final[int] = 4096

# Patterns that make a generated/project environment untrusted code instead of
# a declarative environment. The sandbox is the enforcement boundary; these
# checks are defence in depth and produce a typed blocker.
FORBIDDEN_NIX_PATTERNS: Final[tuple[tuple[re.Pattern[str], str, str], ...]] = (
    (re.compile(r"\bbuiltins\.exec\b"), "ENVIRONMENT_SANDBOX_DENIED", "builtins.exec"),
    (re.compile(r"\bbuiltins\.getEnv\b"), "ENVIRONMENT_SANDBOX_DENIED", "builtins.getEnv"),
    (re.compile(r"\bbuiltins\.currentSystem\b"), "ENVIRONMENT_PIN_MISSING", "builtins.currentSystem"),
    (re.compile(r"\bbuiltins\.storePath\b"), "ENVIRONMENT_SANDBOX_DENIED", "builtins.storePath"),
    (re.compile(r"\b__noChroot\b"), "ENVIRONMENT_SANDBOX_DENIED", "__noChroot"),
    (re.compile(r"\b--impure\b"), "ENVIRONMENT_SANDBOX_DENIED", "--impure"),
    (re.compile(r"\bimpure\s*[=:]\s*true\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "impure=true"),
    (re.compile(r"\bimport\s+<nixpkgs>"), "ENVIRONMENT_PIN_MISSING", "import <nixpkgs>"),
    # Reject every unfree-policy identifier, not just `allowUnfree = true`.
    # This covers allowUnfreePredicate, permittedUnfreePackages,
    # config.allowUnfree, nixpkgs.config.allowUnfree and equivalent forms.
    (re.compile(r"unfree", re.IGNORECASE), "ENVIRONMENT_LICENSE_DENIED", "unfree policy"),
    (re.compile(r"\bbuiltins\.getAttr\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "builtins.getAttr"),
    # Dynamic attribute/config construction can rebuild an unfree policy from
    # fragments that do not contain the literal identifier.
    (re.compile(r"\bbuiltins\.listToAttrs\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "builtins.listToAttrs"),
    (re.compile(r"\bbuiltins\.mapAttrs\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "builtins.mapAttrs"),
    (re.compile(r"\bbuiltins\.fromJSON\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "builtins.fromJSON"),
    (re.compile(r"\bbuiltins\.attrNames\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "builtins.attrNames"),
    (re.compile(r"\bbuiltins\.functionArgs\b", re.IGNORECASE), "ENVIRONMENT_SANDBOX_DENIED", "builtins.functionArgs"),
    (re.compile(r"\bsandbox\s*=\s*false\b"), "ENVIRONMENT_SANDBOX_DENIED", "sandbox=false"),
    (re.compile(r"\bnixConfig\s*=\b"), "ENVIRONMENT_SANDBOX_DENIED", "flake nixConfig"),
    (re.compile(r"\bextra-substituters\b"), "ENVIRONMENT_NETWORK_DENIED", "extra substituter"),
    (re.compile(r"\bextra-trusted-public-keys\b"), "ENVIRONMENT_NETWORK_DENIED", "extra trusted key"),
    (re.compile(r"\b(?:builtins\.)?fetchTarball\b"), "ENVIRONMENT_PIN_MISSING", "fetchTarball"),
    (re.compile(r"\b(?:builtins\.)?fetchGit\b"), "ENVIRONMENT_PIN_MISSING", "fetchGit"),
    (re.compile(r"\b(?:builtins\.)?fetchurl\b"), "ENVIRONMENT_PIN_MISSING", "fetchurl"),
    (re.compile(r"\b(?:builtins\.)?fetchTree\b"), "ENVIRONMENT_PIN_MISSING", "fetchTree"),
)

INPUT_URL = re.compile(r"\binputs(?:\.[A-Za-z0-9_.-]+)*\.url\s*=\s*[\"']([^\"']+)[\"']")
YAML_INPUT_URL = re.compile(r"^\s*url:\s*[\"']?([^\"'\s]+)[\"']?\s*$", re.MULTILINE)
YAML_BOOL = re.compile(r"^\s*([A-Za-z0-9_-]+)\s*:\s*(true|false)\s*$", re.MULTILINE)
LOCKED_REVISION = re.compile(r"[?&]rev=([0-9a-f]{40}|[0-9a-f]{64})")
GITHUB_REVISION = re.compile(r"^github:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/([0-9a-f]{40}|[0-9a-f]{64})(?:\?.*)?$")


class EnvironmentFile:
    __slots__ = ("path", "sha256", "origin", "content")

    def __init__(self, path: str, sha256: str, origin: str, content: str) -> None:
        self.path = path
        self.sha256 = sha256
        self.origin = origin
        self.content = content

    def __repr__(self) -> str:  # pragma: no cover - diagnostics only
        return f"EnvironmentFile({self.path!r}, {self.origin!r})"


def _require_mapping(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail("ENVIRONMENT_SPEC_INVALID", f"{label} must be an object")
    return value


def _require_list(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        fail("ENVIRONMENT_SPEC_INVALID", f"{label} must be an array")
    return value


def _require_str(value: Any, label: str, limit: int = 2_000) -> str:
    try:
        return bounded_text(value, limit)
    except ValueError as error:
        fail("ENVIRONMENT_SPEC_INVALID", f"{label}: {error}")
    raise AssertionError("unreachable")


def _require_bool(value: Any, label: str) -> bool:
    if not isinstance(value, bool):
        fail("ENVIRONMENT_SPEC_INVALID", f"{label} must be a boolean")
    return value


def _require_int(value: Any, label: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool):
        fail("ENVIRONMENT_SPEC_INVALID", f"{label} must be an integer")
    return value


def validate_resources(resources: Any) -> dict[str, int]:
    resources = _require_mapping(resources, "resources")
    cpu = _require_int(resources.get("cpu"), "resources.cpu")
    memory = _require_int(resources.get("memoryMiB"), "resources.memoryMiB")
    disk = _require_int(resources.get("diskMiB"), "resources.diskMiB")
    pids = _require_int(resources.get("pids"), "resources.pids")
    timeout = _require_int(resources.get("timeoutSecs"), "resources.timeoutSecs")
    if (
        not MIN_CPU <= cpu <= MAX_CPU
        or not MIN_MEMORY_MIB <= memory <= MAX_MEMORY_MIB
        or not MIN_DISK_MIB <= disk <= MAX_DISK_MIB
        or not MIN_PIDS <= pids <= MAX_PIDS
        or not MIN_TIMEOUT_SECS <= timeout <= MAX_TIMEOUT_SECS
    ):
        fail(
            "ENVIRONMENT_RESOURCE_LIMIT",
            "CPU, memory, disk, PID or wall-clock bound is outside the supported policy",
        )
    return {
        "cpu": cpu,
        "memoryMiB": memory,
        "diskMiB": disk,
        "pids": pids,
        "timeoutSecs": timeout,
    }


def validate_network(network: Any) -> dict[str, Any]:
    network = _require_mapping(network, "network")
    build = _require_str(network.get("build"), "network.build")
    fetch = _require_str(network.get("fetch"), "network.fetch")
    allowlist = network.get("fetchAllowlist", [])
    if not isinstance(allowlist, list) or len(allowlist) > 32:
        fail("ENVIRONMENT_NETWORK_DENIED", "fetch allowlist is missing or exceeds its bound")
    if build != "NONE":
        fail("ENVIRONMENT_NETWORK_DENIED", "the build/evaluation network must be NONE")
    if fetch != "BROKER":
        fail("ENVIRONMENT_NETWORK_DENIED", "package acquisition must use the broker")
    cleaned: list[str] = []
    for entry in allowlist:
        entry = _require_str(entry, "network.fetchAllowlist entry", 256)
        if (
            "*" in entry
            or "@" in entry
            or any(character.isspace() for character in entry)
            or not (entry.startswith("https://") or entry.startswith("github:"))
        ):
            fail(
                "ENVIRONMENT_NETWORK_DENIED",
                "fetch allowlist entries must be credential-free HTTPS or pinned GitHub sources",
            )
        cleaned.append(entry)
    return {"build": build, "fetch": fetch, "fetchAllowlist": cleaned}


APPROVED_FETCH_NETWORKS: Final[frozenset[str]] = frozenset(
    {
        "none",
        "factory-egress",
        "factory-deps-egress",
        "factory-egress-internal",
        "software-factory_factory-egress",
        "software-factory_factory-deps-egress",
    }
)
BROKER_FETCH_NETWORK = re.compile(r"factory-env-egress-[1-9][0-9]?")


def validate_fetch_policy(request: dict[str, Any], plan: dict[str, Any], phase: str) -> None:
    """Defence in depth for the wrapper/broker fetch contract.

    A fetch run may use only an approved broker network and must carry a
    credential-free proxy plus the exact plan allowlist with broker
    enforcement. Build/check runs may not carry a network at all.
    """
    network = request.get("fetchNetwork", "none")
    if not isinstance(network, str) or (
        network not in APPROVED_FETCH_NETWORKS
        and BROKER_FETCH_NETWORK.fullmatch(network) is None
    ):
        fail("ENVIRONMENT_NETWORK_DENIED", f"fetch network {network!r} is not approved")
    if phase not in ("fetch", "resolve"):
        if network != "none":
            fail("ENVIRONMENT_NETWORK_DENIED", f"{phase} must run with network none")
        return
    if network == "none":
        return
    proxy = request.get("fetchProxy")
    if (
        not isinstance(proxy, str)
        or not re.fullmatch(r"https?://[^@\s]+", proxy)
        or "@" in proxy
    ):
        fail("ENVIRONMENT_NETWORK_DENIED", "fetch requires a credential-free HTTP(S) broker proxy")
    policy = request.get("fetchPolicy")
    if not isinstance(policy, dict) or policy.get("enforcedBy") != "broker":
        fail("ENVIRONMENT_NETWORK_DENIED", "the fetch proxy is not backed by broker allowlist enforcement")
    expected = sorted(_require_list(plan.get("network", {}).get("fetchAllowlist", []), "plan.network.fetchAllowlist"))
    actual = policy.get("allowlist")
    if not isinstance(actual, list) or sorted(str(entry) for entry in actual) != expected:
        fail("ENVIRONMENT_NETWORK_DENIED", "the broker allowlist does not match the frozen plan allowlist")


def validate_sandbox(sandbox: Any) -> None:
    sandbox = _require_mapping(sandbox, "nix.sandbox")
    if (
        _require_bool(sandbox.get("sandbox"), "nix.sandbox.sandbox") is not True
        or _require_bool(sandbox.get("impure"), "nix.sandbox.impure") is not False
        or _require_bool(sandbox.get("privileged"), "nix.sandbox.privileged") is not False
        or _require_bool(sandbox.get("hostNetwork"), "nix.sandbox.hostNetwork") is not False
        or _require_list(sandbox.get("hostMounts", []), "nix.sandbox.hostMounts")
        or _require_list(sandbox.get("hostSockets", []), "nix.sandbox.hostSockets")
    ):
        fail(
            "ENVIRONMENT_SANDBOX_DENIED",
            "the builder must run with sandbox=true, impure=false, no host mount, socket, privilege or host network",
        )


def validate_resolution_plan(
    plan: Any,
    *,
    source_workspace: Path,
    descriptor_image: str,
    descriptor_digest: str,
    descriptor_platform: str,
) -> tuple[dict[str, Any], list[EnvironmentFile], dict[str, int]]:
    """Validate a generated plan before resolving its lock files.

    Resolution happens before Core freezes the spec, so it validates the plan
    itself and the exact builder identity instead of a frozen spec.
    """
    plan = _require_mapping(plan, "plan")
    if _require_int(plan.get("schemaVersion"), "plan.schemaVersion") != SCHEMA_VERSION:
        fail("ENVIRONMENT_SPEC_INVALID", "unsupported environment plan schema version")
    status = _require_str(plan.get("status"), "plan.status", 32)
    if status not in ("CORE_VALIDATED", "LEAD_APPROVED", "FROZEN"):
        fail("ENVIRONMENT_NOT_FROZEN", "the environment plan is not validated for resolution")
    for field in ("planId", "projectId", "requestId", "actionId", "baseCommit"):
        _require_str(plan.get(field), f"plan.{field}", 200)
    if not is_sha256(plan.get("planSha256")):
        fail("ENVIRONMENT_SPEC_INVALID", "plan.planSha256 must be SHA-256")
    if _require_str(plan.get("source"), "plan.source", 32) not in ("GENERATED", "PROJECT_COMMITTED"):
        fail("ENVIRONMENT_PLAN_INVALID", "the environment plan source is invalid")
    builder = _require_mapping(plan.get("builder"), "plan.builder")
    image = _require_str(builder.get("image"), "plan.builder.image", 512)
    digest = _require_str(builder.get("digest"), "plan.builder.digest", 71)
    platform = _require_str(builder.get("platform"), "plan.builder.platform", 64)
    policy_revision = _require_str(builder.get("policyRevision"), "plan.builder.policyRevision", 128)
    if image != descriptor_image:
        fail("ENVIRONMENT_HASH_MISMATCH", "the plan builder image does not match the descriptor")
    if digest != descriptor_digest:
        fail("ENVIRONMENT_HASH_MISMATCH", "the plan builder digest does not match the descriptor")
    if descriptor_platform and platform != descriptor_platform:
        fail("ENVIRONMENT_HASH_MISMATCH", "the plan builder platform does not match the descriptor")
    if policy_revision != BUILDER_POLICY_REVISION:
        fail("ENVIRONMENT_HASH_MISMATCH", "the plan builder policy revision is unsupported")
    nix = _require_mapping(plan.get("nix"), "plan.nix")
    nixpkgs = _require_str(nix.get("nixpkgs"), "plan.nix.nixpkgs", 256)
    if not nixpkgs.startswith(NIXPKGS_PREFIX) or not is_git_revision(
        nixpkgs.removeprefix(NIXPKGS_PREFIX).split("?", 1)[0]
    ):
        fail(
            "ENVIRONMENT_PIN_MISSING",
            "nixpkgs must be pinned as github:NixOS/nixpkgs/<exact-revision>",
        )
    if _require_bool(nix.get("allowUnfree"), "plan.nix.allowUnfree") is not False:
        fail("ENVIRONMENT_LICENSE_DENIED", "allowUnfree must remain false")
    validate_sandbox(nix.get("sandbox"))
    resources = validate_resources(plan.get("resources"))
    validate_network(plan.get("network"))
    for package in plan.get("packages", []):
        if not isinstance(package, dict):
            fail("ENVIRONMENT_PLAN_INVALID", "a package entry is not an object")
        validate_package_source(package)
    files = validate_environment_files(plan, source_workspace)
    validate_workspace_environment_overrides(source_workspace)
    validate_workspace_paths(source_workspace)
    validate_nix_constructs(files)
    validate_secrets(files, dict(os.environ))
    return plan, files, resources


def validate_spec_and_plan(
    spec: Any,
    plan: Any,
    *,
    actual_nix_version: str,
    actual_devenv_version: str,
    descriptor_image: str,
    descriptor_digest: str,
    descriptor_platform: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    spec = _require_mapping(spec, "spec")
    plan = _require_mapping(plan, "plan")
    if _require_int(spec.get("schemaVersion"), "spec.schemaVersion") != SCHEMA_VERSION:
        fail("ENVIRONMENT_SPEC_INVALID", "unsupported environment spec schema version")
    if _require_int(plan.get("schemaVersion"), "plan.schemaVersion") != SCHEMA_VERSION:
        fail("ENVIRONMENT_SPEC_INVALID", "unsupported environment plan schema version")
    if _require_str(spec.get("status"), "spec.status") != "FROZEN":
        fail("ENVIRONMENT_NOT_FROZEN", "the environment spec is not FROZEN")
    if _require_str(plan.get("status"), "plan.status") != "FROZEN":
        fail("ENVIRONMENT_NOT_FROZEN", "the environment plan is not FROZEN")

    for field in ("specId", "projectId", "requestId", "actionId", "baseCommit", "planId"):
        _require_str(spec.get(field), f"spec.{field}", 200)
    _require_str(plan.get("planId"), "plan.planId", 200)
    if not is_sha256(spec.get("specSha256")):
        fail("ENVIRONMENT_SPEC_INVALID", "spec.specSha256 must be SHA-256")
    if not is_sha256(plan.get("planSha256")):
        fail("ENVIRONMENT_SPEC_INVALID", "plan.planSha256 must be SHA-256")
    if spec.get("planSha256") != plan.get("planSha256"):
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec does not bind the supplied plan hash")
    for field in ("projectId", "requestId", "actionId", "baseCommit", "planId"):
        if spec.get(field) != plan.get(field):
            fail("ENVIRONMENT_HASH_MISMATCH", f"spec.{field} does not match the plan")

    platform = _require_str(spec.get("platform"), "spec.platform", 64)
    if platform != DEFAULT_PLATFORM:
        fail(
            "ENVIRONMENT_UNSUPPORTED",
            f"builder platform {platform} is not supported by the current policy",
        )
    if descriptor_platform and platform != descriptor_platform:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec platform does not match the builder descriptor")

    nix_version = _require_str(spec.get("nixVersion"), "spec.nixVersion", 64)
    devenv_version = _require_str(spec.get("devenvVersion"), "spec.devenvVersion", 64)
    if nix_version != actual_nix_version:
        fail(
            "ENVIRONMENT_HASH_MISMATCH",
            f"the spec pins Nix {nix_version} but the builder provides {actual_nix_version}",
        )
    if devenv_version != actual_devenv_version:
        fail(
            "ENVIRONMENT_HASH_MISMATCH",
            f"the spec pins devenv {devenv_version} but the builder provides {actual_devenv_version}",
        )

    image = _require_str(spec.get("builderImage"), "spec.builderImage", 512)
    digest = _require_str(spec.get("builderImageDigest"), "spec.builderImageDigest", 71)
    if not image or not is_digest(digest):
        fail("ENVIRONMENT_BUILDER_UNAVAILABLE", "the frozen spec has no digest-pinned builder image")
    if descriptor_image and image != descriptor_image:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec builder image does not match the descriptor")
    if descriptor_digest and digest != descriptor_digest:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec builder digest does not match the descriptor")

    builder = _require_mapping(plan.get("builder"), "plan.builder")
    if (
        builder.get("image") != image
        or builder.get("digest") != digest
        or builder.get("platform") != platform
        or builder.get("policyRevision") != BUILDER_POLICY_REVISION
    ):
        fail("ENVIRONMENT_HASH_MISMATCH", "the plan builder descriptor does not match the spec")

    nix = _require_mapping(plan.get("nix"), "plan.nix")
    nixpkgs = _require_str(nix.get("nixpkgs"), "plan.nix.nixpkgs", 256)
    if not nixpkgs.startswith(NIXPKGS_PREFIX) or not is_git_revision(
        nixpkgs.removeprefix(NIXPKGS_PREFIX).split("?", 1)[0]
    ):
        fail(
            "ENVIRONMENT_PIN_MISSING",
            "nixpkgs must be pinned as github:NixOS/nixpkgs/<exact-revision>",
        )
    if _require_bool(nix.get("allowUnfree"), "plan.nix.allowUnfree") is not False:
        fail(
            "ENVIRONMENT_LICENSE_DENIED",
            "allowUnfree must remain false; an unfree package requires an exact owner override",
        )
    validate_sandbox(nix.get("sandbox"))
    inputs = _require_list(nix.get("inputs", []), "plan.nix.inputs")
    if len(inputs) > MAX_INPUTS:
        fail("ENVIRONMENT_PLAN_INVALID", "plan.nix.inputs exceeds its bound")
    for entry in inputs:
        entry = _require_mapping(entry, "plan.nix.inputs entry")
        name = _require_str(entry.get("name"), "plan.nix.inputs.name", 100)
        url = _require_str(entry.get("url"), "plan.nix.inputs.url", 512)
        if not re.fullmatch(r"[A-Za-z0-9_.-]+", name):
            fail("ENVIRONMENT_PLAN_INVALID", "a Nix input name is not a safe identifier")
        validate_pinned_url(url, "plan.nix.inputs.url", allow_follows=False)

    plan_resources = validate_resources(plan.get("resources"))
    spec_resources = validate_resources(spec.get("resourcePolicy"))
    if plan_resources != spec_resources:
        fail("ENVIRONMENT_RESOURCE_LIMIT", "plan resources do not match the frozen spec resource policy")
    plan_network = validate_network(plan.get("network"))
    spec_network = validate_network(spec.get("networkPolicy"))
    if plan_network != spec_network:
        fail("ENVIRONMENT_NETWORK_DENIED", "plan network policy does not match the frozen spec network policy")
    packages = _require_list(plan.get("packages", []), "plan.packages")
    if len(packages) > MAX_PACKAGES:
        fail("ENVIRONMENT_PLAN_INVALID", "plan.packages exceeds its bound")
    for package in packages:
        package = _require_mapping(package, "plan.packages entry")
        _require_str(package.get("name"), "plan.packages.name", 200)
        _require_str(package.get("version"), "plan.packages.version", 64)
        _require_str(package.get("license"), "plan.packages.license", 200)
    return spec, plan


def validate_pinned_url(url: str, label: str, *, allow_follows: bool) -> None:
    if allow_follows and "follows" in url:
        return
    if GITHUB_REVISION.fullmatch(url):
        return
    if url.startswith("git+https://") and LOCKED_REVISION.search(url):
        return
    if url.startswith("https://") and "sha256-" in url:
        return
    fail(
        "ENVIRONMENT_PIN_MISSING",
        f"{label} must be an exact GitHub revision, a git URL with rev=, or a hash-pinned archive",
    )


def validate_environment_files(plan: dict[str, Any], source_workspace: Path) -> list[EnvironmentFile]:
    entries = _require_list(plan.get("files"), "plan.files")
    if not entries:
        fail("ENVIRONMENT_PLAN_INVALID", "the environment plan has no environment files")
    if len(entries) > MAX_ENVIRONMENT_FILES:
        fail("ENVIRONMENT_PLAN_INVALID", "the environment plan has too many environment files")
    by_path: dict[str, EnvironmentFile] = {}
    for raw in entries:
        entry = _require_mapping(raw, "plan.files entry")
        try:
            path = safe_relative_path(entry.get("path"))
        except ValueError as error:
            fail("ENVIRONMENT_PLAN_INVALID", f"environment file path: {error}")
        if path in by_path:
            fail("ENVIRONMENT_PLAN_INVALID", f"duplicate environment file path {path}")
        origin = _require_str(entry.get("origin"), f"{path}.origin", 32)
        if origin not in ("PROJECT", "GENERATED"):
            fail("ENVIRONMENT_PLAN_INVALID", f"environment file {path} has an unknown origin")
        expected = _require_str(entry.get("sha256"), f"{path}.sha256", 64)
        if not is_sha256(expected):
            fail("ENVIRONMENT_PIN_MISSING", f"environment file {path} has no SHA-256")
        if origin == "GENERATED":
            content = entry.get("content")
            if not isinstance(content, str):
                fail("ENVIRONMENT_PLAN_INVALID", f"generated environment file {path} has no content")
            data = content.encode("utf-8")
        else:
            source = source_workspace / path
            if not source.is_file():
                fail("ENVIRONMENT_PLAN_INVALID", f"project environment file {path} is missing")
            data = source.read_bytes()
        if len(data) > MAX_ENVIRONMENT_FILE_BYTES:
            fail("ENVIRONMENT_PLAN_INVALID", f"environment file {path} exceeds its byte bound")
        actual = sha256_bytes(data)
        if actual != expected:
            fail(
                "ENVIRONMENT_HASH_MISMATCH",
                f"environment file {path} does not match the frozen hash",
            )
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError:
            fail("ENVIRONMENT_PLAN_INVALID", f"environment file {path} is not UTF-8 text")
        by_path[path] = EnvironmentFile(path, expected, origin, text)

    names = {Path(path).name for path in by_path}
    if "flake.nix" not in names and "devenv.nix" not in names:
        fail(
            "ENVIRONMENT_PLAN_INVALID",
            "the environment plan must contain a flake.nix or devenv.nix",
        )
    return [by_path[path] for path in sorted(by_path)]


def validate_nix_constructs(files: Iterable[EnvironmentFile]) -> None:
    for file in files:
        if not file.path.endswith((".nix", ".yaml", ".yml")):
            continue
        for pattern, code, label in FORBIDDEN_NIX_PATTERNS:
            if pattern.search(file.content):
                # fetch* and currentSystem need a pin check, not an automatic
                # rejection when the exact hash/rev is present.
                if label in ("fetchTarball", "fetchGit", "fetchurl", "fetchTree"):
                    _validate_fetch_use(file, label, code)
                    continue
                if label == "builtins.currentSystem" and file.path.endswith("flake.nix"):
                    # Flakes evaluate without currentSystem; a project file that
                    # needs it is not reproducible and must be rejected.
                    fail(code, f"{file.path} uses {label}, which is not reproducible")
                fail(code, f"{file.path} contains forbidden construct {label}")
        if file.path.endswith(("devenv.yaml", "devenv.yml")):
            for name, value in YAML_BOOL.findall(file.content):
                if name in ("impure", "sandbox") and value == "true" and name == "impure":
                    fail("ENVIRONMENT_SANDBOX_DENIED", f"{file.path} enables impure evaluation")
                if name == "sandbox" and value == "false":
                    fail("ENVIRONMENT_SANDBOX_DENIED", f"{file.path} disables the Nix sandbox")
                if name == "allowUnfree" and value == "true":
                    fail("ENVIRONMENT_LICENSE_DENIED", f"{file.path} enables unfree packages")
        if file.path.endswith("flake.nix"):
            for url in INPUT_URL.findall(file.content):
                if url.startswith("path:"):
                    fail("ENVIRONMENT_SANDBOX_DENIED", "a flake input points at a host path")
                if url.startswith(("github:", "git+https://", "https://")):
                    validate_pinned_url(url, f"{file.path} input", allow_follows=False)
        if file.path.endswith(("devenv.yaml", "devenv.yml")):
            for url in YAML_INPUT_URL.findall(file.content):
                if url.startswith("path:"):
                    fail("ENVIRONMENT_SANDBOX_DENIED", "a devenv input points at a host path")
                validate_pinned_url(url, f"{file.path} input", allow_follows=False)


def _validate_fetch_use(file: EnvironmentFile, label: str, code: str) -> None:
    """Allow a fetch only when the same expression carries an exact hash/rev."""
    for match in re.finditer(rf"\b{re.escape(label)}\b", file.content):
        window = file.content[match.start() : match.start() + 2_000]
        if label in ("fetchTarball", "fetchurl"):
            if not re.search(r"\bsha256\s*=\s*[\"'][A-Za-z0-9+/=_-]{20,}[\"']", window):
                fail(code, f"{file.path} has an unpinned {label}")
        else:
            if not (
                re.search(r"\brev\s*=\s*[\"'][0-9a-f]{40,64}[\"']", window)
                or re.search(r"\bnarHash\s*=\s*[\"']sha256-[A-Za-z0-9+/=_-]+[\"']", window)
            ):
                fail(code, f"{file.path} has an unpinned {label}")


def validate_lock_pins(
    files: Iterable[EnvironmentFile],
    plan: dict[str, Any],
    spec: dict[str, Any],
) -> None:
    by_name = {Path(file.path).name: file for file in files}
    flake_lock = by_name.get("flake.lock")
    devenv_lock = by_name.get("devenv.lock")
    if flake_lock is None and devenv_lock is None:
        fail("ENVIRONMENT_PIN_MISSING", "the environment has no flake.lock or devenv.lock")
    expected_flake = spec.get("flakeLockSha256") or ""
    expected_devenv = spec.get("devenvLockSha256") or ""
    if flake_lock is not None:
        if not is_sha256(expected_flake) or expected_flake != flake_lock.sha256:
            fail("ENVIRONMENT_HASH_MISMATCH", "flake.lock does not match the frozen spec")
        _validate_lock_document(flake_lock, plan, "flake.lock")
    elif expected_flake:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec requires a flake.lock that is not present")
    if devenv_lock is not None:
        if not is_sha256(expected_devenv) or expected_devenv != devenv_lock.sha256:
            fail("ENVIRONMENT_HASH_MISMATCH", "devenv.lock does not match the frozen spec")
        _validate_lock_document(devenv_lock, plan, "devenv.lock")
    elif expected_devenv:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec requires a devenv.lock that is not present")


def _validate_lock_document(file: EnvironmentFile, plan: dict[str, Any], label: str) -> None:
    try:
        document = json.loads(file.content)
    except json.JSONDecodeError:
        fail("ENVIRONMENT_PIN_MISSING", f"{label} is not valid JSON")
    nodes = document.get("nodes")
    if not isinstance(nodes, dict) or not nodes:
        fail("ENVIRONMENT_PIN_MISSING", f"{label} has no nodes")
    if len(nodes) > MAX_INPUTS + 4:
        fail("ENVIRONMENT_PLAN_INVALID", f"{label} exceeds its input bound")
    plan_inputs = {
        str(entry.get("name")) for entry in plan.get("nix", {}).get("inputs", []) if isinstance(entry, dict)
    }
    nixpkgs = str(plan.get("nix", {}).get("nixpkgs", ""))
    nixpkgs_rev = nixpkgs.removeprefix(NIXPKGS_PREFIX).split("?", 1)[0]
    root_inputs = set()
    for name, node in nodes.items():
        if not isinstance(node, dict):
            fail("ENVIRONMENT_PIN_MISSING", f"{label} node {name} is invalid")
        if name == "root":
            inputs = node.get("inputs")
            if isinstance(inputs, dict):
                root_inputs = set(inputs)
            continue
        locked = node.get("locked")
        if not isinstance(locked, dict):
            fail("ENVIRONMENT_PIN_MISSING", f"{label} node {name} has no locked data")
        nar_hash = locked.get("narHash")
        if nar_hash is not None and not str(nar_hash).startswith("sha256-"):
            fail("ENVIRONMENT_PIN_MISSING", f"{label} node {name} has an invalid narHash")
        kind = str(locked.get("type", ""))
        if kind == "github":
            rev = locked.get("rev")
            if not is_git_revision(rev):
                fail("ENVIRONMENT_PIN_MISSING", f"{label} node {name} has no exact revision")
            if locked.get("repo") == "nixpkgs" and rev != nixpkgs_rev:
                fail("ENVIRONMENT_HASH_MISMATCH", f"{label} nixpkgs does not match the plan")
        elif kind == "git":
            rev = locked.get("rev")
            if not is_git_revision(rev):
                fail("ENVIRONMENT_PIN_MISSING", f"{label} node {name} has no exact revision")
        elif kind == "tarball":
            url = str(locked.get("url", ""))
            if not url.startswith("https://") or not str(locked.get("narHash", "")).startswith("sha256-"):
                fail("ENVIRONMENT_PIN_MISSING", f"{label} node {name} has no pinned tarball")
        else:
            fail("ENVIRONMENT_UNSUPPORTED", f"{label} uses an unsupported input type {kind!r}")
    if plan_inputs and not plan_inputs.issubset(root_inputs | {"nixpkgs", "devenv"}):
        fail("ENVIRONMENT_PIN_MISSING", f"{label} contains an input outside the plan policy")
    # devenv's own module input is pinned by the devenv lock and is not a
    # project-selected package. It still must be a GitHub revision.


CORE_LICENSE_ENTRY_FIELDS: Final[tuple[str, ...]] = (
    "name",
    "version",
    "license",
    "source",
    "sha256",
    "unfree",
    "ownerOverrideId",
    "attribution",
)


def core_entries_sha256(entries: list[Any]) -> str:
    """Match Core's ``LicenseInventory::compute_sbom_sha256``.

    Core hashes the serde_json serialization of the ordered ``LicenseEntry``
    structs. Field order matters; sorting keys would produce a different hash
    from the frozen spec.
    """
    ordered: list[dict[str, Any]] = []
    for entry in entries:
        if not isinstance(entry, dict):
            fail("ENVIRONMENT_LICENSE_UNKNOWN", "a license inventory entry is not an object")
        normalized: dict[str, Any] = {}
        for field in CORE_LICENSE_ENTRY_FIELDS:
            if field == "ownerOverrideId":
                normalized[field] = entry.get(field)
            elif field == "unfree":
                normalized[field] = bool(entry.get(field, False))
            elif field in ("sha256", "attribution"):
                normalized[field] = str(entry.get(field, ""))
            else:
                normalized[field] = str(entry.get(field, ""))
        ordered.append(normalized)
    return sha256_text(
        json.dumps(ordered, ensure_ascii=False, separators=(",", ":"), sort_keys=False)
    )


def validate_sbom_binding(spec: dict[str, Any], plan: dict[str, Any]) -> str:
    """Require the frozen SBOM hash to match the exact inventory entries.

    A missing expectation or a generated hash that differs from the frozen
    ``spec.sbomSha256`` is a blocker. The builder never reports SUCCEEDED for a
    different SBOM.
    """
    expected = spec.get("sbomSha256")
    if not is_sha256(expected):
        fail("ENVIRONMENT_HASH_MISMATCH", "the frozen spec has no valid SBOM hash")
    inventory = _require_mapping(plan.get("licenseInventory"), "plan.licenseInventory")
    entries = _require_list(inventory.get("entries"), "plan.licenseInventory.entries")
    computed = core_entries_sha256(entries)
    if expected != computed:
        fail(
            "ENVIRONMENT_HASH_MISMATCH",
            "the generated SBOM does not match the frozen spec SBOM hash",
        )
    if inventory.get("sbomSha256") != expected:
        fail("ENVIRONMENT_HASH_MISMATCH", "the license inventory SBOM hash is inconsistent")
    expected_inventory = spec.get("licenseInventorySha256")
    if not is_sha256(expected_inventory) or inventory.get("inventorySha256") != expected_inventory:
        fail("ENVIRONMENT_HASH_MISMATCH", "the frozen license inventory hash is inconsistent")
    return computed


def validate_license_expression(value: str) -> None:
    if not isinstance(value, str):
        fail("ENVIRONMENT_LICENSE_UNKNOWN", "a selected package has no license expression")
    value = value.strip()
    if not value or value in ("NONE", "NOASSERTION", "UNLICENSED", "UNKNOWN"):
        fail("ENVIRONMENT_LICENSE_UNKNOWN", f"the license expression {value!r} is unknown")
    if value in DENIED_SPDX:
        fail("ENVIRONMENT_LICENSE_DENIED", f"the license expression {value} is denied by policy")
    if value in ALLOWED_SPDX:
        return
    normalized = (
        value.replace("(", " ")
        .replace(")", " ")
        .replace(" AND ", " ")
        .replace(" OR ", " ")
        .replace(" WITH ", " ")
    )
    atoms = normalized.split()
    if atoms and all(
        atom in ALLOWED_SPDX or atom.startswith("LLVM-exception") for atom in atoms
    ):
        return
    fail("ENVIRONMENT_LICENSE_UNKNOWN", f"the license expression {value} is unknown")


def _closure_license_known(value: str) -> bool:
    if value in DENIED_SPDX:
        return False
    if value in CLOSURE_ALLOWED_SPDX:
        return True
    normalized = (
        value.replace("(", " ")
        .replace(")", " ")
        .replace(" AND ", " ")
        .replace(" OR ", " ")
        .replace(" WITH ", " ")
    )
    atoms = normalized.split()
    return bool(atoms) and all(
        atom in CLOSURE_ALLOWED_SPDX or atom.startswith("LLVM-exception")
        for atom in atoms
    )


def validate_closure_licenses(entries: Any) -> list[dict[str, Any]]:
    """Fail closed on any unfree, denied or unknown package in the realised
    environment closure. This evidence comes from the builder's independent
    closure metadata evaluation, not from the Core plan's package claims."""
    entries = _require_list(entries, "closure license entries")
    if len(entries) > MAX_CLOSURE_PACKAGES:
        fail("ENVIRONMENT_LICENSE_UNKNOWN", "the environment closure exceeds its package bound")
    observed: list[dict[str, Any]] = []
    for entry in entries:
        entry = _require_mapping(entry, "closure license entry")
        path = _require_str(entry.get("path"), "closure license path", 512)
        if not path.startswith("/nix/store/"):
            fail("ENVIRONMENT_LICENSE_UNKNOWN", "a closure license path is not a Nix store path")
        name = _require_str(entry.get("name"), "closure license name", 200)
        version = _require_str(entry.get("version"), "closure license version", 128)
        license_value = _require_str(entry.get("license"), "closure license expression", 200)
        unfree = _require_bool(entry.get("unfree"), "closure license unfree")
        if unfree:
            fail(
                "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
                f"transitive closure package {name} {version} is unfree",
            )
        if license_value in DENIED_SPDX:
            fail(
                "ENVIRONMENT_LICENSE_DENIED",
                f"transitive closure package {name} {version} uses denied license {license_value}",
            )
        if not _closure_license_known(license_value):
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                f"transitive closure package {name} {version} has unknown license {license_value}",
            )
        observed.append(
            {
                "path": path,
                "name": name,
                "version": version,
                "license": license_value,
                "unfree": unfree,
            }
        )
    return observed


PACKAGE_ATTR = re.compile(r"^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*$")


def validate_package_source(package: dict[str, Any]) -> None:
    """Require the exact pinned nixpkgs attribute, not an opaque suffix."""
    name = str(package.get("name", "")).strip()
    attr = str(package.get("attr", "")).strip() or name
    if not name or len(attr) > 128 or not PACKAGE_ATTR.fullmatch(attr):
        fail("ENVIRONMENT_PLAN_INVALID", "a package nix attribute is not a bounded dotted identifier")
    source = str(package.get("source", ""))
    if source not in ("nixpkgs", f"nixpkgs:{attr}"):
        fail(
            "ENVIRONMENT_PIN_MISSING",
            f"package {name} source must be exactly nixpkgs or nixpkgs:{attr}",
        )


def validate_license_inventory(
    spec: dict[str, Any],
    plan: dict[str, Any],
) -> dict[str, Any]:
    inventory = _require_mapping(plan.get("licenseInventory"), "plan.licenseInventory")
    if _require_bool(inventory.get("allowUnfree"), "plan.licenseInventory.allowUnfree") is not False:
        fail("ENVIRONMENT_LICENSE_DENIED", "the license inventory must keep allowUnfree=false")
    if inventory.get("policyRevision") != LICENSE_POLICY_REVISION:
        fail("ENVIRONMENT_LICENSE_UNKNOWN", "the license inventory uses an unknown policy revision")
    blockers = _require_list(inventory.get("blockers", []), "plan.licenseInventory.blockers")
    if blockers:
        first = blockers[0] if isinstance(blockers[0], dict) else {}
        code = first.get("code")
        if code not in (
            "ENVIRONMENT_LICENSE_BLOCKED",
            "ENVIRONMENT_LICENSE_DENIED",
            "ENVIRONMENT_LICENSE_UNKNOWN",
            "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
        ):
            code = "ENVIRONMENT_LICENSE_BLOCKED"
        fail(code, str(first.get("message", "the license gate blocked the plan")))

    entries = _require_list(inventory.get("entries", []), "plan.licenseInventory.entries")
    by_package: dict[tuple[str, str], dict[str, Any]] = {}
    for entry in entries:
        entry = _require_mapping(entry, "license entry")
        name = _require_str(entry.get("name"), "license entry name", 200)
        version = _require_str(entry.get("version"), "license entry version", 64)
        validate_license_expression(_require_str(entry.get("license"), "license entry license", 200))
        if entry.get("unfree") and not entry.get("ownerOverrideId"):
            fail(
                "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
                f"{name} {version} is unfree and the inventory has no owner override id",
            )
        by_package[(name.lower(), version)] = entry

    overrides = _require_list(inventory.get("overrides", []), "plan.licenseInventory.overrides")
    for package in plan.get("packages", []):
        if not isinstance(package, dict):
            fail("ENVIRONMENT_PLAN_INVALID", "a package entry is not an object")
        name = str(package.get("name", ""))
        version = str(package.get("version", ""))
        license_value = str(package.get("license", ""))
        validate_package_source(package)
        validate_license_expression(license_value)
        if package.get("unfree"):
            matching = [
                override
                for override in overrides
                if isinstance(override, dict)
                and str(override.get("package", "")).lower() == name.lower()
                and str(override.get("version", "")) == version
                and str(override.get("license", "")) == license_value
                and len(str(override.get("reason", "")).strip()) >= 8
                and str(override.get("decisionId", "")).strip()
            ]
            if not matching:
                fail(
                    "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
                    f"{name} {version} is unfree and has no exact owner override",
                )
        entry = by_package.get((name.lower(), version))
        if entry is None:
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                f"{name} {version} has no license inventory entry",
            )
        if entry.get("license") != license_value:
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                f"{name} {version} has a contradictory license expression",
            )

    inventory_sha = inventory.get("inventorySha256")
    sbom_sha = inventory.get("sbomSha256")
    if not is_sha256(inventory_sha) or not is_sha256(sbom_sha):
        fail("ENVIRONMENT_LICENSE_UNKNOWN", "the license inventory has no content hashes")
    if spec.get("licenseInventorySha256") != inventory_sha:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec does not bind the license inventory hash")
    if spec.get("sbomSha256") != sbom_sha:
        fail("ENVIRONMENT_HASH_MISMATCH", "the spec does not bind the SBOM hash")
    if plan.get("licenseInventorySha256") not in (None, "", inventory_sha):
        fail("ENVIRONMENT_HASH_MISMATCH", "the plan license inventory hash is inconsistent")
    return inventory


def validate_secrets(files: Iterable[EnvironmentFile], environ: dict[str, str]) -> None:
    for file in files:
        for pattern in SECRET_PATTERNS:
            if pattern.search(file.content):
                fail(
                    "ENVIRONMENT_SECRET_DETECTED",
                    f"environment file {file.path} contains a secret-shaped value",
                )
    allowed = {
        "PATH",
        "HOME",
        "TMPDIR",
        "TMP",
        "PWD",
        "OLDPWD",
        "SHLVL",
        "_",
        "LANG",
        "LC_ALL",
        "TERM",
        "HOSTNAME",
        "NIX_CONFIG",
        "NIX_PATH",
        "NIX_REMOTE",
        "NIX_USER_CONF_FILES",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "DEVENV_ROOT",
        "DEVENV_STATE",
        "DEVENV_DOTFILE",
        "DEVENV_OFFLINE",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "CURL_CA_BUNDLE",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "npm_config_cache",
        "npm_config_offline",
        "npm_config_proxy",
        "npm_config_https_proxy",
        "PLAYWRIGHT_BROWSERS_PATH",
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD",
        "CHROMIUM_EXE",
        "FACTORY_PLAYWRIGHT_CORE",
        "FACTORY_ENVIRONMENT_BUILDER_IMAGE",
        "FACTORY_ENVIRONMENT_BUILDER_DIGEST",
        "FACTORY_ENVIRONMENT_BUILDER_PLATFORM",
        "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION",
        "FACTORY_ENVIRONMENT_NIX_VERSION",
        "FACTORY_ENVIRONMENT_DEVENV_VERSION",
        "FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID",
    }
    for name, value in environ.items():
        upper = name.upper()
        if any(token in upper for token in ("TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "PRIVATE_KEY")):
            if name not in allowed:
                fail("ENVIRONMENT_SECRET_DETECTED", f"the builder environment contains {name}")
        if name in allowed:
            continue
        for pattern in SECRET_PATTERNS:
            if pattern.search(value):
                fail("ENVIRONMENT_SECRET_DETECTED", f"the builder environment value {name} is secret-shaped")


def validate_workspace_paths(root: Path) -> None:
    if not root.is_dir():
        fail("ENVIRONMENT_PLAN_INVALID", "the project workspace is missing")
    ignored = {".git", ".devenv", "node_modules", "target", ".cache"}
    for entry in root.rglob("*"):
        if ignored.intersection(entry.relative_to(root).parts):
            continue
        if entry.is_symlink():
            target = entry.resolve()
            try:
                target.relative_to(root.resolve())
            except ValueError:
                fail("ENVIRONMENT_PLAN_INVALID", f"workspace symlink {entry} escapes the project")
        if entry.is_file() and entry.stat().st_size > 128 * 1024 * 1024:
            # Large binary assets are not needed to evaluate an environment.
            # Do not fail; they are copied and bounded by disk policy.
            continue


def validate_workspace_environment_overrides(root: Path) -> None:
    forbidden = (
        "devenv.local.nix",
        "devenv.local.yaml",
        "devenv.local.yml",
        "flake.local.nix",
        ".envrc",
    )
    for name in forbidden:
        if (root / name).exists() or (root / name).is_symlink():
            fail(
                "ENVIRONMENT_SANDBOX_DENIED",
                f"the project workspace contains a forbidden environment override {name}",
            )


def validate_check(check: Any) -> dict[str, Any]:
    check = _require_mapping(check, "check")
    check_id = _require_str(check.get("checkId"), "check.checkId", 200)
    argv = _require_list(check.get("argv"), "check.argv")
    if not argv or len(argv) > MAX_ARGV:
        fail("ENVIRONMENT_PLAN_INVALID", f"check {check_id} has an invalid argv vector")
    for arg in argv:
        if not isinstance(arg, str) or len(arg) > MAX_ARG_CHARS or "\x00" in arg:
            fail("ENVIRONMENT_PLAN_INVALID", f"check {check_id} has an invalid argv value")
    cwd = _require_str(check.get("cwd", "."), "check.cwd", 512)
    if cwd != ".":
        try:
            safe_relative_path(cwd)
        except ValueError as error:
            fail("ENVIRONMENT_PLAN_INVALID", f"check {check_id} cwd: {error}")
    timeout = _require_int(check.get("timeoutSecs"), "check.timeoutSecs")
    if not MIN_TIMEOUT_SECS <= timeout <= MAX_TIMEOUT_SECS:
        fail("ENVIRONMENT_RESOURCE_LIMIT", f"check {check_id} timeout is outside policy")
    expect_exit = _require_int(check.get("expectExit", 0), "check.expectExit")
    if not -255 <= expect_exit <= 255:
        fail("ENVIRONMENT_PLAN_INVALID", f"check {check_id} has an invalid expected exit")
    return {
        "checkId": check_id,
        "argv": argv,
        "cwd": cwd,
        "timeoutSecs": timeout,
        "expectExit": expect_exit,
    }


def validate_environment_digest(spec: dict[str, Any], plan: dict[str, Any], user: str = "") -> str:
    """Compute a builder-local content hash for the exact validated inputs.

    Core's ``specSha256`` is authoritative and is bound verbatim. This digest is
    additional evidence over the exact files, pins, policy and user that the
    builder actually consumed.
    """
    content = {
        "schemaVersion": SCHEMA_VERSION,
        "specSha256": spec.get("specSha256"),
        "planSha256": plan.get("planSha256"),
        "platform": spec.get("platform"),
        "nixVersion": spec.get("nixVersion"),
        "devenvVersion": spec.get("devenvVersion"),
        "nixpkgs": plan.get("nix", {}).get("nixpkgs"),
        "inputs": plan.get("nix", {}).get("inputs", []),
        "files": [
            {"path": entry.get("path"), "sha256": entry.get("sha256"), "origin": entry.get("origin")}
            for entry in plan.get("files", [])
        ],
        "packages": plan.get("packages", []),
        "licenses": plan.get("licenseInventory", {}).get("inventorySha256"),
        "sbom": plan.get("licenseInventory", {}).get("sbomSha256"),
        "builderPolicyRevision": BUILDER_POLICY_REVISION,
        "user": user,
    }
    return sha256_text(canonical_json(content))


def environment_without_secrets(environ: dict[str, str]) -> dict[str, str]:
    """A deliberately narrow environment for Nix/devenv child processes."""
    keep = {
        "PATH",
        "HOME",
        "TMPDIR",
        "TMP",
        "LANG",
        "LC_ALL",
        "TERM",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "CURL_CA_BUNDLE",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "NIX_CONFIG",
        "NIX_PATH",
        "NIX_REMOTE",
        "NIX_USER_CONF_FILES",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "DEVENV_ROOT",
        "DEVENV_STATE",
        "DEVENV_DOTFILE",
        "DEVENV_OFFLINE",
        "npm_config_cache",
        "npm_config_offline",
        "npm_config_proxy",
        "npm_config_https_proxy",
        "PLAYWRIGHT_BROWSERS_PATH",
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD",
        "CHROMIUM_EXE",
        "FACTORY_PLAYWRIGHT_CORE",
        "FACTORY_ENVIRONMENT_BUILDER_IMAGE",
        "FACTORY_ENVIRONMENT_BUILDER_DIGEST",
        "FACTORY_ENVIRONMENT_BUILDER_PLATFORM",
        "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION",
        "FACTORY_ENVIRONMENT_NIX_VERSION",
        "FACTORY_ENVIRONMENT_DEVENV_VERSION",
        "FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID",
    }
    return {name: value for name, value in environ.items() if name in keep}
