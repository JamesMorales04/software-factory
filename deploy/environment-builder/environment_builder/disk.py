"""Runtime disk-budget enforcement for the environment builder.

The builder runs inside a container whose ``/cache`` and ``/nix`` mounts may
grow without an exact filesystem quota. Coreutils ``du -skc`` provides a
complete, bounded-output disk accounting over the exact mounts. Any ``du``
error, timeout or malformed output is treated as an incomplete scan and fails
closed. ``disk.exact`` is always false; this is not an exact quota.
"""

from __future__ import annotations

import os
import signal
import subprocess
import threading
import time
from pathlib import Path
from typing import Iterable

from .errors import fail

DEFAULT_SAMPLE_INTERVAL_SECONDS = 10.0
DU_TIMEOUT_SECONDS = 120.0
DISK_MONITOR_FAILURE_THRESHOLD = 3
DISK_CHECK_MAX_ATTEMPTS = 3
DEFAULT_CHECK_RETRY_BACKOFF_SECONDS = 2.0
MAX_DU_OUTPUT_BYTES = 64 * 1024
MAX_DU_STDERR_BYTES = 16 * 1024
DU_CHUNK_BYTES = 16 * 1024


def _read_capped(stream: object, limit: int) -> tuple[bytes, bool]:
    captured = bytearray()
    truncated = False
    while True:
        try:
            chunk = stream.read(DU_CHUNK_BYTES)
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


def _parse_du(
    output: str,
    expected_paths: list[str],
) -> tuple[int, bool]:
    """Parse ``du -skc`` output and require a total and every expected path."""
    total_kib: int | None = None
    seen: set[str] = set()
    lines = 0
    for line in output.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split("\t", 1)
        if len(parts) != 2:
            return 0, False
        try:
            kib = int(parts[0])
        except ValueError:
            return 0, False
        if kib < 0:
            return 0, False
        lines += 1
        name = parts[1].strip()
        if name == "total":
            total_kib = kib
        else:
            seen.add(name)
    if lines == 0 or total_kib is None:
        return 0, False
    if seen != set(expected_paths):
        return 0, False
    return total_kib * 1024, True


def _run_du(paths: list[str], timeout: float) -> tuple[int, str, bool]:
    """Run ``du`` with bounded stdout/stderr drains. Returns (rc, stdout, ok)."""
    import threading

    try:
        process = subprocess.Popen(
            ["du", "-skc", "--", *paths],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,
            start_new_session=True,
        )
    except (OSError, ValueError):
        return 1, "", False
    assert process.stdout is not None
    assert process.stderr is not None
    results: dict[str, tuple[bytes, bool]] = {}

    def reader(name: str, stream: object, limit: int) -> None:
        results[name] = _read_capped(stream, limit)

    stdout_thread = threading.Thread(
        target=reader, args=("stdout", process.stdout, MAX_DU_OUTPUT_BYTES), daemon=True
    )
    stderr_thread = threading.Thread(
        target=reader, args=("stderr", process.stderr, MAX_DU_STDERR_BYTES), daemon=True
    )
    stdout_thread.start()
    stderr_thread.start()
    timed_out = False
    try:
        process.wait(timeout=max(1.0, timeout))
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except OSError:
            pass
        try:
            process.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired):
            pass
    stdout_thread.join(10)
    stderr_thread.join(10)
    for stream in (process.stdout, process.stderr):
        try:
            stream.close()
        except (OSError, ValueError):
            pass
    stdout_bytes, stdout_truncated = results.get("stdout", (b"", False))
    _stderr_bytes, _stderr_truncated = results.get("stderr", (b"", False))
    if timed_out or stdout_truncated:
        return 1, "", False
    return (
        process.returncode if process.returncode is not None else 1,
        stdout_bytes.decode("utf-8", errors="replace"),
        True,
    )


def _normalize_paths(paths: Iterable[Path | str]) -> list[str]:
    resolved: list[Path] = []
    for path in paths:
        candidate = Path(path).resolve(strict=False)
        if candidate not in resolved:
            resolved.append(candidate)
    keep = [
        path
        for path in resolved
        if not any(other != path and other in path.parents for other in resolved)
    ]
    return [str(path) for path in keep]


