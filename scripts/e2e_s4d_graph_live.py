#!/usr/bin/env python3
"""Bounded F1-S4-D multi-WorkUnit live certification preflight.

Default: local negative authorization preflight only; no provider call.
Live execution is NOT enabled in this candidate. It requires a separate
numeric Product Owner authorization and a completed runner pinned to an exact
clean Factory SHA. This file deliberately fails closed after authorization
validation until that runner is implemented and reviewed.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CAMPAIGN = "F1-S4-D-GRAPH-LIVE"
PROJECT = "factory-fixture-s4d-graph"
PROVIDER, MODEL, MODE = "opencode", "opencode-go/deepseek-v4.1-flash", "build"
ROUTE = f"{PROVIDER}/{MODEL}"
AUTHORIZATION_ID = "s4d-graph-owner-auth-1"
SESSIONS = 4          # 1 assessment + 2 unit implementers + 1 integration
ATTEMPTS = 3
REPAIRS = 0
PARALLEL = 2
MAX_WALL_CLOCK_SECS = 1800


def guard(env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(ROOT / "scripts/live-e2e-authorization.py"),
         "--campaign", CAMPAIGN, "--project", PROJECT,
         "--max-sessions", str(SESSIONS), "--route", ROUTE,
         "--requested-route", ROUTE],
        cwd=ROOT, env=env, text=True, capture_output=True, check=False,
    )


def safe_preflight() -> int:
    env = os.environ.copy()
    for key in (
        "FACTORY_LIVE_MODE", "FACTORY_LIVE_AUTHORIZATION", "FACTORY_LIVE_AUTH_CAMPAIGN",
        "FACTORY_LIVE_AUTH_PROJECT", "FACTORY_LIVE_AUTH_SESSION_BUDGET",
        "FACTORY_LIVE_AUTH_ROUTES", "FACTORY_LIVE_AUTH_FACTORY_SHA",
        "FACTORY_LIVE_AUTHORIZATION_ID",
    ):
        env.pop(key, None)
    result = guard(env)
    rejected = result.returncode == 2 and "[BLOCKED]" in result.stderr
    print(json.dumps({
        "status": "PREFLIGHT_SAFE" if rejected else "PREFLIGHT_FAILED",
        "providerCalls": 0,
        "guardRejectedMissingOwnerAuthorization": rejected,
        "liveRunnerOptIn": "FACTORY_S4D_GRAPH_LIVE=1",
        "liveExecutionImplemented": False,
        "route": ROUTE,
        "workUnits": 2,
        "integrationUnits": 1,
        "sessionBudget": {"total": SESSIONS, "assessment": 1, "implementers": 2, "integration": 1},
        "attemptBudget": ATTEMPTS,
        "maxRepairCycles": REPAIRS,
        "maxParallel": PARALLEL,
        "maxWallClockSecs": MAX_WALL_CLOCK_SECS,
    }, indent=2))
    return 0 if rejected else 1


def main() -> int:
    if os.environ.get("FACTORY_S4D_GRAPH_LIVE") != "1":
        return safe_preflight()
    result = guard(os.environ.copy())
    if result.returncode != 0:
        print("[BLOCKED] live authorization: " + result.stderr.strip(), file=sys.stderr)
        return 2
    if os.environ.get("FACTORY_LIVE_AUTH_SESSION_BUDGET") != str(SESSIONS):
        print("[BLOCKED] live authorization: session budget must equal " + str(SESSIONS), file=sys.stderr)
        return 2
    if os.environ.get("FACTORY_LIVE_AUTHORIZATION_ID") != AUTHORIZATION_ID:
        print("[BLOCKED] live authorization: authorization ID mismatch", file=sys.stderr)
        return 2
    frozen = os.environ.get("FACTORY_LIVE_AUTH_FACTORY_SHA", "")
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True,
                          capture_output=True, check=False).stdout.strip()
    if len(frozen) != 40 or frozen != head:
        print("[BLOCKED] live authorization: exact frozen Factory SHA required", file=sys.stderr)
        return 2
    if subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, text=True,
                      capture_output=True, check=False).stdout.strip():
        print("[BLOCKED] live authorization: Factory worktree must be clean", file=sys.stderr)
        return 2
    print(json.dumps({
        "status": "BLOCKED_OR_INCOMPLETE",
        "reason": "multi-unit live runner is prepared as preflight only; final numeric authorization and reviewed execution are required",
        "providerCalls": 0,
    }), file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
