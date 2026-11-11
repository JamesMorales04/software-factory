"""Command line used inside the builder image.

Subcommands:
  descriptor  print the EnvironmentBuilderDescriptor
  build       validate + fetch/build the frozen environment and write a receipt
  check       run one SpecCheck inside the frozen environment
  self-test   deterministic policy smoke test with no Nix build
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

from . import BUILDER_POLICY_REVISION, DEFAULT_PLATFORM, SCHEMA_VERSION
from .disk import DiskBudget, builder_disk_budget
from .errors import BuilderError, fail
from .nix import (
    BuildLog,
    child_environment,
    closure_manifest,
    closure_manifest_sha256,
    devenv_version,
    environment_digest,
    nix_eval_json,
    nix_version,
    prepare_workspace,
    realize_environment,
    run_check,
    run_logged,
    write_environment_files,
)
from .policy import (
    MAX_ENVIRONMENT_FILE_BYTES,
    validate_check,
    validate_environment_digest,
    validate_fetch_policy,
    validate_environment_files,
    validate_license_inventory,
    validate_lock_pins,
    validate_nix_constructs,
    validate_package_source,
    validate_resolution_plan,
    validate_resources,
    validate_sandbox,
    validate_sbom_binding,
    validate_secrets,
    validate_spec_and_plan,
    validate_workspace_environment_overrides,
    validate_workspace_paths,
)
from .receipt import build_receipt, observe_attestation
from .sbom import build_sbom, license_inventory_copy, sbom_sha256, sbom_text
from .util import now_rfc3339, read_json, sha256_text, tail, write_json

CACHE_ROOT = Path("/cache")


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def descriptor() -> dict[str, Any]:
    image = _env("FACTORY_ENVIRONMENT_BUILDER_IMAGE")
    digest = _env("FACTORY_ENVIRONMENT_BUILDER_DIGEST")
    platform = _env("FACTORY_ENVIRONMENT_BUILDER_PLATFORM", DEFAULT_PLATFORM)
    policy = _env("FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION", BUILDER_POLICY_REVISION)
    status = "READY" if image and digest else "UNAVAILABLE"
    return {
        "image": image,
        "digest": digest,
        "platform": platform,
        "policyRevision": policy,
        "status": status,
    }


def _actual_versions() -> tuple[str, str]:
    return nix_version(), devenv_version()


def _load_request(path: str) -> dict[str, Any]:
    try:
        if path == "-":
            value = json.load(sys.stdin)
        else:
            value = read_json(Path(path))
    except (OSError, json.JSONDecodeError) as error:
        fail("ENVIRONMENT_SPEC_INVALID", f"cannot read the build request: {error}")
    if not isinstance(value, dict):
        fail("ENVIRONMENT_SPEC_INVALID", "the build request must be an object")
    if value.get("schemaVersion", SCHEMA_VERSION) != SCHEMA_VERSION:
        fail("ENVIRONMENT_SPEC_INVALID", "unsupported builder request schema version")
    return value


def _run_dir(request: dict[str, Any], explicit: str | None) -> Path:
    raw = explicit or request.get("runDir") or request.get("outputDir")
    if not raw:
        fail("ENVIRONMENT_SPEC_INVALID", "the build request has no output directory")
    path = Path(str(raw))
    if not path.is_absolute():
        fail("ENVIRONMENT_SPEC_INVALID", "the output directory must be absolute")
    path.mkdir(parents=True, exist_ok=True)
    return path


def _workspace(request: dict[str, Any], explicit: str | None) -> Path:
    raw = explicit or request.get("workspace") or "/inputs/workspace"
    path = Path(str(raw))
    if not path.is_absolute():
        fail("ENVIRONMENT_SPEC_INVALID", "the project workspace must be absolute")
    return path


def _validate_request(
    request: dict[str, Any],
    source_workspace: Path,
) -> tuple[dict[str, Any], dict[str, Any], list[Any], dict[str, int], str, str]:
    spec = request.get("spec")
    plan = request.get("plan")
    actual_nix, actual_devenv = _actual_versions()
    desc = descriptor()
    spec, plan = validate_spec_and_plan(
        spec,
        plan,
        actual_nix_version=actual_nix,
        actual_devenv_version=actual_devenv,
        descriptor_image=desc["image"],
        descriptor_digest=desc["digest"],
        descriptor_platform=desc["platform"],
    )
    files = validate_environment_files(plan, source_workspace)
    validate_workspace_environment_overrides(source_workspace)
    validate_workspace_paths(source_workspace)
    validate_nix_constructs(files)
    validate_lock_pins(files, plan, spec)
    validate_secrets(files, dict(os.environ))
    validate_license_inventory(spec, plan)
    frozen_sbom_sha256 = validate_sbom_binding(spec, plan)
    validate_sandbox(plan.get("nix", {}).get("sandbox"))
    resources = validate_resources(plan.get("resources"))
    digest = validate_environment_digest(spec, plan, os.environ.get("FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID", ""))
    return spec, plan, files, resources, actual_nix, digest, frozen_sbom_sha256


def _write_failure(
    run_dir: Path,
    spec: dict[str, Any] | None,
    error: BuilderError,
    *,
    started: float,
    log_ref: str,
    resources: dict[str, int] | None = None,
    disk_budget: DiskBudget | None = None,
) -> None:
    empty_spec = spec or {
        "specSha256": "",
        "platform": "",
        "builderImage": "",
        "builderImageDigest": "",
    }
    resources = resources or {"cpu": 2, "memoryMiB": 4096, "diskMiB": 10240, "pids": 512, "timeoutSecs": 1800}
    attestation = observe_attestation(resources=resources, sandbox_observed=False, no_secrets=False)
    receipt = build_receipt(
        spec=empty_spec,
        status="FAILED",
        store_path="",
        output_sha256="",
        log_ref=log_ref,
        attestation=attestation,
        blockers=[error.blocker()],
        duration_ms=int((time.monotonic() - started) * 1000),
        sbom_sha256=str((spec or {}).get("sbomSha256", "")),
    )
    write_json(run_dir / "result.json", receipt)
    write_json(
        run_dir / "builder-evidence.json",
        {
            "schemaVersion": SCHEMA_VERSION,
            "status": "FAILED",
            "error": error.blocker(),
            "createdAt": now_rfc3339(),
        },
    )
    if disk_budget and error.code == "ENVIRONMENT_RESOURCE_LIMIT":
        used, complete = disk_budget.usage()
        write_json(
            run_dir / "disk-failure.json",
            {
                **disk_budget.failure_record("build", used, complete),
                "error": error.blocker(),
                "createdAt": now_rfc3339(),
            },
        )


def build(args: argparse.Namespace) -> int:
    started = time.monotonic()
    request = _load_request(args.request)
    source_workspace = _workspace(request, args.workspace)
    run_dir = _run_dir(request, args.output)
    log_ref = str((run_dir / "build.log").relative_to(run_dir))
    spec: dict[str, Any] | None = None
    resources: dict[str, int] | None = None
    disk_budget: DiskBudget | None = None
    try:
        spec, plan, files, resources, actual_nix, digest, frozen_sbom_sha256 = _validate_request(
            request, source_workspace
        )
        phase = args.phase
        if phase not in ("fetch", "build"):
            fail("ENVIRONMENT_SPEC_INVALID", "phase must be fetch or build")
        validate_fetch_policy(request, plan, phase)
        build_workspace = run_dir / "workspace"
        disk_budget = builder_disk_budget(resources, run_dir, build_workspace)
        prepare_workspace(source_workspace, build_workspace, reuse=phase == "build")
        write_environment_files(build_workspace, files)
        disk_budget.check()
        proxy = request.get("fetchProxy")
        if proxy is not None:
            if not isinstance(proxy, str) or not proxy.startswith(("http://", "https://")) or "@" in proxy:
                fail("ENVIRONMENT_NETWORK_DENIED", "fetchProxy must be a credential-free HTTP(S) proxy")
        if os.environ.get("FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE") != "1":
            fail(
                "ENVIRONMENT_BUILDER_UNAVAILABLE",
                "the builder requires a persistent verified Nix store cache; unsigned local closure import is not supported",
            )
        log = BuildLog(run_dir / "build.log")
        try:
            root = realize_environment(
                build_workspace,
                run_dir,
                offline=phase == "build",
                resources=resources,
                timeout=resources["timeoutSecs"],
                log=log,
                proxy=proxy if phase == "fetch" else None,
                disk_budget=disk_budget,
            )
            disk_budget.check()
            manifest = closure_manifest(root)
            output_sha = environment_digest(
                spec["specSha256"], spec["platform"], root, manifest, frozen_sbom_sha256
            )
            if phase == "fetch":
                write_json(
                    run_dir / "fetch-result.json",
                    {
                        "schemaVersion": SCHEMA_VERSION,
                        "status": "FETCHED",
                        "specSha256": spec["specSha256"],
                        "storePath": root,
                        "outputSha256": output_sha,
                        "environmentDigest": digest,
                        "closureManifestSha256": closure_manifest_sha256(manifest),
                        "closureEntries": len(manifest),
                        "durationMs": int((time.monotonic() - started) * 1000),
                    },
                )
                return 0
        finally:
            log.close()

        sbom = build_sbom(
            spec=spec,
            plan=plan,
            manifest=manifest,
            nix_version=actual_nix,
            devenv_version=_actual_versions()[1],
        )
        sbom_path = run_dir / "sbom.cdx.json"
        sbom_path.write_text(sbom_text(sbom), encoding="utf-8")
        sbom_path.chmod(0o600)
        sbom_document_sha = sbom_sha256(sbom)
        write_json(
            run_dir / "sbom-digest.json",
            {
                "schemaVersion": SCHEMA_VERSION,
                "frozenSbomSha256": frozen_sbom_sha256,
                "licenseInventoryEntriesSha256": frozen_sbom_sha256,
                "cycloneDxDocumentSha256": sbom_document_sha,
                "matchesFrozenSpec": frozen_sbom_sha256 == spec.get("sbomSha256"),
            },
        )
        inventory = license_inventory_copy(plan)
        write_json(run_dir / "license-inventory.json", inventory)
        write_json(run_dir / "closure.json", {"entries": manifest, "count": len(manifest)})
        disk_budget.check()
        attestation = observe_attestation(
            resources=resources,
            sandbox_observed=True,
            no_secrets=True,
        )
        receipt = build_receipt(
            spec=spec,
            status="SUCCEEDED",
            store_path=root,
            output_sha256=output_sha,
            log_ref=log_ref,
            attestation=attestation,
            blockers=[],
            duration_ms=int((time.monotonic() - started) * 1000),
            sbom_sha256=frozen_sbom_sha256,
        )
        write_json(run_dir / "result.json", receipt)
        write_json(
            run_dir / "builder-evidence.json",
            {
                "schemaVersion": SCHEMA_VERSION,
                "status": "SUCCEEDED",
                "specSha256": spec["specSha256"],
                "planSha256": spec["planSha256"],
                "environmentDigest": digest,
                "outputSha256": output_sha,
                "storePath": root,
                "closureEntries": len(manifest),
                "frozenSbomSha256": frozen_sbom_sha256,
                "sbomDocumentSha256": sbom_document_sha,
                "licenseInventorySha256": inventory.get("inventorySha256"),
                "nixVersion": actual_nix,
                "devenvVersion": _actual_versions()[1],
                "sandboxAttestation": attestation,
                "createdAt": now_rfc3339(),
            },
        )
        print(json.dumps(receipt, sort_keys=True))
        return 0 if receipt["status"] == "SUCCEEDED" else 2
    except BuilderError as error:
        _write_failure(
            run_dir,
            spec,
            error,
            started=started,
            log_ref="build.log",
            resources=resources,
            disk_budget=disk_budget,
        )
        print(json.dumps({"status": "FAILED", "blocker": error.blocker()}, sort_keys=True))
        return error.exit_code


def _resolution_file(workspace: Path, name: str, origin: str) -> dict[str, Any]:
    path = workspace / name
    if not path.is_file():
        fail("ENVIRONMENT_PIN_MISSING", f"the generated environment has no {name}")
    try:
        content = path.read_text(encoding="utf-8")
    except OSError as error:
        fail("ENVIRONMENT_PIN_MISSING", f"cannot read {name}: {error}")
    if len(content.encode("utf-8")) > MAX_ENVIRONMENT_FILE_BYTES:
        fail("ENVIRONMENT_PLAN_INVALID", f"{name} exceeds its byte bound")
    try:
        document = json.loads(content)
    except json.JSONDecodeError:
        fail("ENVIRONMENT_PIN_MISSING", f"{name} is not a valid lock document")
    if not isinstance(document, dict) or not document:
        fail("ENVIRONMENT_PIN_MISSING", f"{name} is not a valid lock document")
    if name == "flake.lock" and not isinstance(document.get("nodes"), dict):
        fail("ENVIRONMENT_PIN_MISSING", "flake.lock has no locked nodes")
    return {
        "path": name,
        "sha256": sha256_text(content),
        "origin": origin,
        "content": content,
    }


def _package_metadata_expression(workspace: Path, entries: list[dict[str, str]]) -> str:
    """Build a pure Nix expression that reads exact package metadata.

    Package names and attributes are JSON data, never interpolated as Nix
    source. `builtins.getFlake` reads the already-locked local flake and its
    pinned nixpkgs input; no host path or unpinned fetch is introduced.
    """
    workspace_ref = json.dumps(f"path:{workspace}")
    platform = json.dumps(DEFAULT_PLATFORM)
    packages_json = json.dumps(entries)
    return f"""let
  flake = builtins.getFlake {workspace_ref};
  pkgs = import flake.inputs.nixpkgs {{ system = {platform}; }};
  lib = pkgs.lib;
  packages = builtins.fromJSON {json.dumps(packages_json)};
  licenseOf = p:
    if (p ? meta) && (p.meta ? license) then
      (if builtins.isList p.meta.license then
        builtins.concatStringsSep " AND " (map (l: if builtins.isAttrs l then (l.spdxId or l.shortName or "") else "") p.meta.license)
       else if builtins.isAttrs p.meta.license then (p.meta.license.spdxId or p.meta.license.shortName or "")
       else "")
    else "";
  unfreeOf = p:
    if (p ? meta) && (p.meta ? unfree) then p.meta.unfree
    else if (p ? meta) && (p.meta ? license) then
      (if builtins.isList p.meta.license then
        !(lib.all (l: if builtins.isAttrs l then (l.free or true) else true) p.meta.license)
       else if builtins.isAttrs p.meta.license then !(p.meta.license.free or true)
       else false)
    else false;
