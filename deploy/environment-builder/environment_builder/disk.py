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

DEFAULT_SAMPLE_INTERVAL_SECONDS = 3.0
DU_TIMEOUT_SECONDS = 20.0
MAX_DU_OUTPUT_BYTES = 64 * 1024


def _parse_du(output: str) -> tuple[int, bool]:
    """Parse ``du -skc`` output and return ``(bytes, complete)``."""
    total_kib: int | None = None
    summed_kib = 0
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
        if parts[1].strip() == "total":
            total_kib = kib
        else:
            summed_kib += kib
    if lines == 0:
        return 0, False
    if total_kib is not None:
        return total_kib * 1024, True
    return summed_kib * 1024, True


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
    resolved = [str(Path(path)) for path in paths]
    if not resolved:
        return 0, False
    try:
        completed = subprocess.run(
            ["du", "-skc", "--", *resolved],
            check=False,
            capture_output=True,
            text=True,
            timeout=max(1.0, timeout),
        )
    except (OSError, subprocess.SubprocessError):
        return 0, False
    if completed.returncode != 0:
        return 0, False
    stdout = completed.stdout
    if len(stdout.encode("utf-8", errors="replace")) > MAX_DU_OUTPUT_BYTES:
        return 0, False
    return _parse_du(stdout)


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
        self.interval = max(0.5, interval)
        self.exceeded = False
        self.incomplete = False
        self.observed_bytes = 0
        self.complete = True
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def _run(self) -> None:
        while not self._stop.wait(self.interval):
            used, complete = self.budget.usage()
            self.observed_bytes = used
            self.complete = complete
            if complete and used <= self.budget.limit_bytes:
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
        self._thread.join(self.interval + DU_TIMEOUT_SECONDS + 2.0)


class DiskBudget:
    """A frozen per-operation disk bound over cache/store/workspace paths."""

    def __init__(
        self,
        limit_mib: int,
        paths: Iterable[Path | str],
        *,
        timeout: float = DU_TIMEOUT_SECONDS,
        interval: float = DEFAULT_SAMPLE_INTERVAL_SECONDS,
    ) -> None:
        self.limit_mib = max(0, int(limit_mib))
        self.limit_bytes = self.limit_mib * 1024 * 1024
        self.paths = tuple(Path(path) for path in paths)
        self.timeout = timeout
        self.interval = interval

    def usage(self) -> tuple[int, bool]:
        return directory_usage_bytes(self.paths, timeout=self.timeout)

    def check(self) -> None:
        used, complete = self.usage()
        if not complete:
            fail(
                "ENVIRONMENT_RESOURCE_LIMIT",
                "the builder could not complete the du disk scan; the frozen "
                "disk bound cannot be verified (exact=false)",
            )
        if used > self.limit_bytes:
            fail(
                "ENVIRONMENT_RESOURCE_LIMIT",
                f"observed builder disk usage {used} bytes exceeds the frozen "
                f"{self.limit_mib} MiB bound; exact=false",
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
