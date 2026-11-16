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
import re
import sys
import time
from pathlib import Path
from typing import Any

from . import BUILDER_POLICY_REVISION, DEFAULT_PLATFORM, SCHEMA_VERSION
from .disk import DiskBudget, builder_disk_budget
from .errors import BuilderError, fail
from .nix import (
    BuildLog,
    check_command_available,
    child_environment,
    closure_manifest,
    closure_manifest_sha256,
    devenv_version,
    environment_digest,
    nix_eval_json,
    nix_parse,
    nix_version,
    prepare_workspace,
    realize_environment,
    run_check,
    run_logged,
    write_environment_files,
)
from .policy import (
    MAX_ENVIRONMENT_FILE_BYTES,
    NIXPKGS_PREFIX,
    reject_computed_nixpkgs_import,
    reject_dynamic_config_ast,
    validate_check,
    validate_closure_licenses,
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
from .util import (
    last_nonempty_line,
    now_rfc3339,
    read_json,
    redact,
    sha256_text,
    tail,
    write_json,
)

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


def validate_nix_config_ast(files: list[Any]) -> None:
    """Parse every Nix file and reject dynamic nixpkgs config attrset keys."""
    for file in files:
        if not file.path.endswith(".nix"):
            continue
        ast = nix_parse(file.content, filename=file.path)
        reject_dynamic_config_ast(ast, file.path)
        reject_computed_nixpkgs_import(ast, file.path)


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
    validate_nix_config_ast(files)
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
            _verify_command_executables(
                build_workspace,
                run_dir,
                plan,
                offline=phase == "build",
                resources=resources,
                timeout=resources["timeoutSecs"],
                proxy=proxy if phase == "fetch" else None,
            )
            manifest = closure_manifest(root)
            package_metadata = _package_metadata(
                str(plan["nix"]["nixpkgs"]),
                plan,
                child_environment(
                    build_workspace,
                    run_dir,
                    offline=phase == "build",
                    resources=resources,
                    proxy=proxy if phase == "fetch" else None,
                ),
                resources["timeoutSecs"],
                log,
            )
            if plan.get("source") == "GENERATED":
                license_manifest = (
                    closure_manifest(
                        [str(package["storePath"]) for package in package_metadata]
                    )
                    if package_metadata
                    else []
                )
                exclude_paths: set[str] = set()
            else:
                profile = _environment_profile(build_workspace, run_dir)
                license_root = profile or root
                license_manifest = closure_manifest(license_root)
                exclude_paths = {license_root}
            closure_licenses = _closure_license_entries(
                plan,
                license_manifest,
                child_environment(
                    build_workspace,
                    run_dir,
                    offline=phase == "build",
                    resources=resources,
                    proxy=proxy if phase == "fetch" else None,
                ),
                resources["timeoutSecs"],
                log,
                exclude=exclude_paths,
            )
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
            closure_licenses=closure_licenses,
            packages=package_metadata,
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
        write_json(
            run_dir / "closure-license-inventory.json",
            {"entries": closure_licenses, "count": len(closure_licenses)},
        )
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


def _nixpkgs_expression_prefix(nixpkgs_pin: str) -> str:
    return (
        f"""let
  pkgs = import (builtins.getFlake {json.dumps(nixpkgs_pin)}).outPath {{ system = {json.dumps(DEFAULT_PLATFORM)}; }};
  lib = pkgs.lib;
"""
    )


def _package_metadata_expression(nixpkgs_pin: str, entries: list[dict[str, str]]) -> str:
    """Build a pure Nix expression that reads exact package metadata from the
    plan's pinned nixpkgs, never from a workspace flake. This supports
    Devenv-only projects that have no flake.nix.

    Package names and attributes are JSON data, never interpolated as Nix
    source.
    """
    packages_json = json.dumps(entries)
    return (
        _nixpkgs_expression_prefix(nixpkgs_pin)
        + f"""  packages = builtins.fromJSON {json.dumps(packages_json)};"""
        + """
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
  in {
    name = entry.name;
    attr = entry.attr;
    storePath = pkg.outPath;
    version = pkg.version or "";
    license = licenseOf pkg;
    unfree = unfreeOf pkg;
  }) packages
"""
    )


VERSION_CONSTRAINT = re.compile(r"^[0-9]+(?:\.[0-9]+)*$")


def version_constraint_matches(constraint: str, observed: str) -> bool:
    """Accept an exact version or a numeric dotted prefix of the observed one.

    A constraint such as ``22`` or ``22.20`` matches an observed ``22.20.0``
    only at a dot boundary. Ranges, wildcards and malformed values never match.
    """
    if not VERSION_CONSTRAINT.fullmatch(constraint):
        return False
    if observed == constraint:
        return True
    return observed.startswith(constraint + ".")


def _package_version_constraint(name: str, value: Any) -> str:
    constraint = str(value)
    if constraint != constraint.strip() or not VERSION_CONSTRAINT.fullmatch(constraint):
        fail(
            "ENVIRONMENT_PIN_MISSING",
            f"package {name} version must be an exact version or a numeric dotted prefix constraint",
        )
    return constraint


def _package_metadata(
    nixpkgs_pin: str,
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
                "version": _package_version_constraint(name, package.get("version", "")),
                "license": str(package.get("license", "")),
                "unfree": bool(package.get("unfree", False)),
            }
        )
    observed = nix_eval_json(
        _package_metadata_expression(nixpkgs_pin, entries),
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
        out_path = raw.get("storePath")
        version = raw.get("version")
        license_value = raw.get("license")
        unfree = raw.get("unfree")
        if (
            not isinstance(name, str)
            or not isinstance(attr, str)
            or not isinstance(out_path, str)
            or not out_path.startswith("/nix/store/")
            or not isinstance(version, str)
            or not isinstance(license_value, str)
            or not isinstance(unfree, bool)
        ):
            fail("ENVIRONMENT_PLAN_INVALID", "package metadata has an invalid shape")
        if name != expected["name"] or attr != expected["attr"]:
            fail("ENVIRONMENT_PLAN_INVALID", "package metadata identity does not match the plan")
        if not version_constraint_matches(claim["version"], version):
            fail(
                "ENVIRONMENT_PIN_MISSING",
                f"package {name} version constraint {claim['version']} does not match the independently observed {version}",
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
                "storePath": out_path,
                "version": version,
                "versionConstraint": claim["version"],
                "license": license_value,
                "unfree": unfree,
            }
        )
    return result