in map (entry:
  let pkg = lib.attrByPath (lib.splitString "." entry.attr) (throw "unknown package attribute") pkgs;
  in {{
    name = entry.name;
    attr = entry.attr;
    version = pkg.version or "";
    license = licenseOf pkg;
    unfree = unfreeOf pkg;
  }}) packages
"""


def _package_metadata(
    workspace: Path,
    plan: dict[str, Any],
    environment: dict[str, str],
    timeout: int,
    log: BuildLog,
) -> list[dict[str, Any]]:
    selected = plan.get("packages", [])
    if not selected:
        return []
    entries: list[dict[str, str]] = []
    claims: list[dict[str, Any]] = []
    for package in selected:
        if not isinstance(package, dict):
            fail("ENVIRONMENT_PLAN_INVALID", "a package entry is not an object")
        validate_package_source(package)
        name = str(package.get("name", "")).strip()
        attr = str(package.get("attr", "")).strip() or name
        entries.append({"name": name, "attr": attr})
        claims.append(
            {
                "version": str(package.get("version", "")),
                "license": str(package.get("license", "")),
                "unfree": bool(package.get("unfree", False)),
            }
        )
    observed = nix_eval_json(
        _package_metadata_expression(workspace, entries),
        env=environment,
        timeout=timeout,
        log=log,
        failure_code="ENVIRONMENT_PIN_MISSING",
    )
    if not isinstance(observed, list) or len(observed) != len(entries):
        fail(
            "ENVIRONMENT_PLAN_INVALID",
            "the package metadata evaluation did not return every selected package",
        )
    result: list[dict[str, Any]] = []
    for raw, expected, claim in zip(observed, entries, claims, strict=True):
        if not isinstance(raw, dict):
            fail("ENVIRONMENT_PLAN_INVALID", "package metadata is not an object")
        name = raw.get("name")
        attr = raw.get("attr")
        version = raw.get("version")
        license_value = raw.get("license")
        unfree = raw.get("unfree")
        if (
            not isinstance(name, str)
            or not isinstance(attr, str)
            or not isinstance(version, str)
            or not isinstance(license_value, str)
            or not isinstance(unfree, bool)
        ):
            fail("ENVIRONMENT_PLAN_INVALID", "package metadata has an invalid shape")
        if name != expected["name"] or attr != expected["attr"]:
            fail("ENVIRONMENT_PLAN_INVALID", "package metadata identity does not match the plan")
        if version != claim["version"]:
            fail(
                "ENVIRONMENT_PIN_MISSING",
                f"package {name} version does not match the independently observed metadata",
            )
        if license_value != claim["license"]:
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                f"package {name} license does not match the independently observed metadata",
            )
        if unfree != claim["unfree"]:
            fail(
                "ENVIRONMENT_UNFREE_REQUIRES_OWNER",
                f"package {name} unfree state does not match the independently observed metadata",
            )
        result.append(
            {
                "name": name,
                "attr": attr,
                "version": version,
                "license": license_value,
                "unfree": unfree,
            }
        )
    return result


def resolve(args: argparse.Namespace) -> int:
    request = _load_request(args.request)
    source_workspace = _workspace(request, args.workspace)
    run_dir = _run_dir(request, args.output)
    log = BuildLog(run_dir / "resolve.log")
    disk_budget: DiskBudget | None = None
    try:
        actual_nix, actual_devenv = _actual_versions()
        desc = descriptor()
        plan, files, resources = validate_resolution_plan(
            request.get("plan"),
            source_workspace=source_workspace,
            descriptor_image=desc["image"],
            descriptor_digest=desc["digest"],
            descriptor_platform=desc["platform"],
        )
        validate_fetch_policy(request, plan, "resolve")
        network = request.get("fetchNetwork", "none")
        proxy = request.get("fetchProxy")
        build_workspace = run_dir / "workspace"
        disk_budget = builder_disk_budget(resources, run_dir, build_workspace)
        prepare_workspace(source_workspace, build_workspace, reuse=True)
        write_environment_files(build_workspace, files)
        disk_budget.check()
        environment = child_environment(
            build_workspace,
            run_dir,
            offline=network == "none",
            resources=resources,
            proxy=proxy if isinstance(proxy, str) else None,
        )
        existing = {
            name: (build_workspace / name).is_file()
            for name in ("flake.lock", "devenv.lock")
        }
        timeout = resources["timeoutSecs"]
        if not existing["flake.lock"] and (
            (build_workspace / "flake.nix").is_file()
        ):
            command = ["nix", "flake", "lock"]
            if network == "none":
                command.append("--offline")
            run_logged(
                command,
                cwd=build_workspace,
                env=environment,
                timeout=timeout,
                log=log,
                failure_code="ENVIRONMENT_PIN_MISSING",
                offline=network == "none",
                disk_budget=disk_budget,
            )
        if not existing["devenv.lock"] and (
            (build_workspace / "devenv.nix").is_file()
            or (build_workspace / "devenv.yaml").is_file()
        ):
            run_logged(
                ["devenv", "update"],
                cwd=build_workspace,
                env=environment,
                timeout=timeout,
                log=log,
                failure_code="ENVIRONMENT_PIN_MISSING",
                offline=network == "none",
                disk_budget=disk_budget,
            )
        resolution_files = []
        for name in ("flake.lock", "devenv.lock"):
            if (build_workspace / name).is_file():
                origin = "PROJECT" if existing[name] else "GENERATED"
                resolution_files.append(_resolution_file(build_workspace, name, origin))
        if not resolution_files:
            fail(
                "ENVIRONMENT_PIN_MISSING",
                "the environment builder resolved no flake.lock or devenv.lock",
            )
        disk_budget.check()
        resolution = {
            "schemaVersion": SCHEMA_VERSION,
            "planSha256": plan["planSha256"],
            "files": resolution_files,
            "nixVersion": actual_nix,
            "devenvVersion": actual_devenv,
            "packages": _package_metadata(
                build_workspace, plan, environment, timeout, log
            ),
        }
        write_json(run_dir / "resolution.json", resolution)
        print(json.dumps(resolution, sort_keys=True))
        return 0
    except BuilderError as error:
        if disk_budget and error.code == "ENVIRONMENT_RESOURCE_LIMIT":
            used, complete = disk_budget.usage()
            write_json(
                run_dir / "disk-failure.json",
                {
                    **disk_budget.failure_record("resolve", used, complete),
                    "error": error.blocker(),
                    "createdAt": now_rfc3339(),
                },
            )
        print(json.dumps({"status": "FAILED", "blocker": error.blocker()}, sort_keys=True))
        return error.exit_code
    finally:
        log.close()


def _validate_receipt(spec: dict[str, Any], receipt: Any) -> dict[str, Any]:
    if not isinstance(receipt, dict):
        fail("ENVIRONMENT_BUILD_FAILED", "the check request has no build receipt")
    if receipt.get("status") != "SUCCEEDED":
        fail("ENVIRONMENT_BUILD_FAILED", "the check request receipt did not succeed")
    if receipt.get("specSha256") != spec.get("specSha256"):
        fail("ENVIRONMENT_HASH_MISMATCH", "the receipt binds a different environment spec")
    if (
        receipt.get("platform") != spec.get("platform")
        or receipt.get("builderImage") != spec.get("builderImage")
        or receipt.get("builderImageDigest") != spec.get("builderImageDigest")
    ):
        fail("ENVIRONMENT_HASH_MISMATCH", "the receipt builder identity does not match the spec")
    attestation = receipt.get("sandboxAttestation")
    if not isinstance(attestation, dict) or not all(
        attestation.get(field) is True
        for field in (
            "nonRoot",
            "noSecrets",
            "noHostNetwork",
            "noHostSocket",
            "noHostMount",
            "sandbox",
            "boundedResources",
        )
    ):
        fail("ENVIRONMENT_SANDBOX_DENIED", "the receipt lacks a complete sandbox attestation")
    if attestation.get("impure") is not False:
        fail("ENVIRONMENT_SANDBOX_DENIED", "the receipt records impure evaluation")
    if not receipt.get("outputSha256"):
        fail("ENVIRONMENT_HASH_MISMATCH", "the receipt has no output digest")
    return receipt


def check(args: argparse.Namespace) -> int:
    started = time.monotonic()
    request = _load_request(args.request)
    source_workspace = _workspace(request, args.workspace)
    run_dir = _run_dir(request, args.output)
    if not (run_dir / "check.json").exists():
        (run_dir / "check.json").parent.mkdir(parents=True, exist_ok=True)
    disk_budget: DiskBudget | None = None
    try:
        spec, plan, files, resources, _actual_nix, _digest, frozen_sbom_sha256 = _validate_request(
            request, source_workspace
        )
        validate_fetch_policy(request, plan, "check")
        receipt = _validate_receipt(spec, request.get("receipt"))
        check_spec = validate_check(request.get("check"))
        if check_spec["checkId"] != args.check_id and args.check_id:
            fail("ENVIRONMENT_SPEC_INVALID", "the requested check id does not match the check document")
        if os.environ.get("FACTORY_ENVIRONMENT_BUILDER_SHARED_STORE") != "1":
            fail(
                "ENVIRONMENT_BUILDER_UNAVAILABLE",
                "the builder requires a persistent verified Nix store cache; unsigned local closure import is not supported",
            )
        # A check must always see the candidate workspace supplied by the
        # broker, never the workspace left behind by the build phase in the
        # shared per-spec cache. Use a distinct path and recreate it on every
        # check; only the Nix store and other cache state are preserved.
        check_workspace = run_dir / "check-workspace"
        disk_budget = builder_disk_budget(resources, run_dir, check_workspace)
        prepare_workspace(source_workspace, check_workspace, reuse=False)
        write_environment_files(check_workspace, files)
        disk_budget.check()
        log = BuildLog(run_dir / "check.log")
        try:
            root = realize_environment(
                check_workspace,
                run_dir,
                offline=True,
                resources=resources,
                timeout=resources["timeoutSecs"],
                log=log,
                proxy=None,
                disk_budget=disk_budget,
            )
            disk_budget.check()
            manifest = closure_manifest(root)
            output_sha = environment_digest(
                spec["specSha256"], spec["platform"], root, manifest, frozen_sbom_sha256
            )
            if output_sha != receipt.get("outputSha256"):
                fail(
                    "ENVIRONMENT_HASH_MISMATCH",
                    "the restored environment does not match the frozen build receipt",
                )
            cwd = check_workspace if check_spec["cwd"] == "." else check_workspace / check_spec["cwd"]
            if not cwd.is_dir():
                fail("ENVIRONMENT_PLAN_INVALID", f"check working directory {check_spec['cwd']} is missing")
            code, stdout, stderr, stdout_truncated, stderr_truncated = run_check(
                check_workspace,
                run_dir,
                cwd,
                check_spec["argv"],
                timeout=check_spec["timeoutSecs"],
                resources=resources,
                log=log,
                expect_exit=check_spec["expectExit"],
                disk_budget=disk_budget,
            )
            disk_budget.check()
        finally:
            log.close()
        passed = code == check_spec["expectExit"]
        write_json(
            run_dir / "check-result.json",
            {
                "schemaVersion": SCHEMA_VERSION,
                "status": "PASSED" if passed else "FAILED",
                "checkId": check_spec["checkId"],
                "specSha256": spec["specSha256"],
                "outputSha256": receipt.get("outputSha256"),
                "exitCode": code,
                "expectExit": check_spec["expectExit"],
                "stdout": stdout,
                "stderr": stderr,
                "stdoutTruncated": stdout_truncated,
                "stderrTruncated": stderr_truncated,
                "durationMs": int((time.monotonic() - started) * 1000),
                "createdAt": now_rfc3339(),
            },
        )
        print(json.dumps({"status": "PASSED" if passed else "FAILED", "checkId": check_spec["checkId"]}))
        return 0 if passed else 1
    except BuilderError as error:
        write_json(
            run_dir / "check-result.json",
            {
                "schemaVersion": SCHEMA_VERSION,
                "status": "FAILED",
                "error": error.blocker(),
                "durationMs": int((time.monotonic() - started) * 1000),
                "createdAt": now_rfc3339(),
            },
        )
        if disk_budget and error.code == "ENVIRONMENT_RESOURCE_LIMIT":
            used, complete = disk_budget.usage()
            write_json(
                run_dir / "disk-failure.json",
                {
                    **disk_budget.failure_record("check", used, complete),
                    "error": error.blocker(),
                    "createdAt": now_rfc3339(),
                },
            )
        print(json.dumps({"status": "FAILED", "blocker": error.blocker()}, sort_keys=True))
        return error.exit_code


def self_test(_args: argparse.Namespace) -> int:
    checks = {
        "schemaVersion": SCHEMA_VERSION,
        "builderPolicyRevision": BUILDER_POLICY_REVISION,
        "descriptor": descriptor(),
        "nixVersion": nix_version(),
        "devenvVersion": devenv_version(),
    }
    print(json.dumps(checks, sort_keys=True))
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="factory-environment-builder")
    subparsers = parser.add_subparsers(dest="command", required=True)

    descriptor_parser = subparsers.add_parser("descriptor")
    descriptor_parser.set_defaults(handler=lambda _args: _print(descriptor()))

    resolve_parser = subparsers.add_parser("resolve")
    resolve_parser.add_argument("--request", required=True)
    resolve_parser.add_argument("--workspace", default=None)
    resolve_parser.add_argument("--output", default=None)
    resolve_parser.set_defaults(handler=resolve)

    build_parser = subparsers.add_parser("build")
    build_parser.add_argument("--request", required=True)
    build_parser.add_argument("--workspace", default=None)
    build_parser.add_argument("--output", default=None)
    build_parser.add_argument("--phase", choices=("fetch", "build"), default="build")
    build_parser.set_defaults(handler=build)

    fetch_parser = subparsers.add_parser("fetch")
    fetch_parser.add_argument("--request", required=True)
    fetch_parser.add_argument("--workspace", default=None)
    fetch_parser.add_argument("--output", default=None)
    fetch_parser.set_defaults(handler=build, phase="fetch")

    check_parser = subparsers.add_parser("check")
    check_parser.add_argument("--request", required=True)
    check_parser.add_argument("--workspace", default=None)
    check_parser.add_argument("--output", default=None)
    check_parser.add_argument("--check-id", default=None)
    check_parser.set_defaults(handler=check)

    selftest_parser = subparsers.add_parser("self-test")
    selftest_parser.set_defaults(handler=self_test)

    args = parser.parse_args(argv)
    try:
        return int(args.handler(args))
    except BuilderError as error:
        print(json.dumps({"status": "FAILED", "blocker": error.blocker()}, sort_keys=True))
        return error.exit_code


def _print(value: Any) -> int:
    print(json.dumps(value, sort_keys=True))
    return 0


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main())
