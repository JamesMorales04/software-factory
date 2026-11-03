#!/usr/bin/env python3
"""Measure a command and all of its descendant processes.

This is deliberately a sampling harness, not a benchmark framework. It records
what the kernel exposes for the exact process tree that ran:

* wall time and exit status,
* peak summed RSS across live descendants,
* peak process and thread counts,
* sampled CPU time across every PID observed (including exited PIDs),
* direct-child rusage as a cross-check.

Run as:
  python3 measure.py --label prototype --out result.json -- <command> [args...]
"""

from __future__ import annotations

import argparse
import json
import os
import resource
import subprocess
import sys
import time
from pathlib import Path


CLOCK_TICKS = os.sysconf("SC_CLK_TCK")


def proc_stat(pid: int):
    try:
        raw = Path(f"/proc/{pid}/stat").read_text()
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return None
    # comm may contain spaces and parentheses; split after the final ')'.
    head, tail = raw.rsplit(")", 1)
    fields = tail.split()
    if len(fields) < 22:
        return None
    return {
        "pid": pid,
        "ppid": int(fields[1]),
        "utime": int(fields[11]),
        "stime": int(fields[12]),
    }


def proc_rss(pid: int) -> int:
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return 0
    return 0


def proc_threads(pid: int) -> int:
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("Threads:"):
                return int(line.split()[1])
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return 0
    return 0


def snapshot(root_pid: int):
    stats = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        stat = proc_stat(int(entry.name))
        if stat:
            stats[stat["pid"]] = stat
    children: dict[int, list[int]] = {}
    for pid, stat in stats.items():
        children.setdefault(stat["ppid"], []).append(pid)
    live = []
    stack = [root_pid]
    while stack:
        pid = stack.pop()
        if pid not in stats:
            continue
        live.append(pid)
        stack.extend(children.get(pid, []))
    rss = sum(proc_rss(pid) for pid in live)
    threads = sum(proc_threads(pid) for pid in live)
    return {
        "pids": live,
        "rss_kib": rss,
        "threads": threads,
        "cpu_ticks": {pid: stats[pid]["utime"] + stats[pid]["stime"] for pid in live},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--interval-ms", type=int, default=5)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        parser.error("a command is required after --")

    started = time.monotonic()
    process = subprocess.Popen(command)
    peak_rss = 0
    peak_processes = 1
    peak_threads = 1
    cpu_ticks: dict[int, int] = {}
    samples = 0
    while process.poll() is None:
        snap = snapshot(process.pid)
        samples += 1
        peak_rss = max(peak_rss, snap["rss_kib"])
        peak_processes = max(peak_processes, len(snap["pids"]))
        peak_threads = max(peak_threads, snap["threads"])
        for pid, ticks in snap["cpu_ticks"].items():
            cpu_ticks[pid] = max(cpu_ticks.get(pid, 0), ticks)
        time.sleep(args.interval_ms / 1000.0)
    exit_code = process.wait()
    wall_ms = (time.monotonic() - started) * 1000.0
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    result = {
        "label": args.label,
        "command": command,
        "exit_code": exit_code,
        "wall_ms": round(wall_ms, 3),
        "peak_rss_kib": peak_rss,
        "peak_processes": peak_processes,
        "peak_threads": peak_threads,
        "sampled_cpu_ms": round(sum(cpu_ticks.values()) / CLOCK_TICKS * 1000.0, 3),
        "direct_child_rusage_cpu_ms": round(
            (usage.ru_utime + usage.ru_stime) * 1000.0, 3
        ),
        "direct_child_maxrss_kib": usage.ru_maxrss,
        "samples": samples,
        "profile": os.environ.get("PROFILE", "debug"),
    }
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    return 0 if exit_code == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
