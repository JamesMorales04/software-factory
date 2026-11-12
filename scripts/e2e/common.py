"""Shared primitives for the stage-addressed Factory Compose scenario runner.

This module is development/test infrastructure. It is not part of the installed
Factory runtime and it never starts a paid provider session. The runner keeps
non-deterministic boundaries explicit: the model gateway is a local fixture,
GitHub is not contacted, and every stage records the exact identities it used.

The implementation deliberately depends only on the Python standard library so
that the harness can run inside the existing container-native development image
without a new host dependency.
"""

from __future__ import annotations

import http.cookiejar
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_TIMEOUT = 600
RUN_SCHEMA = "factory-e2e-run-v1"
RUN_SENTINEL = "run.json"


class ScenarioError(RuntimeError):
    """A bounded scenario failure with an operator-readable message."""


def now_utc() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def log(message: str) -> None:
    print(f"[{datetime.now().strftime('%H:%M:%S')}] {message}", file=sys.stderr, flush=True)


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_json(path: Path, value: Any) -> None:
    ensure_dir(path.parent)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ScenarioError(f"required JSON file is missing: {path}") from error
    except json.JSONDecodeError as error:
        raise ScenarioError(f"invalid JSON in {path}: {error}") from error


def tail(value: str, limit: int = 12_000) -> str:
    return value if len(value) <= limit else value[-limit:]


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def port_is_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        try:
            listener.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True


def project_name(value: str) -> str:
    normalized = value.strip()
    if not normalized or normalized == "software-factory" or "e2e" not in normalized:
        raise ScenarioError(
            "the Compose project must be dedicated, non-empty, contain 'e2e' and "
            f"must not be 'software-factory': {value!r}"
        )
    if not all(character.isalnum() or character in "-_" for character in normalized):
        raise ScenarioError(f"the Compose project contains invalid characters: {value!r}")
    return normalized


def _bounded_run(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str] | None = None,
    timeout: int = 60,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            command,
            cwd=cwd,
            env=env,
            text=True,
            capture_output=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ScenarioError(f"command failed or timed out: {' '.join(command)}: {error}") from error


def run_sentinel(config: "Config") -> Path:
    return config.run_dir / RUN_SENTINEL


def ensure_run_identity(config: "Config", state: "State") -> dict[str, Any]:
    """Create or validate the ownership marker for a run directory.

    A run directory is disposable, but it may still be pointed at an arbitrary
    path by mistake. The sentinel is the only proof that cleanup/`--fresh` may
    remove the known children; a directory without it is refused.
    """

    sentinel = run_sentinel(config)
    if sentinel.exists():
        payload = read_json(sentinel)
        if payload.get("schema") != RUN_SCHEMA:
            raise ScenarioError(f"run ownership marker has an invalid schema: {sentinel}")
        for key, value in (
            ("runId", config.run_id),
            ("project", config.project),
            ("port", config.port),
        ):
            if payload.get(key) != value:
                raise ScenarioError(
                    f"run ownership marker mismatch for {key}: {payload.get(key)!r} != {value!r}"
                )
        return payload
    if config.run_dir.exists() and any(config.run_dir.iterdir()):
        raise ScenarioError(
            f"run directory is not owned by this harness: {config.run_dir}. "
            "Use a new --run-dir or remove it manually."
        )
    payload = {
        "schema": RUN_SCHEMA,
        "runId": config.run_id,
        "project": config.project,
        "port": config.port,
        "scenario": config.scenario,
        "createdAt": now_utc(),
    }
    ensure_dir(config.run_dir)
    os.chmod(config.run_dir, 0o700)
    write_json(sentinel, payload)
    state.set("run_sentinel", str(sentinel))
    return payload


def require_owned_run(config: "Config") -> None:
    sentinel = run_sentinel(config)
    if not sentinel.is_file():
        raise ScenarioError(
            f"refusing destructive cleanup: {config.run_dir} has no {RUN_SENTINEL} marker"
        )
    payload = read_json(sentinel)
    if payload.get("schema") != RUN_SCHEMA or payload.get("runId") != config.run_id:
        raise ScenarioError(f"refusing destructive cleanup: invalid run marker in {config.run_dir}")


