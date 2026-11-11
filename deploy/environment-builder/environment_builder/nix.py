"""Bounded Nix/devenv execution and closure evidence.

The module never builds a shell string from untrusted input. Every command is
an argv vector. Build/evaluation runs with ``--offline``, ``sandbox = true`` and
``sandbox-fallback = false``; a missing store path is a network/build failure,
not a reason to relax the sandbox.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import signal
import subprocess
import time
from pathlib import Path
from typing import Any, Iterable

from .errors import BuilderError, fail
from .policy import EnvironmentFile, environment_without_secrets
from .util import (
    canonical_json,
    last_lines,
    redact,
    sha256_bytes,
    sha256_text,
    tail,
)

MAX_LOG_BYTES = 1024 * 1024
MAX_TAIL_BYTES = 64 * 1024
NIX_VERSION = re.compile(r"nix \(Nix\) ([0-9]+\.[0-9]+\.[0-9]+)")
DEVENV_VERSION = re.compile(r"devenv ([0-9]+\.[0-9]+\.[0-9]+)")
OFFLINE_MARKERS = (
    "unable to download",
    "could not resolve host",
    "network is unreachable",
    "temporary failure in name resolution",
    "unable to start any build",
    "cannot fetch",
    "unable to fetch",
)
SANDBOX_MARKERS = (
    "auto-disabling sandboxing",
    "kernel namespaces that are required for sandboxing",
    "sandbox-fallback",
)


def _tool_version(argv: list[str], pattern: re.Pattern[str], label: str) -> str:
    try:
        completed = subprocess.run(
            argv,
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
            env=environment_without_secrets(dict(os.environ)),
        )
    except (OSError, subprocess.SubprocessError) as error:
        fail("ENVIRONMENT_BUILDER_UNAVAILABLE", f"{label} is unavailable: {error}")
    output = f"{completed.stdout}\n{completed.stderr}"
    match = pattern.search(output)
    if completed.returncode != 0 or match is None:
        fail("ENVIRONMENT_BUILDER_UNAVAILABLE", f"{label} did not report a pinned version")
    return match.group(1)


def nix_version() -> str:
    return _tool_version(["nix", "--version"], NIX_VERSION, "Nix")


def devenv_version() -> str:
    return _tool_version(["devenv", "--version"], DEVENV_VERSION, "devenv")


class BuildLog:
    """Bounded, redacted log with a separately retained diagnostic tail."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._handle = self.path.open("w", encoding="utf-8")
        self._written = 0
        self._tail = ""
        self.truncated = False

    def write(self, line: str) -> None:
        self._tail = (self._tail + line)[-MAX_TAIL_BYTES:]
        if self._written >= MAX_LOG_BYTES:
            self.truncated = True
            return
        encoded = redact(line)
        remaining = MAX_LOG_BYTES - self._written
        data = encoded[:remaining]
        self._handle.write(data)
        self._written += len(data.encode("utf-8", errors="replace"))
        if len(data) < len(encoded):
            self.truncated = True

    def close(self) -> None:
        if not self._handle.closed:
            if self.truncated:
                self._handle.write("\n[build log truncated by the Factory builder]\n")
            self._handle.close()

    @property
    def diagnostic(self) -> str:
        return last_lines(self._tail, 80, 12_000)

    def __enter__(self) -> "BuildLog":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


