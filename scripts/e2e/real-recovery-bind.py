#!/usr/bin/env python3
"""Pre-run binding for the real generation-2 recovery scenario.

Creates the disposable UAT project configuration, the canonical V2 execution
profile and the project execution-policy grant through existing Core APIs. It
never calls a model, never creates a subscription admission, never reads or
writes the Core database and never overrides a policy value.

The grant is the materialization of the Owner authorization already on record;
this tool only performs the task-scoped operator setup. Run it once per fresh
disposable stack; ``--plan`` prints the exact API sequence without a network
call.

Usage:
    real-recovery-bind.py --plan  --compose-project e2e-recovery --project-repository <url>
    real-recovery-bind.py --apply --compose-project e2e-recovery --project-repository <url> \
        --core-api http://127.0.0.1:8899 --operator-token-file <0600 file> --output <0600 json>

The input token file is only read into memory for the Authorization header; it
is never printed or written to the interface JSON.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from real_recovery_profile import canonical_profile, profile_mismatches  # noqa: E402

PROJECT_ALLOWED_PATHS = ["src"]
PROJECT_CAPABILITIES = ["repository-edit", "generic-toolchain"]
PROJECT_QUALITY_PROFILE = "generic"

DEFAULT_REASONING_NOTE = (
    "the canonical deterministic profile; no turns, attempts, cumulative tokens "
    "or monetary cap"
)


class BindError(Exception):
    """A binding precondition, API or contract violation; never prints secrets."""


class Api:
    def __init__(self, base: str, token: str) -> None:
        self.base = base.rstrip("/")
        self.token = token

    def _request(
        self,
        method: str,
        path: str,
        payload: dict[str, Any] | None = None,
        expected: tuple[int, ...] = (200, 201),
    ) -> Any:
        data = None if payload is None else json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            self.base + path,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": "application/json",
                "Content-Type": "application/json",
                "Origin": self.base,
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                status = response.status
                body = response.read().decode("utf-8")
        except urllib.error.HTTPError as error:
            raw = error.read().decode("utf-8", errors="replace")
            try:
                parsed = json.loads(raw)
            except json.JSONDecodeError:
                parsed = {}
            code = str(parsed.get("code") or parsed.get("error") or "")[:200]
            raise BindError(f"{method} {path} -> {error.code} {code}".strip()) from None
        if status not in expected:
            raise BindError(f"{method} {path} -> unexpected status {status}")
        return json.loads(body) if body else None

    def get(self, path: str) -> Any:
        return self._request("GET", path)

    def post(self, path: str, payload: dict[str, Any]) -> Any:
        return self._request("POST", path, payload)

    def put(self, path: str, payload: dict[str, Any]) -> Any:
        return self._request("PUT", path, payload)


def _required(value: Any, name: str, errors: list[str]) -> None:
    if not value:
        errors.append(name)


def _validate_project_name(name: str) -> None:
    if name == "software-factory":
        raise BindError("refusing to bind the installed product project")
    if not any(token in name for token in ("recovery", "uat")):
        raise BindError("--compose-project must be a disposable *recovery*/*uat* project")


def _read_token(path: Path) -> str:
    if not path.is_file():
        raise BindError("--operator-token-file does not exist")
    mode = path.stat().st_mode
    if mode & 0o077:
        raise BindError("--operator-token-file must not be group/world readable")
    token = path.read_text(encoding="utf-8").strip()
    if len(token) < 16:
        raise BindError("--operator-token-file is too short to be an operator token")
    return token


def _same_list(left: Any, right: list[str]) -> bool:
    return isinstance(left, list) and sorted(map(str, left)) == sorted(right)


def config_is_bound(config: dict[str, Any]) -> bool:
    return (
        config.get("active") is True
        and _same_list(config.get("allowedPaths"), PROJECT_ALLOWED_PATHS)
        and _same_list(config.get("capabilities"), PROJECT_CAPABILITIES)
        and str(config.get("qualityProfile") or "") == PROJECT_QUALITY_PROFILE
    )


def _config_payload(config: dict[str, Any]) -> dict[str, Any]:
    revision = config.get("revision")
    if not isinstance(revision, int) or revision <= 0:
        raise BindError("project config has no usable revision")
    return {
        "expectedRevision": revision,
        "allowedPaths": list(PROJECT_ALLOWED_PATHS),
        "capabilities": list(PROJECT_CAPABILITIES),
        "qualityProfile": PROJECT_QUALITY_PROFILE,
        "active": True,
    }


def _find_profile(api: Api, name: str) -> dict[str, Any] | None:
    listing = api.get("/execution-profiles") or {}
    profiles = listing.get("profiles") if isinstance(listing, dict) else listing
    matches = [row for row in (profiles or []) if str(row.get("name") or "") == name]
    if len(matches) > 1:
        raise BindError(f"more than one execution profile is named {name!r}")
    return matches[0] if matches else None


def _bind_project(api: Api, args: argparse.Namespace) -> tuple[str, dict[str, Any]]:
    if args.project_id:
        project_id = args.project_id
    else:
        created = api.post(
            "/projects",
            {
                "repository": args.project_repository,
                "idempotency_key": f"{args.idempotency_key}:project:{args.compose_project}",
            },
        )
        if not isinstance(created, dict) or not created.get("id"):
            raise BindError("Core returned no project id")
        project_id = str(created["id"])

    config = api.get(f"/projects/{project_id}/config")
    if not isinstance(config, dict):
        raise BindError("Core returned no project config")
    if not config_is_bound(config):
        config = api.put(f"/projects/{project_id}/config", _config_payload(config))
        if not isinstance(config, dict) or not config_is_bound(config):
            raise BindError("Core did not persist the required project configuration")
    config_hash = str(config.get("configHash") or "")
    base_commit = str(config.get("baseCommit") or "")
    if not config_hash:
        raise BindError("project config has no configHash")
    return project_id, config


def _bind_profile(api: Api, args: argparse.Namespace) -> dict[str, Any]:
    name = args.profile_name
    existing = _find_profile(api, name)
    if existing is not None:
        profile = api.get(f"/execution-profiles/{existing['id']}")
        if not isinstance(profile, dict):
            raise BindError("Core returned no execution profile detail")
        problems = profile_mismatches(
            profile, provider=args.provider, model=args.model
        )
        if problems:
            raise BindError(
                "the existing execution profile differs from the canonical contract: "
                + ", ".join(problems)
            )
        return profile

    created = api.post(
        "/execution-profiles",
        canonical_profile(
            name=name,
            description=(
                "Pre-run binding for the real generation-2 recovery scenario; "
                + DEFAULT_REASONING_NOTE
            ),
            provider=args.provider,
            model=args.model,
        ),
    )
    if not isinstance(created, dict) or not created.get("id"):
        raise BindError("Core returned no execution profile")
    problems = profile_mismatches(created, provider=args.provider, model=args.model)
    if problems:
        raise BindError(
            "Core persisted an execution profile that differs from the canonical contract: "
            + ", ".join(problems)
        )
    return created


def _verify_grant(
    result: dict[str, Any],
    *,
    project_id: str,
    config_hash: str,
    profile: dict[str, Any],
) -> None:
    grant = result.get("grant") or {}
    delegation = result.get("delegation") or {}
    problems: list[str] = []
    expected_snake = {
        "project_id": project_id,
        "profile_id": str(profile["id"]),
        "profile_revision_id": str(profile.get("revisionId") or ""),
        "profile_revision": profile.get("revision"),
        "profile_hash": str(profile.get("contentHash") or ""),
        "project_config_hash": config_hash,
    }
    for field, expected in expected_snake.items():
        if grant.get(field) != expected:
            problems.append(field)
    for field in ("acknowledged",):
        if grant.get(field) is not True:
            problems.append(field)
    for field in ("autonomous", "subscription_confirmed"):
        if grant.get(field) is not False:
            problems.append(field)

    delegation_id = str(grant.get("delegation_id") or "")
    if not delegation_id or str(delegation.get("id") or "") != delegation_id:
        problems.append("delegation_id")
    if str(delegation.get("projectId") or "") != project_id:
        problems.append("delegation.projectId")
    if int(delegation.get("revision") or 0) <= 0:
        problems.append("delegation.revision")
    if not str(delegation.get("contentHash") or ""):
        problems.append("delegation.contentHash")
    if delegation.get("autonomousStart") is not False:
        problems.append("delegation.autonomousStart")
    if _same_list(delegation.get("allowedPaths"), PROJECT_ALLOWED_PATHS) is False:
        problems.append("delegation.allowedPaths")
    if _same_list(delegation.get("capabilities"), PROJECT_CAPABILITIES) is False:
        problems.append("delegation.capabilities")
    if delegation.get("revokedAt") is not None:
        problems.append("delegation.revokedAt")
    if problems:
        raise BindError(
            "Core execution-policy grant differs from the binding: " + ", ".join(problems)
        )


def _bind_grant(
    api: Api, args: argparse.Namespace, project_id: str, config: dict[str, Any], profile: dict[str, Any]
) -> dict[str, Any]:
    revision = profile.get("revision")
    content_hash = str(profile.get("contentHash") or "")
    revision_id = str(profile.get("revisionId") or "")
    if not isinstance(revision, int) or revision <= 0 or not content_hash or not revision_id:
        raise BindError("execution profile has no usable revision identity")
    payload = {
        "execution_profile_id": str(profile["id"]),
        "expected_revision": revision,
        "expected_revision_hash": content_hash,
        "project_config_hash": str(config.get("configHash") or ""),
        "acknowledge": True,
        "autonomous": False,
        "subscription_confirmed": False,
        "idempotency_key": f"{args.idempotency_key}:grant:{project_id}",
    }
    result = api.post(f"/projects/{project_id}/execution-policy", payload)
    if not isinstance(result, dict) or not isinstance(result.get("grant"), dict):
        raise BindError("Core returned no execution-policy grant")
    if not isinstance(result.get("delegation"), dict):
        raise BindError("Core returned no standing delegation")
    _verify_grant(
        result,
        project_id=project_id,
        config_hash=str(config.get("configHash") or ""),
        profile=profile,
    )
    return result


def _interface(
    args: argparse.Namespace,
    project_id: str,
    config: dict[str, Any],
    profile: dict[str, Any],
    grant: dict[str, Any],
) -> dict[str, Any]:
    grant_record = grant.get("grant") or {}
    delegation = grant.get("delegation") or {}
    return {
        "project_id": project_id,
        "profile_id": str(profile["id"]),
        "profile_revision": profile.get("revision"),
        "profile_revision_id": str(profile.get("revisionId") or ""),
        "profile_content_hash": str(profile.get("contentHash") or ""),
        "project_config_hash": str(config.get("configHash") or ""),
        "base_commit": str(config.get("baseCommit") or ""),
        "git_dir": args.git_dir,
        "core_api": args.core_api,
        "operator_token_file": args.operator_token_file,
        "core_database": args.core_database,
        "compose_project": args.compose_project,
        "provider": args.provider,
        "model": args.model,
        "grant_id": str(grant_record.get("delegation_id") or ""),
        "delegation_id": str(delegation.get("id") or ""),
        "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }


def _write_private_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + f".tmp.{os.getpid()}")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def plan_document(args: argparse.Namespace) -> dict[str, Any]:
    profile = canonical_profile(
        name=args.profile_name,
        description=(
            "Pre-run binding for the real generation-2 recovery scenario; "
            + DEFAULT_REASONING_NOTE
        ),
        provider=args.provider,
        model=args.model,
    )
    return {
        "status": "PLAN",
        "note": (
            "existing Core APIs only; no model call, no subscription admission, "
            "no Core database access, no policy override"
        ),
        "steps": [
            {
                "step": "project",
                "request": {
                    "method": "POST",
                    "path": "/projects",
                    "body": {
                        "repository": args.project_repository or "<existing project>",
                        "idempotency_key": f"{args.idempotency_key}:project:{args.compose_project}",
                    },
                },
                "reuse": "GET /projects/{id}/config is unchanged, otherwise PUT with expectedRevision",
                "config": {
                    "allowedPaths": PROJECT_ALLOWED_PATHS,
                    "capabilities": PROJECT_CAPABILITIES,
                    "qualityProfile": PROJECT_QUALITY_PROFILE,
                    "active": True,
                },
            },
            {
                "step": "profile",
                "request": {"method": "POST", "path": "/execution-profiles", "body": profile},
                "reuse": "GET /execution-profiles and reuse only an exact canonical match",
            },
            {
                "step": "grant",
                "request": {
                    "method": "POST",
                    "path": "/projects/{project_id}/execution-policy",
                    "body": {
                        "execution_profile_id": "<profile id>",
                        "expected_revision": "<profile revision>",
                        "expected_revision_hash": "<profile contentHash>",
                        "project_config_hash": "<configHash>",
                        "acknowledge": True,
                        "autonomous": False,
                        "subscription_confirmed": False,
                        "idempotency_key": f"{args.idempotency_key}:grant:<project id>",
                    },
                },
            },
            {
                "step": "interface",
                "fields": [
                    "project_id",
                    "profile_id",
                    "profile_revision",
                    "profile_revision_id",
                    "profile_content_hash",
                    "project_config_hash",
                    "base_commit",
                    "git_dir",
                    "core_api",
                    "operator_token_file",
                    "core_database",
                    "compose_project",
                    "provider",
                    "model",
                    "grant_id",
                    "delegation_id",
                ],
            },
        ],
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Bind the disposable project, canonical profile and grant for the real recovery run"
    )
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--plan", action="store_true")
    mode.add_argument("--apply", action="store_true")
    parser.add_argument("--core-api")
    parser.add_argument("--operator-token-file")
    parser.add_argument("--project-id")
    parser.add_argument("--project-repository")
    parser.add_argument("--compose-project", required=True)
    parser.add_argument("--git-dir")
    parser.add_argument("--core-database")
    parser.add_argument("--output")
    parser.add_argument("--profile-name")
    parser.add_argument("--provider", default="opencode-go")
    parser.add_argument("--model", default="deepseek-v4.1-flash")
    parser.add_argument("--idempotency-key", default="real-recovery-bind")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    if not args.profile_name:
        args.profile_name = f"Real recovery {args.compose_project}"
    try:
        _validate_project_name(args.compose_project)
        if args.project_id and args.project_repository:
            raise BindError("use --project-id or --project-repository, not both")
        if not args.project_id and not args.project_repository:
            raise BindError("--project-id or --project-repository is required")
        if args.plan:
            print(json.dumps(plan_document(args), indent=2))
            return 0

        missing = [
            name
            for name, value in (
                ("--core-api", args.core_api),
                ("--operator-token-file", args.operator_token_file),
                ("--output", args.output),
            )
            if not value
        ]
        if missing:
            raise BindError("missing required apply inputs: " + ", ".join(missing))
        token = _read_token(Path(args.operator_token_file))
        api = Api(args.core_api, token)
        project_id, config = _bind_project(api, args)
        profile = _bind_profile(api, args)
        grant = _bind_grant(api, args, project_id, config, profile)
        interface = _interface(args, project_id, config, profile, grant)
        _write_private_json(Path(args.output), interface)
        print(json.dumps({
            "status": "BOUND",
            "project_id": project_id,
            "profile_id": interface["profile_id"],
            "profile_content_hash": interface["profile_content_hash"],
            "project_config_hash": interface["project_config_hash"],
            "grant_id": interface["grant_id"],
            "delegation_id": interface["delegation_id"],
            "output": str(args.output),
        }, indent=2))
        return 0
    except BindError as error:
        print(f"binding refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