def command_version(program: str) -> str | None:
    try:
        result = subprocess.run(
            [program, "--version"],
            text=True,
            capture_output=True,
            timeout=20,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    output = (result.stdout or result.stderr).strip().splitlines()
    return output[0] if output else None


@dataclass
class Config:
    root: Path
    run_id: str
    run_dir: Path
    artifacts_dir: Path
    project: str
    port: int
    provider: str
    github: str
    cassette: Path | None
    browser: str
    from_stage: str
    to_stage: str
    keep: bool
    snapshot: bool
    restore: bool
    snapshot_dir: Path
    no_build: bool
    timeout: int
    scenario: str

    @property
    def base(self) -> str:
        return f"http://127.0.0.1:{self.port}"

    @property
    def git_dir(self) -> Path:
        return self.run_dir / "git"

    @property
    def model_dir(self) -> Path:
        return self.run_dir / "model"

    @property
    def cookie_jar(self) -> Path:
        return self.run_dir / "cookies.txt"

    @property
    def state_path(self) -> Path:
        return self.run_dir / "state.json"

    def environment(self) -> dict[str, str]:
        environment = os.environ.copy()
        if self.cassette is not None:
            environment["FACTORY_E2E_MODEL_CASSETTE"] = "/context/cassette.json"
        environment.update(
            {
                "COMPOSE_PROJECT_NAME": self.project,
                "FACTORY_PORT": str(self.port),
                "FACTORY_E2E_GIT_DIR": str(self.git_dir / "server"),
                "FACTORY_E2E_MODEL_DIR": str(self.model_dir),
                "FACTORY_PROVIDER_KIND": "fixture",
                "PODMAN_COMPOSE_PROVIDER": os.environ.get(
                    "PODMAN_COMPOSE_PROVIDER", "/usr/bin/podman-compose"
                ),
                "TMPDIR": str(self.run_dir / "tmp"),
            }
        )
        ensure_dir(Path(environment["TMPDIR"]))
        return environment


@dataclass
class State:
    path: Path
    data: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def load_or_create(cls, path: Path) -> "State":
        if path.exists():
            return cls(path, read_json(path))
        return cls(path, {"schema": "factory-e2e-state-v1", "createdAt": now_utc()})

    def save(self) -> None:
        self.data["updatedAt"] = now_utc()
        write_json(self.path, self.data)

    def require(self, *keys: str) -> None:
        missing = [key for key in keys if not self.data.get(key)]
        if missing:
            raise ScenarioError(
                "resume state is incomplete; missing "
                + ", ".join(missing)
                + f". Run the earlier stage first or use a complete --run-dir ({self.path})."
            )

    def get(self, key: str, default: Any = None) -> Any:
        return self.data.get(key, default)

    def set(self, key: str, value: Any) -> None:
        self.data[key] = value

    def append(self, key: str, value: Any) -> None:
        current = self.data.setdefault(key, [])
        if not isinstance(current, list):
            raise ScenarioError(f"state key {key!r} is not a list")
        current.append(value)


class Compose:
    """A thin, logged wrapper around the canonical Compose command."""

    def __init__(self, config: Config):
        self.config = config
        self.log_path = ensure_dir(config.artifacts_dir / "logs") / "commands.log"

    def command(self) -> list[str]:
        return [
            "podman",
            "compose",
            "-p",
            self.config.project,
            "-f",
            "compose.yaml",
            "-f",
            "compose.e2e.yaml",
        ]

    def run(
        self,
        arguments: Iterable[str],
        *,
        check: bool = True,
        log_name: str | None = None,
    ) -> subprocess.CompletedProcess[str]:
        args = list(arguments)
        command = self.command() + args
        started = time.monotonic()
        rendered = " ".join(command)
        log(f"$ {rendered}")
        try:
            result = subprocess.run(
                command,
                cwd=self.config.root,
                env=self.config.environment(),
                text=True,
                capture_output=True,
                timeout=self.config.timeout,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise ScenarioError(f"command timed out after {self.config.timeout}s: {rendered}") from error
        except FileNotFoundError as error:
            raise ScenarioError(f"command is not installed: {command[0]}") from error
        elapsed = time.monotonic() - started
        with self.log_path.open("a", encoding="utf-8") as handle:
            handle.write(f"\n$ {rendered}\n{tail(result.stdout)}\n{tail(result.stderr)}\n")
            handle.write(f"exit={result.returncode} elapsed={elapsed:.3f}s\n")
        if log_name:
            ensure_dir(self.config.artifacts_dir / "logs").joinpath(f"{log_name}.log").write_text(
                f"$ {rendered}\n\nSTDOUT\n{result.stdout}\n\nSTDERR\n{result.stderr}\n",
                encoding="utf-8",
            )
        if check and result.returncode != 0:
            raise ScenarioError(
                f"command failed ({result.returncode}): {rendered}\n"
                f"stdout:\n{tail(result.stdout)}\nstderr:\n{tail(result.stderr)}"
            )
        return result

    def down(self, *, volumes: bool) -> None:
        arguments = ["down"] + (["-v"] if volumes else [])
        self.run(arguments, check=False, log_name="compose-down")

    def up(self, *, build: bool) -> None:
        arguments = ["up", "-d"] + (["--build"] if build else [])
        self.run(arguments, check=False, log_name="compose-up")

    def start(self) -> None:
        self.run(["start"], check=False, log_name="compose-start")

    def stop(self) -> None:
        self.run(["stop"], check=False, log_name="compose-stop")

    def core_container(self) -> str:
        result = _bounded_run(
            [
                "podman",
                "ps",
                "--filter",
                f"label=com.docker.compose.project={self.config.project}",
                "--filter",
                "label=com.docker.compose.service=core",
                "--format",
                "{{.Names}}",
            ],
            cwd=self.config.root,
            env=self.config.environment(),
            timeout=30,
        )
        names = [line.strip() for line in result.stdout.splitlines() if line.strip()]
        return names[0] if names else ""

    def service_container(self, service: str) -> str:
        result = _bounded_run(
            [
                "podman",
                "ps",
                "-a",
                "--filter",
                f"label=com.docker.compose.project={self.config.project}",
                "--format",
                "{{.Names}}",
            ],
            cwd=self.config.root,
            env=self.config.environment(),
            timeout=30,
        )
        for name in result.stdout.splitlines():
            if f"_{service}_" in name:
                return name.strip()
        return ""

    def exec_core(self, arguments: Iterable[str], *, check: bool = True) -> str:
        core = self.core_container()
        if not core:
            raise ScenarioError("Core container is not present")
        result = subprocess.run(
            ["podman", "exec", core, *arguments],
            cwd=self.config.root,
            env=self.config.environment(),
            text=True,
            capture_output=True,
            timeout=self.config.timeout,
            check=False,
        )
        if check and result.returncode != 0:
            raise ScenarioError(
                f"podman exec {' '.join(arguments)} failed ({result.returncode}): "
                f"{tail(result.stderr)}"
            )
        return result.stdout

    def volume_names(self) -> list[str]:
        result = _bounded_run(
            [
                "podman",
                "volume",
                "ls",
                "--filter",
                f"label=com.docker.compose.project={self.config.project}",
                "--format",
                "{{.Name}}",
            ],
            cwd=self.config.root,
            env=self.config.environment(),
            timeout=30,
        )
        return [line.strip() for line in result.stdout.splitlines() if line.strip()]


class Api:
    """Cookie/CSRF-aware client for the real Core HTTP API."""

    def __init__(self, config: Config):
        self.config = config
        self.jar = http.cookiejar.LWPCookieJar(str(config.cookie_jar))
        if config.cookie_jar.exists():
            try:
                self.jar.load(ignore_discard=True, ignore_expires=True)
            except (OSError, http.cookiejar.LoadError):
                # A stale jar is not an authorization bypass; the stage that
                # needs the session will fail closed with a Core 401/403.
                self.jar = http.cookiejar.LWPCookieJar(str(config.cookie_jar))
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.jar)
        )
        self.last_response = ""

    def _save(self) -> None:
        ensure_dir(self.config.cookie_jar.parent)
        try:
            self.jar.save(ignore_discard=True, ignore_expires=True)
            os.chmod(self.config.cookie_jar, 0o600)
        except OSError:
            pass

    def csrf(self) -> str:
        for cookie in self.jar:
            if cookie.name == "factory_csrf":
                return cookie.value
        return ""

    def request(
        self,
        method: str,
        path: str,
        body: Any | None = None,
        *,
        origin: bool = True,
        timeout: int | None = None,
    ) -> str:
        data = None
        headers = {"Accept": "application/json"}
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if origin:
            headers["Origin"] = self.config.base
        if method.upper() not in {"GET", "HEAD", "OPTIONS"}:
            csrf = self.csrf()
            if csrf:
                headers["x-factory-csrf"] = csrf
        request = urllib.request.Request(
            f"{self.config.base}{path}",
            data=data,
            headers=headers,
            method=method.upper(),
        )
        try:
            with self.opener.open(request, timeout=timeout or 60) as response:
                payload = response.read().decode("utf-8")
                self.last_response = payload
        except urllib.error.HTTPError as error:
            payload = error.read().decode("utf-8", errors="replace")
            self.last_response = payload
            raise ScenarioError(
                f"HTTP {error.code} for {method.upper()} {path}: {tail(payload, 4000)}"
            ) from error
        except urllib.error.URLError as error:
            raise ScenarioError(f"HTTP transport failure for {method.upper()} {path}: {error}") from error
        self._save()
        return payload

    def json(self, method: str, path: str, body: Any | None = None, **kwargs: Any) -> Any:
        payload = self.request(method, path, body, **kwargs)
        try:
            return json.loads(payload)
        except json.JSONDecodeError as error:
            raise ScenarioError(f"Core returned non-JSON for {method.upper()} {path}: {tail(payload, 2000)}") from error

    def get(self, path: str) -> Any:
        return self.json("GET", path)

    def post(self, path: str, body: Any | None = None) -> Any:
        return self.json("POST", path, body)

    def put(self, path: str, body: Any | None = None) -> Any:
        return self.json("PUT", path, body)

    def text(self, path: str) -> str:
        return self.request("GET", path)


