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
import re
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_TIMEOUT = 600
DEFAULT_OPERATION_TIMEOUT = 120
DEFAULT_READINESS_TIMEOUT = 300
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
    timeout: int | None = 60,
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
    operation_timeout: int
    readiness_timeout: int | None
    run_timeout: int | None
    scenario: str
    required_stages: list[str] | None = None

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
    def storage_state(self) -> Path:
        """Playwright storageState JSON exported from the driver cookie jar."""

        return self.run_dir / "cookies.storage.json"

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
        # The autonomous journey binds a real EnvironmentPlan/Spec, so the
        # test-support placeholder is assigned "0" there; the fixture-only
        # journeys assign "1". The value is always written explicitly so a host
        # variable can never leak into the stack.
        environment["FACTORY_TEST_ENVIRONMENT_BINDING"] = (
            "0" if self.scenario == "autonomous-intake" else "1"
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



_UNSET = object()


class SupervisedResult:
    """Result of a supervised process-group run with durable streaming logs."""

    def __init__(
        self,
        returncode: int,
        stdout: str,
        stderr: str,
        log_path: Path,
        *,
        timed_out: bool,
        stdout_path: Path | None = None,
        stderr_path: Path | None = None,
    ) -> None:
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr
        self.log_path = log_path
        self.timed_out = timed_out
        self.stdout_path = stdout_path
        self.stderr_path = stderr_path


def _terminate_process_group(process: subprocess.Popen[str]) -> None:
    """Terminate only this process group, reap children and report the exit."""

    if process.poll() is not None:
        return
    try:
        os.killpg(os.getpgid(process.pid), signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass
    try:
        os.killpg(os.getpgid(process.pid), signal.SIGKILL)
    except ProcessLookupError:
        return
    process.wait()


def run_supervised(
    command: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    log_path: Path,
    timeout: int | None = None,
    check: bool = True,
    label: str = "",
) -> SupervisedResult:
    """Run one command with streaming durable output and process-group cleanup.

    ``timeout`` is an **explicit operation deadline** only when the caller has an
    observable contract. Build/start and campaign runs pass ``None``: silence is
    not treated as no progress and no universal clock is invented. Heartbeats
    are observability, never a breaker.
    """

    ensure_dir(log_path.parent)
    rendered = " ".join(command)
    log(f"$ {rendered}")
    started = time.monotonic()
    stdout_tail = ""
    stderr_tail = ""
    timed_out = False
    stdout_path = log_path.with_suffix(log_path.suffix + ".stdout")
    stderr_path = log_path.with_suffix(log_path.suffix + ".stderr")
    stdout_raw = stdout_path.open("w", encoding="utf-8")
    stderr_raw = stderr_path.open("w", encoding="utf-8")
    with log_path.open("w", encoding="utf-8") as handle:
        handle.write(f"$ {rendered}\n")
        handle.flush()
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )

        def pump(stream, name: str) -> None:
            nonlocal stdout_tail, stderr_tail
            assert stream is not None
            for line in iter(stream.readline, ""):
                (stdout_raw if name == "stdout" else stderr_raw).write(line)
                (stdout_raw if name == "stdout" else stderr_raw).flush()
                handle.write(("" if name == "stdout" else "ERR ") + line)
                handle.flush()
                if name == "stdout":
                    stdout_tail = (stdout_tail + line)[-8000:]
                else:
                    stderr_tail = (stderr_tail + line)[-8000:]
            stream.close()

        threads = [
            threading.Thread(target=pump, args=(process.stdout, "stdout"), daemon=True),
            threading.Thread(target=pump, args=(process.stderr, "stderr"), daemon=True),
        ]
        for thread in threads:
            thread.start()
        deadline = None if timeout is None else time.monotonic() + timeout
        last_heartbeat = time.monotonic()
        try:
            while process.poll() is None:
                now = time.monotonic()
                if deadline is not None and now >= deadline:
                    timed_out = True
                    handle.write(
                        f"# explicit operation deadline {timeout}s reached; "
                        "activity=undetermined; terminating our process group\n"
                    )
                    handle.flush()
                    _terminate_process_group(process)
                    break
                if now - last_heartbeat >= 30:
                    handle.write(
                        f"# heartbeat elapsed={now - started:.0f}s pid={process.pid} "
                        "activity=undetermined (silent output is allowed)\n"
                    )
                    handle.flush()
                    last_heartbeat = now
                time.sleep(0.2)
        except BaseException:
            _terminate_process_group(process)
            raise
        returncode = process.wait()
        for thread in threads:
            thread.join(timeout=5)
        elapsed = time.monotonic() - started
        handle.write(f"# exit={returncode} elapsed={elapsed:.3f}s timed_out={timed_out}\n")
    if check and (timed_out or returncode != 0):
        raise ScenarioError(
            f"supervised command failed ({returncode}, timed_out={timed_out}): {rendered}\n"
            f"stdout:\n{tail(stdout_tail)}\nstderr:\n{tail(stderr_tail)}\nlog={log_path}"
        )
    stdout_raw.close()
    stderr_raw.close()
    return SupervisedResult(
        returncode,
        stdout_tail,
        stderr_tail,
        log_path,
        timed_out=timed_out,
        stdout_path=stdout_path,
        stderr_path=stderr_path,
    )


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
        timeout: int | None | object = _UNSET,
    ) -> SupervisedResult:
        args = list(arguments)
        command = self.command() + args
        log_path = (
            ensure_dir(self.config.artifacts_dir / "logs") / f"{log_name}.log"
            if log_name
            else self.log_path
        )
        result = run_supervised(
            command,
            cwd=self.config.root,
            env=self.config.environment(),
            log_path=log_path,
            timeout=(
                self.config.operation_timeout if timeout is _UNSET else timeout
            ),
            check=False,
        )
        with self.log_path.open("a", encoding="utf-8") as handle:
            handle.write(
                f"\n$ {' '.join(command)}\n{tail(result.stdout)}\n{tail(result.stderr)}\n"
                f"exit={result.returncode} log={log_path}\n"
            )
        if check and result.returncode != 0:
            raise ScenarioError(
                f"command failed ({result.returncode}): {' '.join(command)}\n"
                f"stdout:\n{tail(result.stdout)}\nstderr:\n{tail(result.stderr)}\nlog={log_path}"
            )
        return result

    def down(self, *, volumes: bool) -> subprocess.CompletedProcess[str]:
        arguments = ["down"] + (["-v"] if volumes else [])
        return self.run(arguments, check=False, log_name="compose-down")

    def _candidate_identity(self) -> tuple[str, str]:
        """Return the exact clean commit and tree that the build compiles.

        A dirty worktree is refused: HEAD alone does not identify the source
        that Cargo/Node compiled, so image ids could not be bound honestly.
        """

        dirty = subprocess.run(
            ["git", "-C", str(self.config.root), "status", "--porcelain"],
            text=True,
            capture_output=True,
            timeout=self.config.operation_timeout,
            check=False,
        )
        if dirty.returncode != 0:
            raise ScenarioError(f"cannot inspect the worktree: {tail(dirty.stderr)}")
        if dirty.stdout.strip():
            raise ScenarioError(
                "refusing to build bindings from a dirty worktree: commit or use a clean "
                "candidate checkout so the source SHA and tree identify the compiled images"
            )
        sha = subprocess.run(
            ["git", "-C", str(self.config.root), "rev-parse", "HEAD"],
            text=True,
            capture_output=True,
            timeout=self.config.operation_timeout,
            check=False,
        )
        tree = subprocess.run(
            ["git", "-C", str(self.config.root), "rev-parse", "HEAD^{tree}"],
            text=True,
            capture_output=True,
            timeout=self.config.operation_timeout,
            check=False,
        )
        if sha.returncode != 0 or tree.returncode != 0:
            raise ScenarioError("cannot resolve the candidate SHA/tree")
        return sha.stdout.strip(), tree.stdout.strip()

    def _candidate_sha(self) -> str:
        return self._candidate_identity()[0]

    def _compose_services(self) -> list[str]:
        result = self.run(["config", "--services"], log_name="compose-services")
        services = [line.strip() for line in result.stdout.splitlines() if line.strip()]
        if not services:
            raise ScenarioError("Compose reported no services")
        return services

    def _full_stdout(self, result: SupervisedResult) -> str:
        """Machine parsers read the complete durable stream, never a tail."""

        if result.stdout_path is not None and result.stdout_path.is_file():
            return result.stdout_path.read_text(encoding="utf-8")
        return result.stdout

    def _compose_build_plan(self) -> dict[str, dict[str, Any]]:
        """Return service -> {image, build} from the complete Compose config.

        Only services that declare ``build:`` are candidate-built images. Image
        only services (fixtures, upstream base images) are resolved by Compose
        itself and must not be required as if we had built them.
        """

        result = self.run(["config"], log_name="compose-config")
        text = self._full_stdout(result)
        plan: dict[str, dict[str, Any]] = {}
        in_services = False
        current: str | None = None
        for line in text.splitlines():
            if line == "services:":
                in_services = True
                continue
            if not in_services:
                continue
            if line and not line.startswith(" "):
                break
            service = re.match(r"^  ([A-Za-z0-9][A-Za-z0-9_.-]*):\s*$", line)
            if service:
                current = service.group(1)
                plan[current] = {"image": "", "build": False}
                continue
            if current is None:
                continue
            field = re.match(r"^    (build|image):\s*(.*?)\s*$", line)
            if not field:
                continue
            key, value = field.group(1), field.group(2).strip().strip('"')
            if key == "build":
                plan[current]["build"] = True
            else:
                plan[current]["image"] = value
        if not plan:
            raise ScenarioError("Compose reported no services")
        return plan

    def prebuild_sequential(self) -> dict[str, Any]:
        """Build every service one at a time, with no total build deadline.

        ``up --build`` fans out parallel builds and produced ten concurrent
        builds with orphan children after a cancelled parent. This path builds
        sequentially, streams each build to its own durable log, and records the
        exact candidate SHA plus the resulting image ids for later binding.
        """

        plan = self._compose_build_plan()
        built_services = [name for name, entry in plan.items() if entry["build"]]
        upstream = {
            name: entry["image"]
            for name, entry in plan.items()
            if not entry["build"] and entry["image"]
        }
        if not built_services:
            raise ScenarioError("Compose reported no candidate-built services")
        candidate, tree = self._candidate_identity()
        for index, service in enumerate(built_services, start=1):
            log(f"sequential prebuild {index}/{len(built_services)}: {service}")
            result = run_supervised(
                self.command() + ["build", service],
                cwd=self.config.root,
                env=self.config.environment(),
                log_path=ensure_dir(self.config.artifacts_dir / "logs")
                / f"build-{index:02d}-{service}.log",
                timeout=self.config.run_timeout,
                check=False,
            )
            if result.returncode != 0:
                raise ScenarioError(
                    f"sequential build failed for {service} ({result.returncode}); "
                    f"log={result.log_path}"
                )
        images = sorted(
            {
                plan[service]["image"]
                for service in built_services
                if plan[service]["image"]
            }
        )
        if not images:
            raise ScenarioError("candidate-built services declare no image names")
        listed = subprocess.run(
            ["podman", "images", "--format", "{{.Repository}}:{{.Tag}} {{.ID}}"],
            text=True,
            capture_output=True,
            timeout=self.config.operation_timeout,
            check=False,
        )
        if listed.returncode != 0:
            raise ScenarioError(f"cannot list images: {tail(listed.stderr)}")
        observed = {}
        for line in listed.stdout.splitlines():
            parts = line.split()
            if len(parts) == 2:
                repository, image_id = parts
                observed[repository] = image_id
                # Podman prints an unqualified local image as localhost/<name>.
                # Compose config prints it without the implicit localhost
                # namespace, so both spellings identify the same image.
                if repository.startswith("localhost/"):
                    observed[repository.split("/", 1)[1]] = image_id
        missing = [image for image in images if image not in observed]
        if missing:
            raise ScenarioError(
                "sequential prebuild did not produce every Compose image: " + ", ".join(missing)
            )
        binding = {
            "schema": "factory-e2e-prebuild-v1",
            "candidateSha": candidate,
            "candidateTree": tree,
            "createdAt": now_utc(),
            "services": built_services,
            "images": {image: observed[image] for image in images},
            # Image-only Compose services are resolved upstream by Compose at
            # start. We record identity when it already exists, but never claim
            # it as candidate-built provenance and never require it here.
            "upstreamImages": upstream,
        }
        write_json(self.config.run_dir / "prebuilt.json", binding)
        return binding

    def _verify_prebuilt_binding(self) -> None:
        path = self.config.run_dir / "prebuilt.json"
        if not path.is_file():
            raise ScenarioError(
                "refusing --no-build: no candidate-bound prebuild record exists; run the "
                "sequential prebuild first"
            )
        binding = json.loads(path.read_text(encoding="utf-8"))
        if binding.get("schema") != "factory-e2e-prebuild-v1":
            raise ScenarioError("refusing --no-build: prebuild record has an unknown schema")
        candidate, tree = self._candidate_identity()
        if binding.get("candidateSha") != candidate or binding.get("candidateTree") != tree:
            raise ScenarioError(
                "refusing --no-build: prebuild images are not bound to the current candidate "
                f"({binding.get('candidateSha')}/{binding.get('candidateTree')} != "
                f"{candidate}/{tree})"
            )
        listed = subprocess.run(
            ["podman", "images", "--format", "{{.Repository}}:{{.Tag}} {{.ID}}"],
            text=True,
            capture_output=True,
            timeout=self.config.operation_timeout,
            check=False,
        )
        observed = {}
        for line in listed.stdout.splitlines():
            parts = line.split()
            if len(parts) == 2:
                repository, image_id = parts
                observed[repository] = image_id
                if repository.startswith("localhost/"):
                    observed[repository.split("/", 1)[1]] = image_id
        for image, image_id in (binding.get("images") or {}).items():
            if observed.get(image) != image_id:
                raise ScenarioError(
                    f"refusing --no-build: image {image} is not the built digest {image_id}"
                )

    def up(self, *, build: bool, services: list[str] | None = None) -> None:
        if build:
            self.prebuild_sequential()
        else:
            self._verify_prebuilt_binding()
        command = ["up", "-d", "--no-build"]
        if services:
            command.extend(services)
        self.run(
            command,
            check=True,
            log_name="compose-up",
            timeout=self.config.run_timeout,
        )

    def start(self) -> None:
        self.run(
            ["start"], check=False, log_name="compose-start", timeout=self.config.run_timeout
        )

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

    def exec_service(self, service: str, arguments: Iterable[str], *, check: bool = True) -> str:
        container = self.service_container(service)
        if not container:
            raise ScenarioError(f"{service} container is not present")
        result = subprocess.run(
            ["podman", "exec", container, *arguments],
            cwd=self.config.root,
            env=self.config.environment(),
            text=True,
            capture_output=True,
            timeout=self.config.run_timeout,
            check=False,
        )
        if check and result.returncode != 0:
            raise ScenarioError(
                f"{service} exec failed: {tail(result.stderr) or tail(result.stdout)}"
            )
        return result.stdout

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
            timeout=self.config.run_timeout,
            check=False,
        )
        if check and result.returncode != 0:
            raise ScenarioError(
                f"podman exec {' '.join(arguments)} failed ({result.returncode}): "
                f"{tail(result.stderr)}"
            )
        return result.stdout

    def declared_volume_names(self) -> set[str] | None:
        """Return the exact Compose volume names, or None if the config is unreadable."""

        result = self.run(["config"], check=False, log_name="compose-config")
        if result.returncode != 0:
            return None
        names: set[str] = set()
        in_volumes = False
        for line in result.stdout.splitlines():
            if line == "volumes:":
                in_volumes = True
                continue
            if not in_volumes:
                continue
            if line and not line.startswith(" "):
                break
            match = re.match(r"^  ([A-Za-z0-9_.-]+):", line)
            if match:
                names.add(f"{self.config.project}_{match.group(1)}")
        return names or None

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
        self._write_storage_state()

    def storage_state(self) -> dict[str, Any]:
        """Playwright storageState for the exact session cookies in this jar.

        The browser lane must consume the same authenticated session the driver
        produced. Exporting from the already-parsed LWP cookie jar avoids a
        second, divergent cookie parser. Cookie values are never logged.
        """

        cookies = []
        for cookie in self.jar:
            rest = getattr(cookie, "_rest", {}) or {}
            same_site = rest.get("SameSite") or rest.get("samesite")
            if same_site not in {"Strict", "Lax", "None"}:
                same_site = None
            entry: dict[str, Any] = {
                "name": cookie.name,
                "value": cookie.value,
                "domain": cookie.domain or "",
                "path": cookie.path or "/",
                "expires": int(cookie.expires) if cookie.expires else -1,
                "httpOnly": bool(
                    cookie.has_nonstandard_attr("HttpOnly")
                    or rest.get("HttpOnly") in (True, "True", "true")
                ),
                "secure": bool(cookie.secure),
            }
            if same_site:
                entry["sameSite"] = same_site
            cookies.append(entry)
        return {"cookies": cookies, "origins": []}

    def _write_storage_state(self) -> None:
        path = self.config.storage_state
        ensure_dir(path.parent)
        payload = json.dumps(self.storage_state(), indent=2, sort_keys=True) + "\n"
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_text(payload, encoding="utf-8")
        os.chmod(temporary, 0o600)
        temporary.replace(path)

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


