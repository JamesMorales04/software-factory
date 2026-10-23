#!/usr/bin/env python3
"""Bounded CAP-014 semantic evaluation collection.

Default: local negative authorization preflight only; no provider call.
Live: requires a separate 11-session authorization and runs exactly one
assessment per frozen fixture via core/cmd/s4d-semantic-collect. It never
implements code or invokes an Implementer.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CAMPAIGN = "F1-S4-D-SEMANTIC"
PROJECT = "factory-fixture-s4d-semantic"
PROVIDER, MODEL, MODE = "opencode", "opencode-go/deepseek-v4.1-flash", "build"
ROUTE = f"{PROVIDER}/{MODEL}/{MODE}"
AUTHORIZATION_ID = "s4d-semantic-owner-auth-1"
SESSIONS = 11


def cmd(args, *, cwd=ROOT, env=None, timeout=120, check=True):
    p = subprocess.run(args, cwd=cwd, env=env, text=True, capture_output=True, timeout=timeout)
    if check and p.returncode:
        raise RuntimeError(f"{args[0]} exited {p.returncode}: {p.stderr[-1200:]} {p.stdout[-1200:]}")
    return p


def guard(env):
    return cmd([sys.executable, str(ROOT / "scripts/live-e2e-authorization.py"),
                "--campaign", CAMPAIGN, "--project", PROJECT,
                "--max-sessions", str(SESSIONS), "--route", ROUTE,
                "--requested-route", ROUTE], env=env, check=False)


def safe_preflight() -> int:
    env = os.environ.copy()
    for key in ("FACTORY_LIVE_MODE", "FACTORY_LIVE_AUTHORIZATION", "FACTORY_LIVE_AUTH_CAMPAIGN",
                "FACTORY_LIVE_AUTH_PROJECT", "FACTORY_LIVE_AUTH_SESSION_BUDGET",
                "FACTORY_LIVE_AUTH_ROUTES", "FACTORY_LIVE_AUTH_FACTORY_SHA",
                "FACTORY_LIVE_AUTHORIZATION_ID", "FACTORY_S4D_SEMANTIC_LIVE"):
        env.pop(key, None)
    result = guard(env)
    rejected = result.returncode == 2 and "[BLOCKED]" in result.stderr
    print(json.dumps({
        "status": "PREFLIGHT_SAFE" if rejected else "PREFLIGHT_FAILED",
        "providerCalls": 0, "guardRejectedMissingOwnerAuthorization": rejected,
        "liveRunnerOptIn": "FACTORY_S4D_SEMANTIC_LIVE=1",
        "liveExecutionImplemented": True, "route": ROUTE,
        "fixtures": 11, "sessionCeiling": SESSIONS,
        "implementerSessions": 0,
    }, indent=2))
    return 0 if rejected else 1


def require_authority() -> str:
    if os.environ.get("FACTORY_S4D_SEMANTIC_LIVE") != "1":
        raise RuntimeError("FACTORY_S4D_SEMANTIC_LIVE=1 is required")
    p = guard(os.environ.copy())
    if p.returncode:
        raise RuntimeError("live-e2e-authorization.py rejected owner envelope: " + p.stderr.strip())
    if os.environ.get("FACTORY_LIVE_AUTH_SESSION_BUDGET") != str(SESSIONS):
        raise RuntimeError(f"owner must authorize exactly {SESSIONS} assessment sessions")
    if os.environ.get("FACTORY_LIVE_AUTHORIZATION_ID") != AUTHORIZATION_ID:
        raise RuntimeError(f"owner envelope must bind authorization ID {AUTHORIZATION_ID}")
    frozen = os.environ.get("FACTORY_LIVE_AUTH_FACTORY_SHA", "")
    head = cmd(["git", "rev-parse", "HEAD"]).stdout.strip()
    if len(frozen) != 40 or frozen != head:
        raise RuntimeError(f"FACTORY_LIVE_AUTH_FACTORY_SHA must exactly match frozen HEAD {head}")
    if cmd(["git", "status", "--porcelain"]).stdout.strip():
        raise RuntimeError("Factory worktree must be clean")
    return head


def main() -> int:
    if os.environ.get("FACTORY_S4D_SEMANTIC_LIVE") != "1":
        return safe_preflight()
    sha = require_authority()
    paseo = shutil.which("paseo")
    if not paseo:
        raise RuntimeError("paseo CLI is required")
    evidence_dir = Path(os.environ.get(
        "FACTORY_S4D_SEMANTIC_EVIDENCE_DIR",
        ROOT / "docs/evidence" / f"f1-s4d-semantic-live-{sha[:12]}",
    ))
    if evidence_dir.exists():
        raise RuntimeError(f"evidence directory already exists: {evidence_dir}")
    evidence_dir.mkdir(parents=True)
    binary = Path(tempfile.mkdtemp(prefix="s4d-semantic-bin-")) / "s4d-semantic-collect"
    try:
        cmd(["go", "build", "-o", str(binary), "./cmd/s4d-semantic-collect"],
            cwd=ROOT / "core", timeout=300)
        env = os.environ.copy()
        env.update({
            "FACTORY_S4D_SEMANTIC_LIVE": "1",
            "FACTORY_S4D_SEMANTIC_AUTHORIZATION_ID": AUTHORIZATION_ID,
            "FACTORY_S4D_SEMANTIC_ROUTE": f"{PROVIDER}/{MODEL}/{MODE}",
            "FACTORY_LIVE_AUTH_SESSION_BUDGET": str(SESSIONS),
            "FACTORY_LIVE_AUTH_FACTORY_SHA": sha,
            "PASEO_BIN": paseo,
        })
        result = subprocess.run([str(binary), "-out", str(evidence_dir)],
                                cwd=ROOT, env=env, text=True, capture_output=True,
                                timeout=3600, check=False)
        (evidence_dir / "collector.stdout").write_text(result.stdout)
        (evidence_dir / "collector.stderr").write_text(result.stderr)
        manifest = json.loads((evidence_dir / "manifest.json").read_text())
        manifest["factorySha"] = sha
        manifest["authorizationId"] = AUTHORIZATION_ID
        manifest["route"] = ROUTE
        manifest["collectorExit"] = result.returncode
        manifest["status"] = "COLLECTED" if result.returncode == 0 and manifest.get("sessionsUsed") == SESSIONS else "INCOMPLETE"
        (evidence_dir / "semantic-live-evidence.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"evidence: {evidence_dir / 'semantic-live-evidence.json'}")
        return 0 if manifest["status"] == "COLLECTED" else 2
    finally:
        shutil.rmtree(binary.parent, ignore_errors=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, OSError, subprocess.SubprocessError) as exc:
        print("[BLOCKED] live authorization: " + str(exc), file=sys.stderr)
        raise SystemExit(2)