def wait_url(url: str, timeout: int, *, interval: float = 1.0) -> None:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=5) as response:
                if response.status < 500:
                    return
        except (urllib.error.URLError, OSError, TimeoutError) as error:
            last_error = error
        time.sleep(interval)
    raise ScenarioError(f"timed out after {timeout}s waiting for {url}: {last_error}")


def wait_core(compose: Compose, url: str, timeout: int, *, interval: float = 1.0) -> None:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            compose.exec_core(["curl", "-fsS", url], check=True)
            return
        except (ScenarioError, OSError, subprocess.TimeoutExpired) as error:
            last_error = error
        time.sleep(interval)
    raise ScenarioError(f"timed out after {timeout}s waiting for Core to reach {url}: {last_error}")


def wait_until(predicate, timeout: int, description: str, *, interval: float = 1.0) -> Any:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except Exception as error:  # noqa: BLE001 - the caller needs the last diagnostic
            last_error = error
        time.sleep(interval)
    raise ScenarioError(f"timed out after {timeout}s waiting for {description}: {last_error}")


def assert_equal(actual: Any, expected: Any, description: str) -> None:
    if actual != expected:
        raise ScenarioError(f"{description}: expected {expected!r}, got {actual!r}")


def assert_in(value: str, expected: str, description: str) -> None:
    if expected not in value:
        raise ScenarioError(f"{description}: expected {expected!r} in {tail(value, 4000)!r}")


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ScenarioError(message)


