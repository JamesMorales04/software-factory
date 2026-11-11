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
from .disk import DiskBudget
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
MAX_TOOL_OUTPUT_BYTES = 64 * 1024
MAX_CLOSURE_OUTPUT_BYTES = 16 * 1024 * 1024
STREAM_CHUNK_BYTES = 64 * 1024
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


def _read_capped(stream: Any, limit: int) -> tuple[bytes, bool]:
    """Drain a binary stream while retaining at most ``limit`` bytes."""
    captured = bytearray()
    truncated = False
    while True:
        try:
            chunk = stream.read(STREAM_CHUNK_BYTES)
        except (OSError, ValueError):
            break
        if not chunk:
            break
        remaining = limit - len(captured)
        if remaining > 0:
            take = chunk[:remaining]
            captured.extend(take)
            if len(chunk) > len(take):
                truncated = True
        else:
            truncated = True
    return bytes(captured), truncated


def _drain_to_log(stream: Any, log: "BuildLog") -> None:
    """Stream a binary child stream into the bounded, redacted log."""
    while True:
        try:
            chunk = stream.read(STREAM_CHUNK_BYTES)
        except (OSError, ValueError):
            return
        if not chunk:
            return
        log.write(chunk.decode("utf-8", errors="replace"))


def _terminate_process(process: subprocess.Popen[Any]) -> None:
    try:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=10)
        return
    except (OSError, subprocess.TimeoutExpired):
        pass
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except OSError:
        pass
    try:
        process.wait(timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        pass


def _close_process_pipes(process: subprocess.Popen[Any]) -> None:
    """Close both capture pipes on every exit path, including budget kills."""
    for name in ("stdout", "stderr"):
        stream = getattr(process, name, None)
        if stream is None:
            continue
        try:
            stream.close()
        except (OSError, ValueError):
            pass


def _run_bounded_capture(
    argv: list[str],
    *,
    timeout: int,
    env: dict[str, str],
    failure_code: str,
    stdout_limit: int,
    stderr_limit: int,
    cwd: Path | None = None,
) -> tuple[int, bytes, bytes, bool, bool]:
    """Run one process while bounding both captured streams."""
    import threading

    try:
        process = subprocess.Popen(
            argv,
            cwd=str(cwd) if cwd is not None else None,
            env=environment_without_secrets(env),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,
            start_new_session=True,
        )
    except OSError as error:
        fail(failure_code, f"cannot start {argv[0]}: {error}")
    assert process.stdout is not None
    assert process.stderr is not None
    results: dict[str, tuple[bytes, bool]] = {}

    def reader(name: str, stream: Any, limit: int) -> None:
        results[name] = _read_capped(stream, limit)

    stdout_thread = threading.Thread(
        target=reader, args=("stdout", process.stdout, stdout_limit), daemon=True
    )
    stderr_thread = threading.Thread(
        target=reader, args=("stderr", process.stderr, stderr_limit), daemon=True
    )
    stdout_thread.start()
    stderr_thread.start()
    timed_out = False
    try:
        process.wait(timeout=max(1, timeout))
    except subprocess.TimeoutExpired:
        timed_out = True
        _terminate_process(process)
    stdout_thread.join(10)
    stderr_thread.join(10)
    _close_process_pipes(process)
    if timed_out:
        fail(failure_code, "environment command exceeded its wall-clock bound")
    stdout, stdout_truncated = results.get("stdout", (b"", False))
    stderr, stderr_truncated = results.get("stderr", (b"", False))
    return (
        process.returncode if process.returncode is not None else -1,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    )


def _tool_version(argv: list[str], pattern: re.Pattern[str], label: str) -> str:
    returncode, stdout, stderr, stdout_truncated, _stderr_truncated = _run_bounded_capture(
        argv,
        timeout=60,
        env=dict(os.environ),
        failure_code="ENVIRONMENT_BUILDER_UNAVAILABLE",
        stdout_limit=MAX_TOOL_OUTPUT_BYTES,
        stderr_limit=MAX_TOOL_OUTPUT_BYTES,
    )
    if stdout_truncated:
        fail("ENVIRONMENT_BUILDER_UNAVAILABLE", f"{label} output exceeds its bound")
    output = f"{stdout.decode('utf-8', errors='replace')}\n{stderr.decode('utf-8', errors='replace')}"
    match = pattern.search(output)
    if returncode != 0 or match is None:
        fail("ENVIRONMENT_BUILDER_UNAVAILABLE", f"{label} did not report a pinned version")
    return match.group(1)


def nix_eval_json(
    expression: str,
    *,
    env: dict[str, str],
    timeout: int,
    log: BuildLog,
    failure_code: str = "ENVIRONMENT_PIN_MISSING",
) -> Any:
    """Evaluate one bounded pure Nix expression and return decoded JSON."""
    argv = ["nix", "eval", "--json", "--expr", expression]
    returncode, stdout, stderr, stdout_truncated, stderr_truncated = _run_bounded_capture(
        argv,
        timeout=timeout,
        env=env,
        failure_code=failure_code,
        stdout_limit=MAX_CLOSURE_OUTPUT_BYTES,
        stderr_limit=MAX_TOOL_OUTPUT_BYTES,
    )
    rendered = stdout.decode("utf-8", errors="replace")
    diagnostics = stderr.decode("utf-8", errors="replace")
    log.write(f"$ nix eval --json --expr <package metadata>\n{rendered}{diagnostics}")
    if stdout_truncated or stderr_truncated:
        fail(failure_code, "package metadata evaluation output exceeds its bound")
    if returncode != 0:
        fail(
            failure_code,
            f"package metadata evaluation failed: {tail(diagnostics) or tail(rendered)}",
        )
    try:
        return json.loads(rendered)
    except json.JSONDecodeError as error:
        fail(failure_code, f"package metadata evaluation returned invalid JSON: {error}")


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
    disk_budget: DiskBudget | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run an argv vector with a wall-clock bound and a bounded log."""
    import threading

    environment = environment_without_secrets(env)
    started = time.monotonic()
    log.write(f"$ {' '.join(argv)}\n")
    try:
        process = subprocess.Popen(
            argv,
            cwd=str(cwd),
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=False,
            start_new_session=True,
        )
    except OSError as error:
        fail(failure_code, f"cannot start {argv[0]}: {error}")
    assert process.stdout is not None
    reader_error: list[BaseException] = []

    def drain() -> None:
        try:
            _drain_to_log(process.stdout, log)
        except BaseException as error:  # pragma: no cover - defensive thread guard
            reader_error.append(error)

    reader = threading.Thread(target=drain, daemon=True)
    reader.start()
    monitor = disk_budget.monitor(process) if disk_budget else None
    if monitor:
        monitor.start()
    timed_out = False
    try:
        process.wait(timeout=max(1, timeout - int(time.monotonic() - started)))
    except subprocess.TimeoutExpired:
        timed_out = True
        _terminate_process(process)
    finally:
        if monitor:
            monitor.stop()
        reader.join(10)
        _close_process_pipes(process)
    if monitor and monitor.exceeded:
        if monitor.incomplete:
            fail(
                "ENVIRONMENT_RESOURCE_LIMIT",
                "the builder could not complete the disk scan; the frozen disk bound "
                "cannot be verified (exact=false)",
            )
        fail(
            "ENVIRONMENT_RESOURCE_LIMIT",
            f"observed builder disk usage {monitor.observed_bytes} bytes exceeds the frozen "
            f"{disk_budget.limit_mib if disk_budget else 0} MiB bound; exact=false",
        )
    if reader_error:
        fail(failure_code, f"cannot read environment command output: {reader_error[0]}")
    if timed_out:
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
        "trusted-public-keys = cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY= devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=",
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
    }
    for name in ("FACTORY_ENVIRONMENT_BUILDER_IMAGE", "FACTORY_ENVIRONMENT_BUILDER_DIGEST",
                 "FACTORY_ENVIRONMENT_BUILDER_PLATFORM", "FACTORY_ENVIRONMENT_BUILDER_POLICY_REVISION",
                 "FACTORY_ENVIRONMENT_NIX_VERSION", "FACTORY_ENVIRONMENT_DEVENV_VERSION",
                 "FACTORY_ENVIRONMENT_BUILDER_REQUEST_ID", "CHROMIUM_EXE",
                 "FACTORY_PLAYWRIGHT_CORE"):
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


def closure_manifest(root: str) -> list[dict[str, Any]]:
    returncode, stdout, stderr, stdout_truncated, _stderr_truncated = _run_bounded_capture(
        ["nix", "path-info", "--json", "--recursive", "--offline", root],
        timeout=300,
        env=dict(os.environ),
        failure_code="ENVIRONMENT_BUILD_FAILED",
        stdout_limit=MAX_CLOSURE_OUTPUT_BYTES,
        stderr_limit=MAX_TOOL_OUTPUT_BYTES,
    )
    if stdout_truncated:
        fail("ENVIRONMENT_BUILD_FAILED", "environment closure manifest exceeds its bound")
    if returncode != 0:
        fail(
            "ENVIRONMENT_BUILD_FAILED",
            f"cannot read the environment closure: {tail(stderr.decode('utf-8', errors='replace'), 2_000)}",
        )
    try:
        raw = json.loads(stdout.decode("utf-8", errors="replace"))
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
                "deriver": str(info.get("deriver", "")),
            }
        )
    return manifest


def closure_manifest_sha256(manifest: list[dict[str, Any]]) -> str:
    return sha256_text(canonical_json(manifest))


def environment_digest(
    spec_sha256: str,
    platform: str,
    root: str,
    manifest: list[dict[str, Any]],
    sbom_sha256: str = "",
) -> str:
    return sha256_text(
        canonical_json(
            {
                "specSha256": spec_sha256,
                "platform": platform,
                "storePath": root,
                "closure": manifest,
                "sbomSha256": sbom_sha256,
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


def check_command_available(
    workspace: Path,
    cache: Path,
    executable: str,
    *,
    offline: bool,
    resources: dict[str, int],
    timeout: int,
    proxy: str | None = None,
) -> tuple[bool, str]:
    """Check that the realised devShell provides an executable.

    The argv vector passes the executable as `$1`; it is never interpolated
    into shell source.
    """
    env = child_environment(
        workspace,
        cache,
        offline=offline,
        resources=resources,
        proxy=proxy,
    )
    argv = _environment_command(
        workspace,
        ["sh", "-c", 'command -v "$1"', "sh", executable],
        offline=offline,
    )
    returncode, stdout, stderr, stdout_truncated, stderr_truncated = _run_bounded_capture(
        argv,
        cwd=workspace,
        timeout=timeout,
        env=env,
        failure_code="ENVIRONMENT_PLAN_INVALID",
        stdout_limit=MAX_TOOL_OUTPUT_BYTES,
        stderr_limit=MAX_TOOL_OUTPUT_BYTES,
    )
    output = (
        stdout.decode("utf-8", errors="replace")
        + stderr.decode("utf-8", errors="replace")
    )
    if stdout_truncated or stderr_truncated:
        return False, tail(output, 2_000)
    return returncode == 0 and bool(stdout.strip()), tail(output, 2_000)


def realize_environment(
    workspace: Path,
    cache: Path,
    *,
    offline: bool,
    resources: dict[str, int],
    timeout: int,
    log: BuildLog,
    proxy: str | None,
    disk_budget: DiskBudget | None = None,
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
        disk_budget=disk_budget,
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


MAX_CHECK_STREAM_BYTES = 256 * 1024


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
    disk_budget: DiskBudget | None = None,
) -> tuple[int, str, str, bool, bool]:
    """Run one frozen check and retain bounded stdout/stderr separately.

    The broker/Worker check receipt must preserve the exact exit code and
    truncation state. Output is streamed with a bounded retention per stream;
    the rest is drained and discarded, so a noisy check cannot allocate
    unbounded memory.
    """
    import threading

    env = child_environment(workspace, cache, offline=True, resources=resources)
    command_argv = list(argv)
    if cwd.resolve() != workspace.resolve():
        command_argv = ["env", "-C", str(cwd), *command_argv]
    command = _environment_command(workspace, command_argv, offline=True)
    log.write(f"$ {' '.join(command)}\n")
    started = time.monotonic()
    try:
        process = subprocess.Popen(
            command,
            cwd=str(workspace),
            env=environment_without_secrets(env),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,
            start_new_session=True,
        )
    except OSError as error:
        fail("ENVIRONMENT_BUILD_FAILED", f"cannot start {command[0]}: {error}")
    assert process.stdout is not None
    assert process.stderr is not None
    results: dict[str, tuple[bytes, bool]] = {}

    def reader(name: str, stream: Any) -> None:
        results[name] = _read_capped(stream, MAX_CHECK_STREAM_BYTES)

    stdout_thread = threading.Thread(
        target=reader, args=("stdout", process.stdout), daemon=True
    )
    stderr_thread = threading.Thread(
        target=reader, args=("stderr", process.stderr), daemon=True
    )
    stdout_thread.start()
    stderr_thread.start()
    monitor = disk_budget.monitor(process) if disk_budget else None
    if monitor:
        monitor.start()
    timed_out = False
    try:
        process.wait(timeout=max(1, timeout - int(time.monotonic() - started)))
    except subprocess.TimeoutExpired:
        timed_out = True
        _terminate_process(process)
    finally:
        if monitor:
            monitor.stop()
        stdout_thread.join(10)
        stderr_thread.join(10)
        _close_process_pipes(process)
    if monitor and monitor.exceeded:
        if monitor.incomplete:
            fail(
                "ENVIRONMENT_RESOURCE_LIMIT",
                "the builder could not complete the disk scan; the frozen disk bound "
                "cannot be verified (exact=false)",
            )
        fail(
            "ENVIRONMENT_RESOURCE_LIMIT",
            f"observed builder disk usage {monitor.observed_bytes} bytes exceeds the frozen "
            f"{disk_budget.limit_mib if disk_budget else 0} MiB bound; exact=false",
        )
    if timed_out:
        fail(
            "ENVIRONMENT_BUILD_TIMEOUT",
            f"environment check exceeded its {timeout}s wall-clock bound",
        )
    stdout_bytes, stdout_truncated = results.get("stdout", (b"", False))
    stderr_bytes, stderr_truncated = results.get("stderr", (b"", False))
    stdout = stdout_bytes.decode("utf-8", errors="replace")
    stderr = stderr_bytes.decode("utf-8", errors="replace")
    log.write(stdout)
    if stderr:
        log.write(stderr)
    if process.returncode != 0 and process.returncode != expect_exit:
        diagnostic = log.diagnostic
        if any(marker in diagnostic.lower() for marker in OFFLINE_MARKERS):
            fail(
                "ENVIRONMENT_NETWORK_DENIED",
                "the offline check needs a store path that the broker did not prefetch",
            )
    return process.returncode, stdout, stderr, stdout_truncated, stderr_truncated


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