NESTED_PACKAGE_SET_NAMES: tuple[str, ...] = (
    "python312Packages",
    "python311Packages",
    "python3Packages",
    "nodePackages",
)
#: Evaluate a bounded slice of top-level nixpkgs attributes per Nix process.
#: A 1,000-attribute slice stayed below the 4 GiB builder budget in the measured
#: repro; the full 24,882-attribute set exceeded even 8 GiB.
CLOSURE_TOP_LEVEL_BATCH = 1_000


def _selected_package_set_names(plan: dict[str, Any]) -> list[str]:
    """Return the nested nixpkgs sets implied by the selected plan packages.

    The attribute path is evidence from the plan, not a licence lookup by
    package name: a selected ``python312Packages.fastapi`` explicitly says that
    the Python 3.12 package set is in scope. Toolchains are a secondary hint.
    """
    selected: list[str] = []
    packages = plan.get("packages", [])
    if isinstance(packages, list):
        for package in packages:
            if not isinstance(package, dict):
                continue
            attr = str(package.get("attr", "")).strip()
            prefix, _, remainder = attr.partition(".")
            if remainder and prefix in NESTED_PACKAGE_SET_NAMES:
                selected.append(prefix)
    toolchains = plan.get("toolchains", [])
    if isinstance(toolchains, list):
        for toolchain in toolchains:
            if not isinstance(toolchain, dict):
                continue
            identifier = str(toolchain.get("id", "")).strip().lower()
            version = str(toolchain.get("version", "")).strip()
            if "python" in identifier:
                if version.startswith("3.12"):
                    selected.append("python312Packages")
                elif version.startswith("3.11"):
                    selected.append("python311Packages")
                elif version.startswith("3."):
                    selected.append("python3Packages")
            if identifier in ("node", "nodejs") or "node" in identifier:
                selected.append("nodePackages")
    return sorted(set(selected))


def _closure_metadata_expression(
    nixpkgs_pin: str,
    closure_paths: list[str],
    *,
    names: list[str] | None = None,
    package_sets: list[str] | None = None,
) -> str:
    return _nixpkgs_expression_prefix(nixpkgs_pin) + _closure_metadata_body(
        closure_paths, names=names, package_sets=package_sets
    )