def doctor(config: Config, compose: Compose) -> dict[str, Any]:
    """Validate the local prerequisites without starting the stack or mutating state."""

    checks: list[dict[str, Any]] = []

    def check(name: str, ok: bool, detail: str) -> None:
        checks.append({"name": name, "ok": ok, "detail": detail})

    for program in ("python3", "git", "podman"):
        version = command_version(program)
        check(f"command:{program}", version is not None, version or "not found")

    podman_present = shutil.which("podman") is not None
    compose_version = command_version("podman")
    check("podman-compose", podman_present, compose_version or "podman not found")
    provider = os.environ.get("PODMAN_COMPOSE_PROVIDER", "/usr/bin/podman-compose")
    check("compose-provider", Path(provider).exists(), provider)
    compose_probe = None
    if podman_present and Path(provider).exists():
        try:
            compose_probe = _bounded_run(
                ["podman", "compose", "version"],
                cwd=config.root,
                env=config.environment(),
                timeout=20,
            )
        except ScenarioError:
            compose_probe = None
    check(
        "compose:version",
        compose_probe is not None and compose_probe.returncode == 0,
        "podman compose version unavailable" if compose_probe is None else compose_probe.stdout.strip(),
    )

    check("port:free", port_is_free(config.port), f"127.0.0.1:{config.port}")
    check("repo:compose", (config.root / "compose.yaml").is_file(), str(config.root / "compose.yaml"))
    check(
        "repo:compose-e2e",
        (config.root / "compose.e2e.yaml").is_file(),
        str(config.root / "compose.e2e.yaml"),
    )
    check("repo:office-e2e", (config.root / "office/e2e-browser").is_dir(), "office/e2e-browser")
    ensure_dir(config.artifacts_dir)
    check("artifacts:writable", os.access(config.artifacts_dir, os.W_OK), str(config.artifacts_dir))

    if podman_present and Path(provider).exists() and (config.root / "compose.yaml").is_file():
        try:
            configuration = compose.run(["config"], check=False, log_name="doctor-compose-config")
            check("compose:config", configuration.returncode == 0, f"exit {configuration.returncode}")
        except ScenarioError as error:
            check("compose:config", False, str(error))
    else:
        check("compose:config", False, "Compose prerequisites are missing")

    return {
        "schema": "factory-e2e-doctor-v1",
        "ok": all(item["ok"] for item in checks),
        "runId": config.run_id,
        "project": config.project,
        "port": config.port,
        "checks": checks,
    }