def wait_url(url: str, timeout: int | None, *, interval: float = 1.0) -> None:
    deadline = None if timeout is None else time.monotonic() + timeout
    last_error: Exception | None = None
    last_heartbeat = time.monotonic()
    while deadline is None or time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=5) as response:
                if response.status < 500:
                    return
        except (urllib.error.URLError, OSError, TimeoutError) as error:
            last_error = error
        now = time.monotonic()
        if now - last_heartbeat >= 30:
            log(f"waiting for {url}: activity=undetermined last_error={last_error!r}")
            last_heartbeat = now
        time.sleep(interval)
    raise ScenarioError(f"explicit readiness deadline {timeout}s reached for {url}: {last_error}")


def wait_service(compose: Compose, service: str, url: str, timeout: int | None) -> None:
    deadline = None if timeout is None else time.monotonic() + timeout
    last_error: Exception | None = None
    last_heartbeat = time.monotonic()
    while deadline is None or time.monotonic() < deadline:
        try:
            compose.exec_service(service, ["curl", "-fsS", url], check=True)
            return
        except (ScenarioError, OSError, subprocess.TimeoutExpired) as error:
            last_error = error
        now = time.monotonic()
        if now - last_heartbeat >= 30:
            log(f"waiting for {service} {url}: activity=undetermined last_error={last_error!r}")
            last_heartbeat = now
        if deadline is not None and time.monotonic() >= deadline:
            break
        time.sleep(1.0)
    raise ScenarioError(f"{service} did not become ready at {url}: {last_error!r}")