def _closure_metadata_body(
    closure_paths: list[str],
    *,
    names: list[str] | None = None,
    package_sets: list[str] | None = None,
) -> str:
    """Return the pure matching body bound to ``pkgs``.

    Kept separate from the pinned-nixpkgs prefix so a test can evaluate the
    real matching logic against a synthetic attrset without network or a store.
    ``names`` and ``package_sets`` scope one invocation to a bounded slice; the
    default preserves the full traversal for callers that need it.
    """
    wanted_json = json.dumps(closure_paths)
    names_expression = (
        "builtins.attrNames pkgs"
        if names is None
        else "builtins.fromJSON " + json.dumps(json.dumps(names))
    )
    if package_sets is None:
        package_sets = list(NESTED_PACKAGE_SET_NAMES)
    if package_sets:
        nested_expression = "safePackageSet " + "\n    ++ safePackageSet ".join(
            json.dumps(name) for name in package_sets
        )
    else:
        nested_expression = "[ ]"
    return f"""  wanted = builtins.fromJSON {json.dumps(wanted_json)};
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
        !(builtins.all (l: if builtins.isAttrs l then (l.free or true) else true) p.meta.license)
       else if builtins.isAttrs p.meta.license then !(p.meta.license.free or true)
       else false)
    else false;
  # Presence of an attribute set does not imply that its value is evaluable:
  # nixpkgs keeps end-of-life aliases that throw. A throwing set must not abort
  # metadata for every environment; the required plan packages are evaluated
  # separately and still fail closed, and an unresolved closure path still
  # fails closed below.
  safePackageSet = name:
    if pkgs ? ${{name}} then
      let attempt = builtins.tryEval pkgs.${{name}};
      in if attempt.success && builtins.isAttrs attempt.value then [ attempt.value ] else []
    else [];
  nestedPackageSets =
    {nested_expression};
  names = {names_expression};
  candidates = builtins.concatMap
    (name:
      let attempt = builtins.tryEval pkgs.${{name}};
      in if attempt.success && builtins.isAttrs attempt.value then [ attempt.value ] else [])
    names
    ++ builtins.concatMap
      (set:
        builtins.concatMap
          (name:
            let attempt = builtins.tryEval set.${{name}};
            in if attempt.success && builtins.isAttrs attempt.value then [ attempt.value ] else [])
          (builtins.attrNames set))
      nestedPackageSets;
  candidatesWithUnwrapped = builtins.concatMap
    (p:
      if (p ? unwrapped) && builtins.isAttrs p.unwrapped && (p.unwrapped ? outPath)
      then [ p p.unwrapped ] else [ p ])
    candidates;
in map (p: {{
    path = p.outPath;
    outputPaths = if p ? outputs then map (name: p.${{name}}.outPath) p.outputs else [ p.outPath ];
    drvPath = p.drvPath or "";
    name = p.pname or p.name or "";
    version = p.version or "";
    license = licenseOf p;
    unfree = unfreeOf p;
  }})
  (builtins.filter
    (p:
      let attempt = builtins.tryEval (
        (p ? outPath) && builtins.any
          (output: builtins.elem output wanted)
          (if p ? outputs then map (name: p.${{name}}.outPath) p.outputs else [ p.outPath ])
      );
      in attempt.success && attempt.value)
    candidatesWithUnwrapped)
"""


def _package_attribute_names(
    nixpkgs_pin: str,
    *,
    environment: dict[str, str],
    timeout: int,
    log: BuildLog,
) -> list[str]:
    """Return the top-level nixpkgs attribute names without forcing packages."""
    expression = _nixpkgs_expression_prefix(nixpkgs_pin) + "in builtins.attrNames pkgs"
    observed = nix_eval_json(
        expression,
        env=environment,
        timeout=timeout,
        log=log,
        failure_code="ENVIRONMENT_BUILD_FAILED",
    )
    if not isinstance(observed, list) or not all(
        isinstance(name, str) for name in observed
    ):
        fail(
            "ENVIRONMENT_BUILD_FAILED",
            "the package attribute enumeration returned an invalid list",
        )
    return observed