def snapshot_volumes(config: Config, compose: Compose) -> dict[str, Any]:
    """Export Compose volumes with private permissions and a validated index."""

    ensure_dir(config.run_dir)
    os.chmod(config.run_dir, 0o700)
    directory = ensure_dir(config.snapshot_dir)
    os.chmod(directory, 0o700)
    volumes = compose.volume_names()
    if not volumes:
        raise ScenarioError("cannot snapshot: no Compose volumes were found")
    compose.stop()
    index: dict[str, Any] = {
        "schema": "factory-e2e-volume-snapshot-v1",
        "createdAt": now_utc(),
        "project": config.project,
        "volumes": [],
    }
    for volume in volumes:
        archive = directory / f"{volume}.tar"
        result = _bounded_run(
            ["podman", "volume", "export", volume, "--output", str(archive)],
            cwd=config.root,
            env=config.environment(),
            timeout=config.timeout,
        )
        if result.returncode != 0:
            raise ScenarioError(f"failed to export volume {volume}: {tail(result.stderr)}")
        os.chmod(archive, 0o600)
        inspect = _bounded_run(
            ["podman", "volume", "inspect", volume],
            cwd=config.root,
            env=config.environment(),
            timeout=30,
        )
        labels: dict[str, str] = {}
        if inspect.returncode == 0:
            try:
                labels = json.loads(inspect.stdout)[0].get("Labels", {}) or {}
            except (json.JSONDecodeError, IndexError, KeyError):
                labels = {}
        index["volumes"].append(
            {"name": volume, "archive": archive.name, "labels": labels}
        )
    write_json(directory / "index.json", index)
    os.chmod(directory / "index.json", 0o600)
    return index


def _validate_snapshot_index(config: Config, index: dict[str, Any]) -> list[dict[str, Any]]:
    if index.get("schema") != "factory-e2e-volume-snapshot-v1":
        raise ScenarioError("snapshot index has an invalid schema")
    if index.get("project") != config.project:
        raise ScenarioError(
            f"snapshot belongs to project {index.get('project')!r}, not {config.project!r}"
        )
    entries = index.get("volumes")
    if not isinstance(entries, list) or not entries:
        raise ScenarioError("snapshot index has no volumes")
    snapshot_root = config.snapshot_dir.resolve()
    validated: list[dict[str, Any]] = []
    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise ScenarioError("snapshot volume entry is not an object")
        name = entry.get("name")
        archive_name = entry.get("archive")
        if not isinstance(name, str) or not name or Path(name).name != name:
            raise ScenarioError(f"snapshot volume name is unsafe: {name!r}")
        if name in seen:
            raise ScenarioError(f"snapshot contains a duplicate volume: {name}")
        seen.add(name)
        if not name.startswith(config.project):
            raise ScenarioError(f"snapshot volume {name!r} is not owned by {config.project!r}")
        if not isinstance(archive_name, str) or Path(archive_name).name != archive_name:
            raise ScenarioError(f"snapshot archive name is unsafe: {archive_name!r}")
        archive = (config.snapshot_dir / archive_name).resolve()
        if archive.parent != snapshot_root:
            raise ScenarioError(f"snapshot archive escapes the snapshot directory: {archive}")
        if not archive.is_file():
            raise ScenarioError(f"snapshot archive is missing: {archive}")
        labels = entry.get("labels") or {}
        if not isinstance(labels, dict):
            raise ScenarioError("snapshot volume labels are invalid")
        validated.append({"name": name, "archive": archive, "labels": labels})
    return validated