def wait_core(compose: Compose, url: str, timeout: int | None, *, interval: float = 1.0) -> None:
    deadline = None if timeout is None else time.monotonic() + timeout
    last_error: Exception | None = None
    last_heartbeat = time.monotonic()
    while deadline is None or time.monotonic() < deadline:
        try:
            compose.exec_core(["curl", "-fsS", url], check=True)
            return
        except (ScenarioError, OSError, subprocess.TimeoutExpired) as error:
            last_error = error
        now = time.monotonic()
        if now - last_heartbeat >= 30:
            log(f"waiting for Core {url}: activity=undetermined last_error={last_error!r}")
            last_heartbeat = now
        time.sleep(interval)
    raise ScenarioError(
        f"explicit readiness deadline {timeout}s reached waiting for Core {url}: {last_error}"
    )


def wait_until(predicate, timeout: int | None, description: str, *, interval: float = 1.0) -> Any:
    deadline = None if timeout is None else time.monotonic() + timeout
    last_error: Exception | None = None
    last_heartbeat = time.monotonic()
    while deadline is None or time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except Exception as error:  # noqa: BLE001 - the caller needs the last diagnostic
            last_error = error
        now = time.monotonic()
        if now - last_heartbeat >= 30:
            log(f"waiting for {description}: activity=undetermined last_error={last_error!r}")
            last_heartbeat = now
        time.sleep(interval)
    raise ScenarioError(
        f"explicit readiness deadline {timeout}s reached waiting for {description}: {last_error}"
    )


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
    expected = compose.declared_volume_names()
    if not expected or set(volumes) != expected:
        raise ScenarioError(
            "cannot snapshot: live volumes do not match the exact Compose declaration; "
            f"live={sorted(volumes)}, expected={sorted(expected or [])}"
        )
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
            timeout=config.run_timeout,
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