def _verify_command_executables(
    workspace: Path,
    cache: Path,
    plan: dict[str, Any],
    *,
    offline: bool,
    resources: dict[str, int],
    timeout: int,
    proxy: str | None,
) -> None:
    commands = plan.get("commands", {})
    if not isinstance(commands, dict):
        fail("ENVIRONMENT_PLAN_INVALID", "plan commands are not an object")
    checked: set[str] = set()
    for name, command in sorted(commands.items()):
        if not isinstance(command, dict):
            fail("ENVIRONMENT_PLAN_INVALID", f"command {name} is not an object")
        argv = command.get("argv")
        if not isinstance(argv, list) or not argv:
            fail("ENVIRONMENT_PLAN_INVALID", f"command {name} has no argv vector")
        executable = argv[0]
        if isinstance(executable, str) and executable in checked:
            continue
        if (
            not isinstance(executable, str)
            or not executable
            or "/" in executable
            or "\\" in executable
        ):
            fail(
                "ENVIRONMENT_PLAN_INVALID",
                f"command {name} must name a bounded executable without a path",
            )
        checked.add(executable)
        available, detail = check_command_available(
            workspace,
            cache,
            executable,
            offline=offline,
            resources=resources,
            timeout=min(timeout, 120),
            proxy=proxy,
        )
        if not available:
            fail(
                "ENVIRONMENT_PLAN_INVALID",
                f"command {name} executable {executable!r} is not provided by the realised environment: {detail}",
            )


# Exact reviewed internal bootstrap identity keyed by pinned nixpkgs, derivation
# and output path. A name-only `xgcc` exception would accept a fabricated store
# path; this map does not. Any other unobserved closure path fails closed.
REVIEWED_INTERNAL_CLOSURE_IDENTITIES: dict[
    str, dict[tuple[str, str], dict[str, Any]]
] = {
    "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d": {
        (
            "/nix/store/p8vpplfq13sjvxxvcfswp8p2rajji7yh-xgcc-14.3.0.drv",
            "/nix/store/16hvpw4b3r05girazh4rnwbw0jgjkb4l-xgcc-14.3.0-libgcc",
        ): {
            "name": "xgcc",
            "version": "14.3.0-libgcc",
            "license": "GPL-3.0-or-later",
            "unfree": False,
        }
    }
}
# Exact reviewed substrate identities. A name-only match is never accepted.
# These are tied to the current pinned nixpkgs and Devenv lock revisions; a pin
# change fails closed until a new identity is reviewed.
REVIEWED_SUBSTRATE_IDENTITIES: dict[
    str, dict[tuple[str, str], dict[str, Any]]
] = {
    "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d": {
        (
            "/nix/store/aiijyyiz8mrml76mr1kvx0r62aw8ccbn-devenv-1.11.2.drv",
            "/nix/store/mxi4r8qi2y79pybxyfzlqg5i8hl5a910-devenv-1.11.2",
        ): {"name": "devenv", "version": "1.11.2", "license": "Apache-2.0"},
        (
            "/nix/store/dzwdvpqgiv9ja73r6pfnkf4gdjk34x3n-nix-2.28.5.drv",
            "/nix/store/mxafjxh0amr24d2gb7n3km6hljj79qsj-nix-2.28.5",
        ): {"name": "nix", "version": "2.28.5", "license": "LGPL-2.1-only"},
        (
            "/nix/store/dzwdvpqgiv9ja73r6pfnkf4gdjk34x3n-nix-2.28.5.drv",
            "/nix/store/0nrwky0a12lwjvjcx3if3gv2bni2fjmx-nix-2.28.5-man",
        ): {"name": "nix", "version": "2.28.5", "license": "LGPL-2.1-only"},
        (
            "/nix/store/dzwdvpqgiv9ja73r6pfnkf4gdjk34x3n-nix-2.28.5.drv",
            "/nix/store/cak7gm7n4a78ikgna0q6d1l4028n2nnz-nix-2.28.5-dev",
        ): {"name": "nix", "version": "2.28.5", "license": "LGPL-2.1-only"},
        (
            "/nix/store/10jdyhddq82xfyfyrid403bdfsyq2l00-githooks-3.0.4.drv",
            "/nix/store/7s93k2darc8ba3n36j17dafzw7z165ck-githooks-3.0.4",
        ): {"name": "githooks", "version": "3.0.4", "license": "MIT"},
        (
            "",
            "/nix/store/3p306srz83h9z9v0ma9xcxb8y8cdxkxj-source",
        ): {"name": "nixpkgs", "version": "source", "license": "MIT"},
        (
            "",
            "/nix/store/q8g9dljvn01w9fn5ih0yam13z96lxfa2-source",
        ): {"name": "git-hooks.nix", "version": "source", "license": "MIT"},
        (
            "",
            "/nix/store/00av8zvvlwqh6l41kqlfq5n67mv10pfj-source/src/modules",
        ): {"name": "devenv", "version": "source", "license": "Apache-2.0"},
        (
            "unknown-deriver",
            "/nix/store/xlg2s2hdngc2gjb6kfwac1g7b4q2pcg5-nix-2.28.3",
        ): {"name": "nix", "version": "2.28.3", "license": "LGPL-2.1-only"},
    }
}
# Exact reviewed deriver-less helper identities. Devenv materialises strip.sh
# without a deriver; any other deriver-less path fails closed.
REVIEWED_DERIVERLESS_HELPERS: frozenset[str] = frozenset(
    {
        "/nix/store/pilsssjjdxvdphlg2h19p0bfx5q0jzkn-strip.sh",
    }
)
STORE_NAME_VERSION = re.compile(r"^([A-Za-z0-9_+.-]+?)-(\d.*)$")