def directory_usage_bytes(
    paths: Iterable[Path | str],
    *,
    timeout: float = DU_TIMEOUT_SECONDS,
) -> tuple[int, bool]:
    """Return ``(observed_bytes, complete)`` using complete ``du -skc``.

    ``complete`` is false for a missing path, non-zero exit, timeout, excessive
    output or malformed output. The caller must treat an incomplete result as
    fail-closed rather than as a zero-byte success.
    """
    resolved = _normalize_paths(paths)
    if not resolved:
        return 0, False
    returncode, stdout, drained = _run_du(resolved, timeout)
    if returncode != 0 or not drained:
        return 0, False
    return _parse_du(stdout, resolved)


class DiskMonitor:
    """Sample disk usage in a background thread and stop an over-budget child."""

    def __init__(
        self,
        budget: "DiskBudget",
        process: object,
        *,
        interval: float = DEFAULT_SAMPLE_INTERVAL_SECONDS,
    ) -> None:
        self.budget = budget
        self.process = process
        self.interval = max(0.05, interval)
        self.exceeded = False
        self.incomplete = False
        self.observed_bytes = 0
        self.complete = True
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def _run(self) -> None:
        failures = 0
        while not self._stop.wait(self.interval):
            used, complete = self.budget.usage()
            if complete and used <= self.budget.limit_bytes:
                failures = 0
                self.observed_bytes = used
                self.complete = True
                continue
            failures += 1
            self.observed_bytes = used
            self.complete = complete
            if failures < DISK_MONITOR_FAILURE_THRESHOLD:
                continue
            self.exceeded = True
            self.incomplete = not complete
            try:
                if getattr(self.process, "poll")() is None:
                    os.killpg(getattr(self.process, "pid"), signal.SIGKILL)
            except (OSError, AttributeError):
                pass
            return

    def stop(self) -> None:
        self._stop.set()
        self._thread.join(self.interval + self.budget.timeout + 2.0)


class DiskBudget:
    """A frozen per-operation disk bound over cache/store/workspace paths."""

    def __init__(
        self,
        limit_mib: int,
        paths: Iterable[Path | str],
        *,
        timeout: float = DU_TIMEOUT_SECONDS,
        interval: float = DEFAULT_SAMPLE_INTERVAL_SECONDS,
        retry_backoff: float = DEFAULT_CHECK_RETRY_BACKOFF_SECONDS,
    ) -> None:
        self.limit_mib = max(0, int(limit_mib))
        self.limit_bytes = self.limit_mib * 1024 * 1024
        self.paths = tuple(Path(path) for path in paths)
        self.timeout = timeout
        self.interval = interval
        self.retry_backoff = max(0.0, retry_backoff)

    def usage(self) -> tuple[int, bool]:
        return directory_usage_bytes(self.paths, timeout=self.timeout)

    def check(self) -> None:
        last_used = 0
        for attempt in range(DISK_CHECK_MAX_ATTEMPTS):
            used, complete = self.usage()
            last_used = used
            if complete:
                if used > self.limit_bytes:
                    fail(
                        "ENVIRONMENT_RESOURCE_LIMIT",
                        f"observed builder disk usage {used} bytes exceeds the frozen "
                        f"{self.limit_mib} MiB bound; exact=false",
                    )
                return
            if attempt + 1 < DISK_CHECK_MAX_ATTEMPTS and self.retry_backoff > 0:
                time.sleep(self.retry_backoff)
        fail(
            "ENVIRONMENT_RESOURCE_LIMIT",
            "the builder could not complete the du disk scan after "
            f"{DISK_CHECK_MAX_ATTEMPTS} attempts (last observed {last_used} bytes); "
            "the frozen disk bound cannot be verified (exact=false)",
        )

    def monitor(self, process: object) -> DiskMonitor:
        return DiskMonitor(self, process, interval=self.interval)

    def failure_record(self, phase: str, observed_bytes: int, complete: bool) -> dict:
        return {
            "schemaVersion": 1,
            "phase": phase,
            "planDiskMiB": self.limit_mib,
            "observedBytes": observed_bytes,
            "observedMiB": round(observed_bytes / (1024 * 1024), 3),
            "enforcement": "DU_SKC_RUNTIME_SAMPLER",
            "exact": False,
            "sampleComplete": complete,
            "exceeded": (not complete) or observed_bytes > self.limit_bytes,
        }


def builder_disk_budget(
    resources: dict[str, int],
    run_dir: Path,
    workspace: Path,
) -> DiskBudget:
    nix_store = os.environ.get("FACTORY_ENVIRONMENT_NIX_STORE", "/nix")
    paths = [run_dir, workspace]
    if nix_store:
        paths.append(Path(nix_store))
    return DiskBudget(int(resources.get("diskMiB", 0)), paths)