def restore_volumes(config: Config, compose: Compose) -> dict[str, Any]:
    """Validate the full snapshot before any destructive operation, then restore."""

    index = read_json(config.snapshot_dir / "index.json")
    entries = _validate_snapshot_index(config, index)
    # Preflight target names before removing anything. A same-name volume owned
    # by this project is the disposable stack we are replacing; anything else is
    # a foreign volume and must block the restore.
    for entry in entries:
        existing = _bounded_run(
            ["podman", "volume", "inspect", entry["name"]],
            cwd=config.root,
            env=config.environment(),
            timeout=30,
        )
        if existing.returncode == 0:
            try:
                labels = json.loads(existing.stdout)[0].get("Labels", {}) or {}
            except (json.JSONDecodeError, IndexError, KeyError):
                labels = {}
            owner = labels.get("com.docker.compose.project")
            if owner != config.project:
                raise ScenarioError(
                    f"refusing to replace volume {entry['name']!r} with owner "
                    f"{owner!r}; expected {config.project!r}"
                )
    compose.run(["down", "-v"], check=False, log_name="restore-down")
    for entry in entries:
        name = entry["name"]
        archive = entry["archive"]
        existing = _bounded_run(
            ["podman", "volume", "inspect", name],
            cwd=config.root,
            env=config.environment(),
            timeout=30,
        )
        if existing.returncode == 0:
            raise ScenarioError(f"refusing to import into existing volume: {name}")
        create = ["podman", "volume", "create", name]
        for key, value in (entry.get("labels") or {}).items():
            create.extend(["--label", f"{key}={value}"])
        created = _bounded_run(
            create,
            cwd=config.root,
            env=config.environment(),
            timeout=30,
        )
        if created.returncode != 0:
            raise ScenarioError(f"failed to create volume {name}: {tail(created.stderr)}")
        imported = _bounded_run(
            ["podman", "volume", "import", name, str(archive)],
            cwd=config.root,
            env=config.environment(),
            timeout=config.timeout,
        )
        if imported.returncode != 0:
            raise ScenarioError(f"failed to import volume {name}: {tail(imported.stderr)}")
    return index