def _environment_profile(workspace: Path, run_dir: Path) -> str | None:
    """Return the exact realised package profile for a committed environment.

    Devenv exposes the actual package profile at `.devenv/profile`; a flake
    `nix develop` writes the profile to the run directory. Auditing the profile
    closure avoids treating Devenv's shell-wrapper files as nixpkgs packages.
    """
    for candidate in (workspace / ".devenv" / "profile", run_dir / "profile"):
        if candidate.exists() or candidate.is_symlink():
            resolved = candidate.resolve()
            if str(resolved).startswith("/nix/store/"):
                return str(resolved)
    return None


def _reviewed_internal_closure_license(
    pin: str, path: str, deriver: str
) -> dict[str, Any] | None:
    identity = REVIEWED_INTERNAL_CLOSURE_IDENTITIES.get(pin, {}).get((deriver, path))
    if identity is None:
        return None
    return {"path": path, **identity}


def _reviewed_substrate_identity(
    pin: str, path: str, deriver: str
) -> dict[str, Any] | None:
    identity = REVIEWED_SUBSTRATE_IDENTITIES.get(pin, {}).get((deriver, path))
    if identity is None:
        return None
    return {"path": path, **identity, "unfree": False}


CLOSURE_LICENSE_INVENTORY_FILE = "closure-license-inventory.json"
#: The broker reads at most 64 KiB from the cache volume before cleanup. Reserve
#: room for the header; a larger inventory is still written to the file/log but
#: is explicitly marked as a partial capture, never silently complete.
CLOSURE_LICENSE_CAPTURE_BYTES = 60 * 1024


def _closure_license_value(value: Any) -> Any:
    return redact(value) if isinstance(value, str) else value


def _closure_license_id(violation: dict[str, Any]) -> str:
    name = str(violation.get("name") or "").strip()
    version = str(violation.get("version") or "").strip()
    if name or version:
        return f"{name}@{version}"
    return f"index:{violation.get('index')}"


def _closure_license_violation(
    entry: Any, index: int
) -> dict[str, Any] | None:
    """Return one typed violation for any closure entry, if it has one."""
    try:
        validate_closure_licenses([entry])
    except BuilderError as error:
        violation: dict[str, Any] = {
            "index": index,
            "code": error.code,
            "message": redact(error.message),
        }
        if isinstance(entry, dict):
            violation.update(
                {
                    "path": _closure_license_value(entry.get("path")),
                    "name": _closure_license_value(entry.get("name")),
                    "version": _closure_license_value(entry.get("version")),
                    "license": _closure_license_value(entry.get("license")),
                    "unfree": entry.get("unfree"),
                }
            )
        else:
            violation["entryType"] = "non-object"
        return violation
    return None


def _closure_license_inventory_entry(entry: Any, index: int) -> dict[str, Any]:
    if not isinstance(entry, dict):
        return {"index": index, "entryType": "non-object"}
    return {
        "index": index,
        "path": _closure_license_value(entry.get("path")),
        "name": _closure_license_value(entry.get("name")),
        "version": _closure_license_value(entry.get("version")),
        "license": _closure_license_value(entry.get("license")),
        "unfree": entry.get("unfree"),
    }


