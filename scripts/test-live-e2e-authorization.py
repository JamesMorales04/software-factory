#!/usr/bin/env python3
"""Negative and positive tests for the paid E2E preflight; no provider calls."""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHECK = ROOT / "scripts" / "live-e2e-authorization.py"
ROUTE = "opencode/opencode-go/deepseek-v4.1-flash"
BASE = {
    "FACTORY_LIVE_MODE": "1",
    "FACTORY_LIVE_AUTHORIZATION": "APPROVED",
    "FACTORY_LIVE_AUTH_CAMPAIGN": "F1-S4-A",
    "FACTORY_LIVE_AUTH_PROJECT": "default",
    "FACTORY_LIVE_AUTH_SESSION_BUDGET": "1",
    "FACTORY_LIVE_AUTH_ROUTES": ROUTE,
}


def run(changes: dict[str, str | None]) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    for key in BASE:
        env.pop(key, None)
    env.update(BASE)
    for key, value in changes.items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    return subprocess.run(
        [sys.executable, str(CHECK), "--campaign", "F1-S4-A", "--project", "default",
         "--max-sessions", "1", "--route", ROUTE, "--requested-route", ROUTE],
        cwd=ROOT, env=env, capture_output=True, text=True, check=False,
    )


def require_rejected(name: str, changes: dict[str, str | None]) -> None:
    result = run(changes)
    if result.returncode == 0:
        raise AssertionError(f"{name}: preflight unexpectedly accepted: {result.stdout}")
    print(f"[PASS] {name}: rejected before dispatch")


def main() -> int:
    result = run({})
    if result.returncode != 0:
        raise AssertionError(f"valid bounded authorization rejected: {result.stderr}")
    print("[PASS] valid exact campaign/project/budget/route accepted by preflight")

    require_rejected("live mode missing", {"FACTORY_LIVE_MODE": None})
    require_rejected("operator authorization missing", {"FACTORY_LIVE_AUTHORIZATION": None})
    require_rejected("campaign missing", {"FACTORY_LIVE_AUTH_CAMPAIGN": None})
    require_rejected("cross-campaign authorization", {"FACTORY_LIVE_AUTH_CAMPAIGN": "F1-S4-B"})
    require_rejected("project missing", {"FACTORY_LIVE_AUTH_PROJECT": None})
    require_rejected("cross-project authorization", {"FACTORY_LIVE_AUTH_PROJECT": "other-project"})
    require_rejected("zero budget", {"FACTORY_LIVE_AUTH_SESSION_BUDGET": "0"})
    require_rejected("malformed budget", {"FACTORY_LIVE_AUTH_SESSION_BUDGET": "one"})
    require_rejected("budget exceeds script ceiling", {"FACTORY_LIVE_AUTH_SESSION_BUDGET": "2"})
    require_rejected("route authorization missing", {"FACTORY_LIVE_AUTH_ROUTES": None})
    require_rejected("unauthorized model route", {"FACTORY_LIVE_AUTH_ROUTES": "codex/gpt-6-luna"})
    require_rejected("route list has extra route", {"FACTORY_LIVE_AUTH_ROUTES": ROUTE + ",codex/gpt-6-luna"})
    require_rejected("route list malformed", {"FACTORY_LIVE_AUTH_ROUTES": ROUTE + ", codex/gpt-6-luna"})
    print("All live authorization preflight tests passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