def _validate_snapshot_index(
    config: Config,
    index: dict[str, Any],
    expected_volumes: set[str] | None = None,
) -> list[dict[str, Any]]:
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
    if expected_volumes is not None:
        observed = {entry["name"] for entry in validated}
        if observed != expected_volumes:
            missing = sorted(expected_volumes - observed)
            extra = sorted(observed - expected_volumes)
            raise ScenarioError(
                "snapshot volume set does not match the Compose declaration: "
                f"missing={missing}, extra={extra}"
            )
    return validated


def restore_volumes(config: Config, compose: Compose) -> dict[str, Any]:
    """Validate the full snapshot before any destructive operation, then restore."""

    index = read_json(config.snapshot_dir / "index.json")
    expected = compose.declared_volume_names()
    if not expected:
        raise ScenarioError(
            "cannot restore: the exact Compose-declared volume set is unavailable; "
            "refusing to tear down the current stack"
        )
    entries = _validate_snapshot_index(config, index, expected)
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
            timeout=config.run_timeout,
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
    cleanup_result: dict[str, Any] | None = None,
) -> dict[str, Any]:
    head = _git_output(config.root, ["rev-parse", "HEAD"])
    frozen = candidate_sha or state.get("candidate_sha") or head
    base = _git_output(config.root, ["merge-base", "HEAD", "origin/main"]) or head
    live_dirty = bool(_git_output(config.root, ["status", "--porcelain"]))
    dirty = live_dirty or state.get("dirty") is True
    head_changed = frozen != head
    if (
        status == "PASS"
        and cleanup_result is not None
        and cleanup_result.get("status") not in {"PASS", "KEPT"}
    ):
        status = "FAIL"
        error = error or f"Compose cleanup did not complete: {cleanup_result.get('status')}"
    if status == "PASS" and (head_changed or dirty):
        status = "FAIL"
        if head_changed:
            error = error or f"manifest HEAD changed: bound {frozen}, observed {head}"
        else:
            error = error or "manifest observed a dirty worktree"
    manifest = {
        "schema": "factory-e2e-scenario-manifest-v1",
        "status": status,
        "scenario": config.scenario,
        "scenarioSha256": state.get("scenario_sha256"),
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
        "cassetteSha256": state.get("cassette_sha256"),
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
                "cassette_sha256",
                "scenario_sha256",
                "restart_persistence",
                "summary",
            }
        },
        "cleanup": cleanup_result,
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