def _validate_closure_licenses_reporting_all(
    entries: list[dict[str, Any]], log: BuildLog
) -> list[dict[str, Any]]:
    """Validate fail-closed and retain every entry and every offender.

    The real validator runs first and its original typed code is re-raised, so
    the gate result and precedence (unfree/denied/unknown) do not change. The
    complete inventory is written to the existing builder log directory and to
    ``resolve.log`` before raising; the broker reads the file/log before
    cleanup. If the 64 KiB broker capture cannot contain the full list, the
    blocker says so with the expected/observed counts instead of claiming
    completeness.
    """
    try:
        return validate_closure_licenses(entries)
    except BuilderError as error:
        inventory = [
            _closure_license_inventory_entry(entry, index)
            for index, entry in enumerate(entries)
        ]
        violations = [
            violation
            for index, entry in enumerate(entries)
            if (violation := _closure_license_violation(entry, index)) is not None
        ]
        expected = len(entries)
        observed = len(inventory)
        ids = sorted({_closure_license_id(violation) for violation in violations})
        licenses = sorted(
            {
                str(violation["license"])
                for violation in violations
                if violation.get("license") is not None
            }
        )
        payload: dict[str, Any] = {
            "schemaVersion": 1,
            "source": "builder-closure-metadata",
            "expected": expected,
            "observed": observed,
            "complete": observed == expected,
            "violationCount": len(violations),
            "ids": ids,
            "licenses": licenses,
            "entries": inventory,
            "violations": violations,
            "errorCode": error.code,
            "errorMessage": redact(error.message),
        }
        # The broker reads the pretty JSON file from the cache volume before
        # cleanup. Reserve the 64 KiB read bound; compact log text is separate.
        payload_without_capture = json.dumps(payload, indent=2, sort_keys=True)
        capture_complete = (
            len(payload_without_capture.encode("utf-8")) <= CLOSURE_LICENSE_CAPTURE_BYTES
        )
        payload["captureComplete"] = capture_complete
        payload["captureReason"] = (
            ""
            if capture_complete
            else "inventory exceeds the 60 KiB broker capture bound; header and unique IDs retained"
        )
        path = log.path.parent / CLOSURE_LICENSE_INVENTORY_FILE
        write_json(path, payload)
        try:
            path.chmod(0o600)
        except OSError:
            pass
        compact = json.dumps(payload, sort_keys=True, separators=(",", ":"))
        header = {
            "schemaVersion": 1,
            "expected": expected,
            "observed": observed,
            "complete": observed == expected,
            "violationCount": len(violations),
            "ids": ids,
            "licenses": licenses,
            "errorCode": error.code,
            "file": path.name,
            "captureComplete": capture_complete,
        }
        log.write(
            "closure-license-inventory: "
            + json.dumps(header, sort_keys=True)
            + "\n"
        )
        log.write("closure-license-inventory-entries: " + compact + "\n")
        try:
            log.path.chmod(0o600)
        except OSError:
            pass

        ids_text = ",".join(ids)
        ids_truncated = len(ids_text) > 400
        if ids_truncated:
            ids_text = ids_text[:400] + ",..."
        licenses_text = ",".join(licenses)
        licenses_truncated = len(licenses_text) > 200
        if licenses_truncated:
            licenses_text = licenses_text[:200] + ",..."
        prefix = (
            f"{len(violations)} closure license violations"
            if violations
            else "closure inventory check failed"
        )
        summary = (
            f"{prefix}; ids={ids_text or 'none'}; "
            f"licenses={licenses_text or 'none'}; expected={expected} "
            f"observed={observed}; inventory={path.name}; "
            f"capture={'complete' if capture_complete else 'partial'}"
        )
        if ids_truncated:
            summary += "; idsTruncated=true"
        if licenses_truncated:
            summary += "; licensesTruncated=true"
        if not capture_complete:
            summary += "; " + payload["captureReason"]
        raise BuilderError(
            error.code,
            f"{summary}; first: {error.message}",
        ) from error