def collect_diagnostics(
    config: Config,
    compose: Compose,
    state: State,
    api: Api | None,
) -> None:
    directory = ensure_dir(config.artifacts_dir / "diagnostics")
    commands = [
        (["podman", "ps", "-a", "--filter", f"label=com.docker.compose.project={config.project}"], "podman-ps.txt"),
        (["podman", "volume", "ls", "--filter", f"label=com.docker.compose.project={config.project}"], "podman-volumes.txt"),
    ]
    for command, name in commands:
        try:
            result = _bounded_run(
                command,
                cwd=config.root,
                env=config.environment(),
                timeout=30,
            )
            body = f"$ {' '.join(command)}\n\n{result.stdout}\n\nSTDERR\n{result.stderr}\n"
        except ScenarioError as error:
            body = f"$ {' '.join(command)}\ndiagnostic failed: {error}\n"
        directory.joinpath(name).write_text(body, encoding="utf-8")
    for service in ("core", "supervisor", "worker", "runtime", "model-fixture", "git-fixture"):
        name = compose.service_container(service)
        if not name:
            continue
        try:
            result = _bounded_run(
                ["podman", "logs", "--tail", "240", name],
                cwd=config.root,
                env=config.environment(),
                timeout=30,
            )
            body = f"container={name}\n\n{result.stdout}\n\nSTDERR\n{result.stderr}\n"
        except ScenarioError as error:
            body = f"container={name}\ndiagnostic failed: {error}\n"
        directory.joinpath(f"logs-{service}.txt").write_text(body, encoding="utf-8")
    if api is not None:
        endpoints = {
            "workers.json": "/workers",
            "projects.json": "/projects",
            "candidates.json": "/candidates",
        }
        for name, path in endpoints.items():
            try:
                directory.joinpath(name).write_text(
                    json.dumps(api.get(path), indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
            except Exception as error:  # noqa: BLE001 - diagnostics must never mask the primary failure
                directory.joinpath(f"{name}.error.txt").write_text(str(error), encoding="utf-8")
        for key, endpoint in (
            ("intake_id", "/intake/requests/{id}"),
            ("campaign_id", "/campaigns/{id}"),
            ("candidate_id", "/quality/candidates/{id}/gate"),
            ("candidate_id", "/quality/candidates/{id}/receipts"),
        ):
            identity = state.get(key)
            if not identity:
                continue
            try:
                path = endpoint.format(id=identity)
                safe = path.strip("/").replace("/", "-")
                directory.joinpath(f"{safe}.json").write_text(
                    json.dumps(api.get(path), indent=2, sort_keys=True) + "\n",
                    encoding="utf-8",
                )
            except Exception as error:  # noqa: BLE001
                directory.joinpath(f"{key}-{endpoint}.error.txt").write_text(
                    str(error), encoding="utf-8"
                )


def write_manifest(
    config: Config,
    state: State,
    *,
    status: str,
    started_at: str,
    error: str | None = None,
    scenarios_ran: list[str] | None = None,
    candidate_sha: str | None = None,
) -> dict[str, Any]:
    head = _git_output(config.root, ["rev-parse", "HEAD"])
    frozen = candidate_sha or state.get("candidate_sha") or head
    base = _git_output(config.root, ["merge-base", "HEAD", "origin/main"]) or head
    dirty = bool(_git_output(config.root, ["status", "--porcelain"]))
    head_changed = frozen != head
    if head_changed and status == "PASS":
        status = "FAIL"
        error = error or f"manifest HEAD changed: bound {frozen}, observed {head}"
    manifest = {
        "schema": "factory-e2e-scenario-manifest-v1",
        "status": status,
        "scenario": config.scenario,
        "runId": config.run_id,
        "startedAt": started_at,
        "finishedAt": now_utc(),
        "base": base,
        "candidate": f"{frozen}-dirty" if dirty else frozen,
        "head": head,
        "headChanged": head_changed,
        "dirty": dirty,
        "project": config.project,
        "port": config.port,
        "provider": config.provider,
        "github": config.github,
        "browser": config.browser,
        "paidInference": False,
        "security": {
            "mode": "TRUSTED_LOCAL",
            "isolated": False,
            "certified": False,
        },
        "stages": state.get("stage_results", []),
        "scenariosRan": scenarios_ran or [config.from_stage, config.to_stage],
        "state": {
            key: value
            for key, value in state.data.items()
            if key
            in {
                "project_id",
                "action_id",
                "profile_id",
                "profile_revision_id",
                "intake_id",
                "intake_action_id",
                "intake_lead_request_id",
                "plan_hash",
                "campaign_id",
                "attempt_id",
                "candidate_id",
                "candidate_commit",
                "gate_outcome",
                "verifier_receipt_id",
                "reviewer_receipt_id",
                "restart_persistence",
                "summary",
            }
        },
        "error": error,
        "limitations": [
            "Development harness only; not independent acceptance.",
            "TRUSTED_LOCAL; no isolation or certification claim.",
            "Provider is a deterministic local fixture or validated cassette; no paid inference ran.",
            "GitHub is not contacted by this scenario.",
            "A partial scenario is PARTIAL, never PASS.",
        ],
        "artifacts": {
            "manifest": str((config.artifacts_dir / "manifest.json").relative_to(config.artifacts_dir)),
            "commands": "logs/commands.log",
            "diagnostics": "diagnostics",
        },
    }
    write_json(config.artifacts_dir / "manifest.json", manifest)
    write_json(config.run_dir / "manifest.json", manifest)
    return manifest


def _git_output(root: Path, arguments: list[str]) -> str:
    try:
        result = subprocess.run(
            ["git", *arguments],
            cwd=root,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"
    return result.stdout.strip() if result.returncode == 0 else "unknown"


def cleanup(config: Config, compose: Compose) -> None:
    if config.keep:
        log(f"keeping run directory and stack: {config.run_dir}")
        return
    if not run_sentinel(config).is_file():
        log(f"refusing destructive cleanup: no run ownership marker in {config.run_dir}")
        return
    compose.down(volumes=True)
    shutil.rmtree(config.git_dir, ignore_errors=True)
    shutil.rmtree(config.model_dir, ignore_errors=True)
    shutil.rmtree(config.run_dir / "tmp", ignore_errors=True)
    try:
        config.cookie_jar.unlink(missing_ok=True)
    except OSError:
        pass