def run_logged(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    timeout: int,
    log: BuildLog,
    failure_code: str = "ENVIRONMENT_BUILD_FAILED",
    offline: bool = False,
    allow_exit: Iterable[int] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run an argv vector with a wall-clock bound and a bounded log."""
    environment = environment_without_secrets(env)
    started = time.monotonic()
    log.write(f"$ {' '.join(argv)}\n")
    try:
        process = subprocess.Popen(
            argv,
            cwd=str(cwd),
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            errors="replace",
            bufsize=1,
            start_new_session=True,
        )
    except OSError as error:
        fail(failure_code, f"cannot start {argv[0]}: {error}")
    assert process.stdout is not None
    try:
        for line in process.stdout:
            log.write(line)
        process.wait(timeout=max(1, timeout - int(time.monotonic() - started)))
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except OSError:
                pass
        fail(
            "ENVIRONMENT_BUILD_TIMEOUT",
            f"environment command exceeded its {timeout}s wall-clock bound",
        )
    allowed = set(allow_exit or ())
    if process.returncode != 0 and process.returncode not in allowed:
        diagnostic = log.diagnostic
        lowered = diagnostic.lower()
        if offline and any(marker in lowered for marker in OFFLINE_MARKERS):
            fail(
                "ENVIRONMENT_NETWORK_DENIED",
                "the offline build needs a store path that the broker did not prefetch",
            )
        fail(failure_code, f"environment command exited {process.returncode}: {diagnostic}")
    return subprocess.CompletedProcess(argv, process.returncode, "", "")


def nix_config(offline: bool, resources: dict[str, int]) -> str:
    lines = [
        "experimental-features = nix-command flakes",
        "sandbox = true",
        "sandbox-fallback = false",
        "build-users-group =",
        "accept-flake-config = false",
        f"max-jobs = {max(1, min(resources['cpu'], 8))}",
        f"cores = {resources['cpu']}",
        "warn-dirty = false",
        "keep-going = false",
        "http-connections = 8",
    ]
    if offline:
        lines.extend(["substituters =", "connect-timeout = 1"])
    return "\n".join(lines) + "\n"


def child_environment(
    workspace: Path,
    cache: Path,
    *,
    offline: bool,
    resources: dict[str, int],
    proxy: str | None = None,
) -> dict[str, str]:
    home = cache / "home"
    tmp = cache / "tmp"
    home.mkdir(parents=True, exist_ok=True)
    tmp.mkdir(parents=True, exist_ok=True)
    env: dict[str, str] = {
        "PATH": "/home/factory/.nix-profile/bin:/nix/var/nix/profiles/default/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        "HOME": str(home),
        "TMPDIR": str(tmp),
        "TMP": str(tmp),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "NIX_CONFIG": nix_config(offline, resources),
        "NIX_PATH": "",
        "NIX_REMOTE": "",
        "XDG_CACHE_HOME": str(cache / "xdg-cache"),
        "XDG_CONFIG_HOME": str(cache / "xdg-config"),
        "XDG_DATA_HOME": str(cache / "xdg-data"),
        "XDG_STATE_HOME": str(cache / "xdg-state"),
        "DEVENV_ROOT": str(workspace),
        "DEVENV_STATE": str(workspace / ".devenv"),
        "DEVENV_DOTFILE": str(workspace / ".devenv"),
        "DEVENV_OFFLINE": "1" if offline else "0",
        "npm_config_cache": str(cache / "npm"),
        "npm_config_offline": "true" if offline else "false",
        "PLAYWRIGHT_BROWSERS_PATH": "/opt/playwright-browsers",
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
    }
    for name in ("FACTORY_ENVIRONMENT_BUILDER_IMAGE", "FACTORY_ENVIRONMENT_BUILDER_DIGEST",
                 "FACTORY_ENVIRONMENT_BUILDER_PLATFORM", "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION",
                 "FACTORY_ENVIRONMENT_NIX_VERSION", "FACTORY_ENVIRONMENT_DEVENV_VERSION",
                 "FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID"):
        if name in os.environ:
            env[name] = os.environ[name]
    if proxy:
        for name in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
            env[name] = proxy
        env["NO_PROXY"] = "127.0.0.1,localhost"
        env["no_proxy"] = "127.0.0.1,localhost"
        env["npm_config_proxy"] = proxy
        env["npm_config_https_proxy"] = proxy
    if "/home/factory/.nix-profile/bin" not in env["PATH"]:
        env["PATH"] = f"/home/factory/.nix-profile/bin:{env['PATH']}"
    return env


def restore_closure(path: Path) -> None:
    """Import a cached closure into this container's ephemeral store."""
    if not path.is_file():
        fail("ENVIRONMENT_BUILDER_UNAVAILABLE", "the cached environment closure is missing")
    with path.open("rb") as handle:
        completed = subprocess.run(
            ["nix-store", "--import"],
            stdin=handle,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=900,
            env=environment_without_secrets(dict(os.environ)),
        )
    if completed.returncode != 0:
        fail(
            "ENVIRONMENT_BUILD_FAILED",
            f"cannot restore the cached environment closure: {tail(completed.stdout, 2_000)}",
        )


def export_closure(root: str, destination: Path) -> None:
    """Export the exact environment closure for later offline checks."""
    completed = subprocess.run(
        ["nix-store", "--query", "--requisites", root],
        check=False,
        capture_output=True,
        text=True,
        timeout=300,
        env=environment_without_secrets(dict(os.environ)),
    )
    if completed.returncode != 0:
        fail("ENVIRONMENT_BUILD_FAILED", "cannot enumerate the environment closure")
    paths = [line for line in completed.stdout.splitlines() if line.startswith("/nix/store/")]
    if not paths:
        fail("ENVIRONMENT_BUILD_FAILED", "the environment closure is empty")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("wb") as handle:
        exported = subprocess.run(
            ["nix-store", "--export", *paths],
            stdout=handle,
            stderr=subprocess.PIPE,
            timeout=900,
            env=environment_without_secrets(dict(os.environ)),
        )
    if exported.returncode != 0:
        fail(
            "ENVIRONMENT_BUILD_FAILED",
            f"cannot export the environment closure: {tail(exported.stderr.decode('utf-8', 'replace'), 2_000)}",
        )


def closure_manifest(root: str) -> list[dict[str, Any]]:
    completed = subprocess.run(
        ["nix", "path-info", "--json", "--recursive", "--offline", root],
        check=False,
        capture_output=True,
        text=True,
        timeout=300,
        env=environment_without_secrets(dict(os.environ)),
    )
    if completed.returncode != 0:
        fail(
            "ENVIRONMENT_BUILD_FAILED",
            f"cannot read the environment closure: {tail(completed.stderr, 2_000)}",
        )
    try:
        raw = json.loads(completed.stdout)
    except json.JSONDecodeError:
        fail("ENVIRONMENT_BUILD_FAILED", "nix path-info returned invalid JSON")
    manifest: list[dict[str, Any]] = []
    for path, info in sorted(raw.items()):
        if not isinstance(info, dict):
            continue
        manifest.append(
            {
                "path": path,
                "narHash": str(info.get("narHash", "")),
                "narSize": int(info.get("narSize", 0) or 0),
            }
        )
    return manifest


def environment_digest(
    spec_sha256: str,
    platform: str,
    root: str,
    manifest: list[dict[str, Any]],
) -> str:
    return sha256_text(
        canonical_json(
            {
                "specSha256": spec_sha256,
                "platform": platform,
                "storePath": root,
                "closure": manifest,
            }
        )
    )


def _environment_command(workspace: Path, argv: list[str], *, offline: bool) -> list[str]:
    devenv = (workspace / "devenv.nix").is_file() or (workspace / "devenv.yaml").is_file()
    if devenv:
        command = ["devenv", "shell"]
        if offline:
            command.append("--offline")
        command.extend(["--", *argv])
        return command
    command = ["nix", "develop"]
    if offline:
        command.append("--offline")
    command.extend([".#default", "--command", *argv])
    return command


def realize_environment(
    workspace: Path,
    cache: Path,
    *,
    offline: bool,
    resources: dict[str, int],
    timeout: int,
    log: BuildLog,
    proxy: str | None,
) -> str:
    """Build/evaluate the exact environment and return its store path."""
    env = child_environment(
        workspace,
        cache,
        offline=offline,
        resources=resources,
        proxy=proxy,
    )
    if (workspace / "devenv.nix").is_file() or (workspace / "devenv.yaml").is_file():
        command = _environment_command(workspace, ["true"], offline=offline)
    else:
        command = ["nix", "develop"]
        if offline:
            command.append("--offline")
        command.extend([".#default", "--profile", str(cache / "profile"), "--command", "true"])
    run_logged(
        command,
        cwd=workspace,
        env=env,
        timeout=timeout,
        log=log,
        failure_code="ENVIRONMENT_BUILD_FAILED",
        offline=offline,
    )
    for candidate in (
        workspace / ".devenv" / "gc" / "shell",
        workspace / ".devenv" / "profile",
        cache / "profile",
    ):
        if candidate.exists() or candidate.is_symlink():
            resolved = candidate.resolve()
            if str(resolved).startswith("/nix/store/"):
                if any(marker in log.diagnostic for marker in SANDBOX_MARKERS):
                    fail("ENVIRONMENT_SANDBOX_DENIED", "Nix disabled or bypassed its sandbox")
                return str(resolved)
    fail(
        "ENVIRONMENT_BUILD_FAILED",
        "the environment did not produce a content-addressed store path",
    )


def run_check(
    workspace: Path,
    cache: Path,
    cwd: Path,
    argv: list[str],
    *,
    timeout: int,
    resources: dict[str, int],
    log: BuildLog,
    expect_exit: int = 0,
) -> int:
    env = child_environment(workspace, cache, offline=True, resources=resources)
    command_argv = list(argv)
    if cwd.resolve() != workspace.resolve():
        command_argv = ["env", "-C", str(cwd), *command_argv]
    command = _environment_command(workspace, command_argv, offline=True)
    completed = run_logged(
        command,
        cwd=workspace,
        env=env,
        timeout=timeout,
        log=log,
        failure_code="ENVIRONMENT_BUILD_FAILED",
        offline=True,
        allow_exit={expect_exit},
    )
    return completed.returncode


def prepare_workspace(source: Path, destination: Path, *, reuse: bool = False) -> None:
    """Copy the read-only project into an ephemeral writable build directory.

    ``reuse`` preserves a workspace prepared by an earlier fetch phase in the
    same run directory (including npm/devenv state). It never reuses a
    workspace from a different run.
    """
    if reuse and destination.is_dir() and (destination / ".devenv").exists():
        return
    if destination.exists():
        shutil.rmtree(destination)
    ignore = shutil.ignore_patterns(
        ".git",
        ".devenv",
        ".devenv.flake.nix",
        "devenv.local.nix",
        "devenv.local.yaml",
        "devenv.local.yml",
        "flake.local.nix",
        ".envrc",
    )
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(source, destination, symlinks=True, ignore=ignore)


def write_environment_files(workspace: Path, files: Iterable[EnvironmentFile]) -> None:
    for file in files:
        target = workspace / file.path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(file.content, encoding="utf-8")
        target.chmod(0o644)
    for name in ("devenv.local.nix", "devenv.local.yaml", "devenv.local.yml", "flake.local.nix"):
        candidate = workspace / name
        if candidate.exists() or candidate.is_symlink():
            candidate.unlink()
