"""Runtime disk-budget enforcement for the environment builder.

The builder runs inside a container whose ``/cache`` and ``/nix`` mounts may
grow without an exact filesystem quota. This module provides a bounded,
best-effort sampler: it stops a command when observed usage exceeds the frozen
plan and reports ``ENVIRONMENT_RESOURCE_LIMIT``. It never claims an exact
quota; ``disk.exact`` is always false.
"""

from __future__ import annotations

import os
import signal
import threading
import time
from pathlib import Path
from typing import Iterable

from .errors import fail

#: A scan stops after this many directory entries or this much wall-clock time.
#: The result is a lower bound when the scan is incomplete; exceeding the limit
#: is still decisive. An incomplete scan is fail-closed because the builder
#: cannot prove the bound.
MAX_SCAN_ENTRIES = 1_000_000
MAX_SCAN_SECONDS = 5.0
DEFAULT_SAMPLE_INTERVAL_SECONDS = 0.5
FILESYSTEM_BLOCK_BYTES = 512


def _allocated_bytes(stat: os.stat_result) -> int:
    blocks = getattr(stat, "st_blocks", 0)
    if blocks:
        return int(blocks) * FILESYSTEM_BLOCK_BYTES
    return int(stat.st_size)


def directory_usage_bytes(
    paths: Iterable[Path | str],
    *,
    max_entries: int = MAX_SCAN_ENTRIES,
    max_seconds: float = MAX_SCAN_SECONDS,
) -> tuple[int, bool]:
    """Return ``(observed_bytes, complete)`` for the supplied paths.

    ``complete`` is false when the bounded scan hit its entry or time limit.
    The byte count is still a valid lower bound and must never be presented as
    an exact disk attestation.
    """
    started = time.monotonic()
    used = 0
    entries = 0
    complete = True
    stack = [Path(path) for path in paths]
    while stack:
        if entries >= max_entries or time.monotonic() - started > max_seconds:
            return used, False
        path = stack.pop()
        try:
            if path.is_symlink():
                used += _allocated_bytes(path.lstat())
                entries += 1
                continue
            if path.is_file():
                used += _allocated_bytes(path.stat())
                entries += 1
                continue
            if not path.is_dir():
                continue
            with os.scandir(path) as entries_iter:
                for entry in entries_iter:
                    entries += 1
                    if entries > max_entries or time.monotonic() - started > max_seconds:
                        return used, False
                    try:
                        if entry.is_dir(follow_symlinks=False):
                            stack.append(Path(entry.path))
                        else:
                            used += _allocated_bytes(entry.stat(follow_symlinks=False))
                    except OSError:
                        complete = False
        except OSError:
            complete = False
    return used, complete


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
        self._thread.join(self.interval + self.budget.max_seconds + 2.0)


class DiskBudget:
    """A frozen per-operation disk bound over cache/store/workspace paths."""

    def __init__(
        self,
        limit_mib: int,
        paths: Iterable[Path | str],
        *,
        max_entries: int = MAX_SCAN_ENTRIES,
        max_seconds: float = MAX_SCAN_SECONDS,
        interval: float = DEFAULT_SAMPLE_INTERVAL_SECONDS,
    ) -> None:
        self.limit_mib = max(0, int(limit_mib))
        self.limit_bytes = self.limit_mib * 1024 * 1024
        self.paths = tuple(Path(path) for path in paths)
        self.max_entries = max_entries
        self.max_seconds = max_seconds
        self.interval = interval

    def usage(self) -> tuple[int, bool]:
        return directory_usage_bytes(
            self.paths,
            max_entries=self.max_entries,
            max_seconds=self.max_seconds,
        )

    def check(self) -> None:
        used, complete = self.usage()
        if not complete:
            fail(
                "ENVIRONMENT_RESOURCE_LIMIT",
                f"cannot complete the builder disk scan after {used} bytes; the "
                "frozen disk bound cannot be verified (exact=false)",
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
            "enforcement": "BOUNDED_RUNTIME_SAMPLER",
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