_SECRET_PATTERN = re.compile(
    r"(?i)(authorization|cookie|password|passwd|secret|token|api[_-]?key)\s*[:=]\s*\S+"
)


def redact_text(value: str) -> str:
    return _SECRET_PATTERN.sub(r"\1=[REDACTED]", value)


def finalize_with_capture(capture, cleanup):
    """Run capture first, then cleanup; never let capture hide the run error.

    ``capture`` and ``cleanup`` are callables supplied by the caller (the real
    Podman capture or a fake adapter in tests). A capture failure is reported
    and the cleanup still runs. The original failure/cancellation is not
    replaced by a capture error.
    """

    capture_result = None
    capture_error = None
    try:
        capture_result = capture()
    except BaseException as error:  # noqa: BLE001 - preserve the run outcome
        capture_error = error
        log(f"final evidence capture failed: {error}")
    cleanup_result = cleanup()
    return capture_result, cleanup_result, capture_error


def capture_final_evidence(
    config: Config,
    compose: Compose,
    state: State,
    api: Api | None,
    *,
    reason: str,
) -> dict[str, Any]:
    """Capture durable pre-cleanup evidence without blocking cleanup.

    Every artifact is bounded and best effort. Missing permissions, an absent
    cgroup or an unavailable endpoint are recorded as NOT_CAPTURED with the
    reason. No prompts, cookies or credentials are written; free text is
    redacted. This is observability, not product telemetry.
    """

    directory = ensure_dir(config.artifacts_dir / "final-capture")
    summary: dict[str, Any] = {"reason": reason, "at": now_utc(), "artifacts": {}}

    def record(name: str, status: str, detail: str = "", body: str = "") -> None:
        summary["artifacts"][name] = {"status": status, "detail": detail}
        if body:
            path = directory / name
            path.write_text(redact_text(body), encoding="utf-8")
            os.chmod(path, 0o600)

    def podman(arguments: list[str], timeout: int = 30):
        return _bounded_run(
            ["podman", *arguments],
            cwd=config.root,
            env=config.environment(),
            timeout=timeout,
        )

    containers: list[str] = []
    try:
        listed = podman(
            ["ps", "-a", "--filter", f"label=com.docker.compose.project={config.project}",
             "--format", "{{.Names}}"]
        )
        containers = [line.strip() for line in listed.stdout.splitlines() if line.strip()]
        record("containers.txt", "CAPTURED", body="\n".join(containers) + "\n")
    except Exception as error:  # noqa: BLE001
        record("containers.txt", "NOT_CAPTURED", f"podman ps failed: {error}")

    for name in containers:
        safe = re.sub(r"[^A-Za-z0-9_.-]", "_", name)
        try:
            logs = podman(["logs", "--tail", "400", name])
            record(f"{safe}.log", "CAPTURED", body=f"{logs.stdout}\n{logs.stderr}")
        except Exception as error:  # noqa: BLE001
            record(f"{safe}.log", "NOT_CAPTURED", f"logs failed: {error}")
        try:
            inspected = podman(
                ["inspect", name, "--format",
                 "{{.State.Status}}|{{.State.ExitCode}}|{{.State.OOMKilled}}|"
                 "{{.HostConfig.Memory}}|{{.HostConfig.PidsLimit}}|{{.State.CgroupPath}}"]
            )
            record(f"{safe}.inspect", "CAPTURED", body=inspected.stdout + "\n")
        except Exception as error:  # noqa: BLE001
            record(f"{safe}.inspect", "NOT_CAPTURED", f"inspect failed: {error}")
        try:
            stats = podman(["stats", "--no-stream", "--format",
                            "{{.MemUsage}}|{{.MemPerc}}|{{.CPUPerc}}", name])
            record(f"{safe}.stats", "CAPTURED", body=stats.stdout + "\n")
        except Exception as error:  # noqa: BLE001
            record(f"{safe}.stats", "NOT_CAPTURED", f"stats failed: {error}")
        # cgroup memory evidence, best effort. The path comes from inspect.
        try:
            cgroup = podman(
                ["inspect", name, "--format", "{{.State.CgroupPath}}"]
            ).stdout.strip()
            if not cgroup:
                record(f"{safe}.memory", "NOT_CAPTURED", "cgroup path is empty")
            else:
                body = []
                for leaf in ("memory.events", "memory.peak", "memory.max", "memory.current"):
                    path = Path("/sys/fs/cgroup") / cgroup.lstrip("/") / leaf
                    if path.is_file():
                        body.append(f"{leaf}: {path.read_text(encoding='utf-8').strip()}")
                    else:
                        body.append(f"{leaf}: NOT_CAPTURED")
                record(f"{safe}.memory", "CAPTURED", body="\n".join(body) + "\n")
        except Exception as error:  # noqa: BLE001
            record(f"{safe}.memory", "NOT_CAPTURED", f"cgroup read failed: {error}")

    core = compose.service_container("core")
    if core:
        try:
            health = podman(
                ["exec", core, "curl", "-fsS", "--max-time", "5",
                 "http://127.0.0.1:8765/health"], timeout=20
            )
            record("health.txt", "CAPTURED", body=health.stdout + "\n")
        except Exception as error:  # noqa: BLE001
            record("health.txt", "NOT_CAPTURED", f"health failed: {error}")
    else:
        record("health.txt", "NOT_CAPTURED", "core container absent")

    if api is not None:
        try:
            calls = api.get("/model/calls")
            rows = calls if isinstance(calls, list) else (
                calls.get("calls") or calls.get("items") or []
            )
            body = "\n".join(
                f"{row.get('purpose')}|{row.get('state') or row.get('status')}|{row.get('createdAt') or row.get('created_at')}"
                for row in rows
            )
            record("model-calls.txt", "CAPTURED", body=body + "\n")
        except Exception as error:  # noqa: BLE001
            record("model-calls.txt", "NOT_CAPTURED", f"api failed: {error}")
        intake_id = state.get("intake_id")
        if intake_id:
            try:
                projection = api.get(f"/intake/requests/{intake_id}")
                record("projection.json", "CAPTURED",
                       body=json.dumps(projection, indent=2, sort_keys=True))
            except Exception as error:  # noqa: BLE001
                record("projection.json", "NOT_CAPTURED", f"projection failed: {error}")
        else:
            record("projection.json", "NOT_CAPTURED", "no intake id in state")

    fixture = compose.service_container("model-fixture")
    if fixture:
        script = (
            "import urllib.request;"
            "print(urllib.request.urlopen('http://127.0.0.1:8080/counters', timeout=5)"
            ".read().decode())"
        )
        try:
            result = podman(["exec", fixture, "python3", "-c", script], timeout=20)
            record("fixture-counters.json", "CAPTURED", body=result.stdout + "\n")
        except Exception as error:  # noqa: BLE001
            record("fixture-counters.json", "NOT_CAPTURED", f"counters failed: {error}")
    else:
        record("fixture-counters.json", "NOT_CAPTURED", "model fixture absent")

    directory.joinpath("capture.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True), encoding="utf-8"
    )
    os.chmod(directory / "capture.json", 0o600)
    log(f"final evidence captured: {directory}")
    return summary


def cleanup(config: Config, compose: Compose) -> dict[str, Any]:
    if config.keep:
        log(f"keeping run directory and stack: {config.run_dir}")
        return {"status": "KEPT", "detail": "containers and volumes retained by --keep"}
    if not run_sentinel(config).is_file():
        log(f"refusing destructive cleanup: no run ownership marker in {config.run_dir}")
        return {"status": "SKIPPED", "detail": "missing run ownership marker"}
    try:
        result = compose.down(volumes=True)
        status = "PASS" if result.returncode == 0 else "FAIL"
        exit_code = result.returncode
        stderr = tail(result.stderr) if result.returncode != 0 else ""
    except (ScenarioError, OSError) as error:
        status = "FAIL"
        exit_code = 1
        stderr = str(error)
    shutil.rmtree(config.git_dir, ignore_errors=True)
    shutil.rmtree(config.model_dir, ignore_errors=True)
    shutil.rmtree(config.run_dir / "tmp", ignore_errors=True)
    try:
        config.cookie_jar.unlink(missing_ok=True)
    except OSError:
        pass
    return {"status": status, "exitCode": exit_code, "stderr": stderr}