def _closure_license_entries(
    plan: dict[str, Any],
    manifest: list[dict[str, Any]],
    environment: dict[str, str],
    timeout: int,
    log: BuildLog,
    *,
    exclude: set[str] | None = None,
) -> list[dict[str, Any]]:
    pin = str(plan.get("nix", {}).get("nixpkgs", ""))
    if not pin.startswith(NIXPKGS_PREFIX):
        fail("ENVIRONMENT_PIN_MISSING", "the plan has no pinned nixpkgs for closure verification")
    substrate_entries: list[dict[str, Any]] = []
    paths: set[str] = set()
    for entry in manifest:
        if not isinstance(entry, dict):
            continue
        path = str(entry.get("path", ""))
        if path in (exclude or set()):
            continue
        deriver_value = entry.get("deriver")
        deriver = str(deriver_value).strip() if deriver_value is not None else ""
        substrate = _reviewed_substrate_identity(pin, path, deriver)
        if substrate is not None:
            substrate_entries.append(substrate)
            continue
        if not deriver:
            if path in REVIEWED_DERIVERLESS_HELPERS:
                continue
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                f"deriver-less closure path is not a reviewed helper: {path}",
            )
        if path:
            paths.add(path)
    ordered_paths = sorted(paths)
    if not ordered_paths:
        return validate_closure_licenses(substrate_entries)
    wanted_paths = set(ordered_paths)
    observed_entries: list[dict[str, Any]] = []
    seen_entries: set[str] = set()
    matched_paths: set[str] = set()

    def collect(observed: Any) -> None:
        if not isinstance(observed, list):
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                "closure metadata evaluation did not return a list",
            )
        for entry in observed:
            if not isinstance(entry, dict):
                continue
            dedupe_key = json.dumps(entry, sort_keys=True, separators=(",", ":"))
            if dedupe_key in seen_entries:
                continue
            seen_entries.add(dedupe_key)
            observed_entries.append(entry)
            output_paths = entry.get("outputPaths")
            if not isinstance(output_paths, list):
                output_paths = [entry.get("path", "")]
            for output_path in output_paths:
                matched_paths.add(str(output_path))

    def evaluate_scope(
        *, names: list[str] | None, package_sets: list[str] | None
    ) -> None:
        try:
            observed = nix_eval_json(
                _closure_metadata_expression(
                    pin, ordered_paths, names=names, package_sets=package_sets
                ),
                env=environment,
                timeout=timeout,
                log=log,
                failure_code="ENVIRONMENT_LICENSE_UNKNOWN",
            )
        except BuilderError as error:
            if "wall-clock bound" in error.message:
                fail(
                    "ENVIRONMENT_BUILD_TIMEOUT",
                    "one closure metadata evaluation exceeded the plan's "
                    f"{timeout}s per-operation timeout",
                )
            raise
        collect(observed)

    # The full nixpkgs universe exceeds the builder's 4 GiB budget (the measured
    # real traversal was killed at both 4 GiB and 8 GiB). Evaluate only the
    # nested sets implied by the selected plan packages first; if system paths
    # remain, traverse the top-level attributes in bounded slices. Each Nix
    # process exits before the next slice, so no single evaluation holds the
    # whole universe in memory. The frozen plan's timeout applies to each Nix
    # operation, not to the aggregate batch loop: a run may take hours.
    selected_sets = _selected_package_set_names(plan)
    # Process the sets implied by the plan first, then the remaining known sets
    # as a correctness fallback for transitive packages from another toolchain.
    package_sets = selected_sets + [
        name for name in NESTED_PACKAGE_SET_NAMES if name not in selected_sets
    ]
    for package_set in package_sets:
        if wanted_paths <= matched_paths:
            break
        log.write(f"[closure-metadata] package set {package_set}\n")
        evaluate_scope(names=[], package_sets=[package_set])
    if not wanted_paths <= matched_paths:
        try:
            attribute_names = _package_attribute_names(
                pin,
                environment=environment,
                timeout=timeout,
                log=log,
            )
        except BuilderError as error:
            if "wall-clock bound" in error.message:
                fail(
                    "ENVIRONMENT_BUILD_TIMEOUT",
                    "package attribute enumeration exceeded the plan's "
                    f"{timeout}s per-operation timeout",
                )
            raise
        total_batches = max(
            1,
            (len(attribute_names) + CLOSURE_TOP_LEVEL_BATCH - 1)
            // CLOSURE_TOP_LEVEL_BATCH,
        )
        for batch_index, start in enumerate(
            range(0, len(attribute_names), CLOSURE_TOP_LEVEL_BATCH), start=1
        ):
            if wanted_paths <= matched_paths:
                break
            batch = attribute_names[start : start + CLOSURE_TOP_LEVEL_BATCH]
            log.write(
                f"[closure-metadata] top-level batch {batch_index}/{total_batches} "
                f"names={len(batch)} matched={len(matched_paths)}/{len(wanted_paths)}\n"
            )
            evaluate_scope(names=batch, package_sets=[])
    by_path: dict[str, dict[str, Any]] = {}
    for entry in observed_entries:
        output_paths = entry.get("outputPaths")
        if not isinstance(output_paths, list):
            output_paths = [entry.get("path", "")]
        for output_path in output_paths:
            by_path[str(output_path)] = entry
    by_deriver = {
        str(entry.get("drvPath", "")): entry
        for entry in observed_entries
        if str(entry.get("drvPath", "")).strip()
    }
    manifest_by_path = {
        str(entry.get("path", "")): entry
        for entry in manifest
        if isinstance(entry, dict)
    }
    resolved: list[dict[str, Any]] = []
    for path in ordered_paths:
        observed_entry = by_path.get(path)
        if observed_entry is None:
            # A package may expose several outputs (for example libgcc) that
            # are not the attribute's default outPath. They share the deriver
            # and therefore the parent package's observed metadata.
            deriver = str(manifest_by_path.get(path, {}).get("deriver", "")).strip()
            observed_entry = by_deriver.get(deriver)
        if observed_entry is None:
            deriver = str(manifest_by_path.get(path, {}).get("deriver", "")).strip()
            reviewed = _reviewed_internal_closure_license(pin, path, deriver)
            if reviewed is not None:
                resolved.append(reviewed)
                continue
            fail(
                "ENVIRONMENT_LICENSE_UNKNOWN",
                f"transitive closure path has no independently observed package metadata or reviewed identity: {path}",
            )
        name = str(observed_entry.get("name", ""))
        version = str(observed_entry.get("version", ""))
        if not version:
            name_match = STORE_NAME_VERSION.match(name)
            if name_match is not None:
                name, version = name_match.group(1), name_match.group(2)
        resolved.append(
            {
                "path": path,
                "name": name,
                "version": version,
                "license": observed_entry.get("license"),
                "unfree": observed_entry.get("unfree"),
            }
        )
    return _validate_closure_licenses_reporting_all(substrate_entries + resolved, log)


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
        validate_nix_config_ast(files)
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
        # Freeze cannot happen until the builder has realised the exact
        # environment and checked the actual closure's licenses.
        root = realize_environment(
            build_workspace,
            run_dir,
            offline=network == "none",
            resources=resources,
            timeout=timeout,
            log=log,
            proxy=proxy if isinstance(proxy, str) else None,
            disk_budget=disk_budget,
        )
        package_metadata = _package_metadata(
            str(plan["nix"]["nixpkgs"]), plan, environment, timeout, log
        )
        if plan.get("source") == "GENERATED":
            # Audit the selected package outputs and their transitive package
            # closure. The Core-generated devShell wrapper is not a nixpkgs
            # package attribute and must not be part of this lookup.
            roots = [str(package["storePath"]) for package in package_metadata]
            closure_licenses = _closure_license_entries(
                plan,
                closure_manifest(roots) if roots else [],
                environment,
                timeout,
                log,
            )
        else:
            # A committed environment may carry packages in flake/devenv files
            # that are not listed in the plan. Audit the realised package
            # profile closure, not Devenv's shell-wrapper file closure, and
            # exclude only the exact profile/root wrapper path.
            profile = _environment_profile(build_workspace, run_dir)
            closure_root = profile or root
            closure_licenses = _closure_license_entries(
                plan,
                closure_manifest(closure_root),
                environment,
                timeout,
                log,
                exclude={closure_root},
            )
        disk_budget.check()
        resolution = {
            "schemaVersion": SCHEMA_VERSION,
            "planSha256": plan["planSha256"],
            "files": resolution_files,
            "nixVersion": actual_nix,
            "devenvVersion": actual_devenv,
            "packages": package_metadata,
            "closureLicenses": closure_licenses,
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
                "description": check_spec["description"],
                "specSha256": spec["specSha256"],
                "outputSha256": receipt.get("outputSha256"),
                "exitCode": code,
                "expectExit": check_spec["expectExit"],
                "stdout": stdout,
                "stderr": stderr,
                "stderrTail": last_nonempty_line(stderr),
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
