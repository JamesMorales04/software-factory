#!/usr/bin/env python3
"""F1-S4-C-07 bounded real-Paseo Vlog certification.

Product Owner opt-in only. Hard bounds: one Project, one initial Implementer
session, at most one corrective cycle and therefore at most two paid
Implementer sessions. No Calculator, no fallback, no automatic retry beyond
Core's bounded repair plan, no merge.
"""
from __future__ import annotations

import json
import os
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROVIDER = "opencode"
MODEL = "opencode-go/deepseek-v4.1-flash"
MODE = "build"
PROJECT_ID = "factory-fixture-vlog"
MAX_ATTEMPTS = 2
MAX_REPAIRS = 1
MAX_PARALLEL = 1
ENDPOINT = "http://127.0.0.1:4173"
ALLOWED_PATHS = ["src/filter.js", "src/app.js"]
TERMINAL = {"accepted", "repair_required", "blocked", "human_required", "failed", "cancelled"}


def fail(message: str, **extra) -> None:
    payload = {"status": "BLOCKED", "reason": message, **extra}
    print(json.dumps(payload, indent=2))
    raise SystemExit(2)


def run(cmd, *, cwd=None, env=None, timeout=None, check=True):
    proc = subprocess.run(
        cmd, cwd=cwd, env=env, text=True, capture_output=True, timeout=timeout,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(map(str, cmd))}\n"
            f"{proc.stdout[-2000:]}\n{proc.stderr[-2000:]}"
        )
    return proc


def git(*args: str, cwd: Path | None = None) -> str:
    return run(["git", "-C", str(cwd or ROOT), *args]).stdout.strip()


class Client:
    def __init__(self, base: str, token: str):
        self.base = base
        self.token = token

    def call(self, method: str, path: str, body=None, timeout=60):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(
            self.base + path, method=method, data=data,
            headers={"Authorization": "Bearer " + self.token,
                     "Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout) as res:
                return res.status, json.loads(res.read().decode() or "{}")
        except urllib.error.HTTPError as exc:
            raw = exc.read().decode(errors="replace")
            try:
                payload = json.loads(raw or "{}")
            except json.JSONDecodeError:
                payload = {"raw": raw[:500]}
            return exc.code, payload


def wait_http(client: Client, deadline: float) -> None:
    while time.time() < deadline:
        try:
            status, _ = client.call("GET", "/api/v1/health", timeout=3)
            if status == 200:
                return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            pass
        time.sleep(0.2)
    raise RuntimeError("Core did not become healthy")


def port_open(port: int) -> bool:
    with socket.socket() as sock:
        sock.settimeout(0.3)
        return sock.connect_ex(("127.0.0.1", port)) == 0


def paseo_json(*args: str):
    proc = run(["paseo", *args, "--json"], timeout=60)
    return json.loads(proc.stdout)


def active_paseo_workspaces() -> set[str]:
    try:
        payload = paseo_json("workspace", "ls")
    except Exception:
        return set()
    ids = set()
    if isinstance(payload, list):
        for item in payload:
            if isinstance(item, dict):
                value = item.get("id") or item.get("workspaceId")
                if value:
                    ids.add(str(value))
    return ids


def managed_server_states(attempt_ids: set[str]) -> list[dict]:
    base = Path(tempfile.gettempdir()) / "factory-test-servers"
    found = []
    if not base.exists():
        return found
    for path in base.rglob("*.json"):
        try:
            payload = json.loads(path.read_text())
        except Exception:
            continue
        if str(payload.get("id", "")) in attempt_ids:
            found.append({"path": str(path), **payload})
    return found


def cleanup_operator(sessions: list[dict], attempts: list[str]) -> dict:
    """Best-effort safety cleanup after evidence capture. Never counted as PASS."""
    notes = []
    for session in sessions:
        sid = session.get("sessionId") or session.get("Id") or ""
        if not sid:
            continue
        for command in (["paseo", "stop", sid], ["paseo", "archive", sid]):
            proc = run(command, check=False)
            if proc.returncode != 0:
                notes.append(f"{command[1]} {sid}: {proc.stderr[-200:]}")
    for state in managed_server_states(set(attempts)):
        try:
            os.killpg(int(state["pgid"]), signal.SIGKILL)
            notes.append(f"killed managed server pgid {state['pgid']}")
        except Exception as exc:
            notes.append(f"managed server cleanup {state.get('path')}: {exc}")
        for key in ("path", "logPath"):
            value = state.get(key)
            if value:
                try:
                    Path(value).unlink()
                except FileNotFoundError:
                    pass
    return {"notes": notes, "performed": bool(notes)}


def main() -> int:
    if os.environ.get("FACTORY_S4C_LIVE") != "1":
        print("Set FACTORY_S4C_LIVE=1 to authorize the single bounded C-07 Vlog campaign.")
        return 0

    tmp = Path(tempfile.mkdtemp(prefix="factory-s4c07-live-"))
    port = int(os.environ.get("FACTORY_S4C07_PORT", "8497"))
    base = f"http://127.0.0.1:{port}"
    token = "s4c07-api-" + secrets.token_hex(16)
    wtoken = "s4c07-worker-" + secrets.token_hex(16)
    vtoken = "s4c07-verifier-" + secrets.token_hex(16)
    rtoken = "s4c07-reviewer-" + secrets.token_hex(16)
    chromium = os.environ.get(
        "CHROMIUM_EXE", str(Path.home() / ".cache/ms-playwright/chromium-1248/chrome-linux64/chrome"))
    playwright_core = os.environ.get(
        "FACTORY_PLAYWRIGHT_CORE", str(ROOT / "office/e2e-browser/node_modules/playwright-core"))
    browser_cli = os.environ.get(
        "FACTORY_BROWSER_CLI", str(ROOT / "office/e2e-browser/node_modules/.bin/playwright-cli"))
    vlog = Path(os.environ.get(
        "S4C_VLOG_REPO", str(Path.home() / "Escritorio/Development/factory-fixtures/factory-fixture-vlog")))
    implementation_sha = git("rev-parse", "HEAD")
    core = worker = None
    core_log = (tmp / "core.log").open("w")
    worker_log = (tmp / "worker.log").open("w")
    result = {
        "campaign": "F1-S4-C-07",
        "implementation_sha": implementation_sha,
        "provider": PROVIDER,
        "model": MODEL,
        "paseo_mode": MODE,
        "project_id": PROJECT_ID,
        "authorization": {
            "max_paid_implementer_sessions": MAX_ATTEMPTS,
            "max_corrective_cycles": MAX_REPAIRS,
            "max_concurrent": MAX_PARALLEL,
        },
        "preflight": {},
        "paid_sessions_spent": 0,
    }

    def record_preflight(name: str, ok: bool, detail) -> None:
        result["preflight"][name] = {"pass": bool(ok), "detail": detail}
        if not ok:
            fail(f"preflight: {name}", preflight=result["preflight"])

    try:
        # --- preflight -------------------------------------------------
        record_preflight("clean-factory-worktree", not git("status", "--porcelain"), "clean")
        record_preflight("implementation-head", bool(re.fullmatch(r"[0-9a-f]{40}", implementation_sha)), implementation_sha)
        record_preflight("vlog-fixture", vlog.is_dir() and (vlog / ".git").is_dir(), str(vlog))
        vlog_head = git("rev-parse", "HEAD", cwd=vlog)
        record_preflight("vlog-clean-worktree", not git("status", "--porcelain", cwd=vlog), vlog_head)
        record_preflight("chromium", Path(chromium).is_file(), chromium)
        record_preflight("playwright-core", Path(playwright_core).is_dir(), playwright_core)
        record_preflight("browser-cli", Path(browser_cli).is_file(), browser_cli)
        record_preflight("managed-port-free", not port_open(4173), "127.0.0.1:4173")

        providers = paseo_json("provider", "ls")
        opencode = [p for p in providers if p.get("provider") == PROVIDER]
        record_preflight("paseo-provider", bool(opencode) and opencode[0].get("status") == "available",
                         opencode[0] if opencode else None)
        models = paseo_json("provider", "models", PROVIDER)
        record_preflight("paseo-model", any(m.get("id") == MODEL for m in models), MODEL)
        paseo_version = run(["paseo", "--version"]).stdout.strip()
        record_preflight("paseo-version", bool(paseo_version), paseo_version)

        run(["go", "build", "-o", str(tmp / "factory-core"), "./cmd/factory-core"], cwd=ROOT / "core")
        run(["go", "build", "-o", str(tmp / "factory-worker"), "./cmd/factory-worker"], cwd=ROOT / "core")
        run(["go", "build", "-o", str(tmp / "factory-verifier"), "./cmd/factory-verifier"], cwd=ROOT / "core")
        run(["go", "build", "-o", str(tmp / "factory-reviewer"), "./cmd/factory-reviewer"], cwd=ROOT / "core")
        run(["npm", "run", "build"], cwd=ROOT / "office")
        record_preflight("clean-build", True, "Core/Worker/Verifier/Reviewer/Office")

        env = os.environ.copy()
        env.update({
            "FACTORY_API_TOKEN": token, "FACTORY_WORKER_TOKEN": wtoken,
            "FACTORY_VERIFIER_TOKEN": vtoken, "FACTORY_REVIEWER_TOKEN": rtoken,
            "CHROMIUM_EXE": chromium, "FACTORY_PLAYWRIGHT_CORE": playwright_core,
            "FACTORY_BROWSER_CLI": browser_cli,
        })
        core = subprocess.Popen(
            [str(tmp / "factory-core"), "--addr", f"127.0.0.1:{port}", "--db", str(tmp / "factory.db"),
             "--web-dir", str(ROOT / "office/dist"), "--verifier-bin", str(tmp / "factory-verifier"),
             "--reviewer-bin", str(tmp / "factory-reviewer")],
            cwd=ROOT, env=env, stdout=core_log, stderr=subprocess.STDOUT,
        )
        client = Client(base, token)
        wait_http(client, time.time() + 30)

        worker_id = "c07-live-" + secrets.token_hex(4)
        worker = subprocess.Popen(
            [str(tmp / "factory-worker"), "--core", base, "--worker-id", worker_id,
             "--exec-mode", "local", "--max-concurrent", str(MAX_PARALLEL),
             "--recovery-interval", "10s"],
            cwd=ROOT, env=env, stdout=worker_log, stderr=subprocess.STDOUT,
        )
        deadline = time.time() + 30
        observed_worker = None
        while time.time() < deadline:
            status, payload = client.call("GET", "/api/v1/workers")
            if status == 200:
                for item in payload.get("workers", []):
                    if item.get("id") == worker_id:
                        observed_worker = item
                        break
            if observed_worker and "browser" in observed_worker.get("capabilities", []):
                break
            time.sleep(0.3)
        record_preflight("worker-registered",
                         bool(observed_worker and "browser" in observed_worker.get("capabilities", [])),
                         observed_worker)
        if worker.poll() is not None:
            fail("real Worker exited during preflight")

        status, payload = client.call("POST", "/api/v1/projects", {
            "id": PROJECT_ID, "name": "Factory Fixture Vlog",
            "repositoryPath": str(vlog), "baseRefs": ["main"],
            "qualityProfile": "node-browser", "workspaceScope": ".",
            "capabilities": ["repository-edit", "node-toolchain", "browser"],
            "browserEndpoints": [ENDPOINT],
            "browserTestServer": {
                "command": ["npm", "start"], "readyPath": "/", "startupTimeoutSecs": 30,
            },
            "executionMode": "trusted-local",
        })
        if status not in (200, 201):
            fail("Vlog Project registration failed", status=status, body=payload)
        project = payload["project"]
        record_preflight("vlog-project-registered", project["id"] == PROJECT_ID, {
            "id": project["id"], "revision": project.get("configRevision"),
            "browserTestServer": project.get("browserTestServer"),
            "browserEndpoints": project.get("browserEndpoints"),
        })

        status, payload = client.call("POST", "/api/v1/actions", {
            "projectId": PROJECT_ID, "title": "F1-S4-C-07 Vlog certification",
            "description": "Bounded real-Paseo Vlog certification",
        })
        if status != 201:
            fail("Action creation failed", status=status, body=payload)
        action = payload["action"]
        goal = (
            "Implement functional category filtering with a no-results state in src/filter.js and "
            "src/app.js. Preserve navigation, accessibility and the five existing entries. The "
            "authorized test server is already managed and running by Factory at "
            "http://127.0.0.1:4173; do not start, restart, or kill it. Use .agents/skills/"
            "factory-browser/SKILL.md and .factory/bin/factory-playwright-cli.mjs. MANDATORY browser "
            "evidence: run `node .factory/bin/factory-playwright-cli.mjs open http://127.0.0.1:4173/` "
            "followed by a successful `node .factory/bin/factory-playwright-cli.mjs snapshot` against "
            "the managed server. Do not use `--help` as browser evidence. Do not edit tests, "
            "browser-check.mjs, package.json, data.js or render.js."
        )
        functional = (
            "node\n--input-type=module\n-e\n"
            "import { filterEntries } from './src/filter.js'; import { entries } from './src/data.js'; "
            "if(filterEntries(entries,'Travel').length!==2||"
            "filterEntries(entries,'Film').length!==0||"
            "filterEntries(entries,'All').length!==5)process.exit(1)"
        )
        negative = (
            "node\n--input-type=module\n-e\n"
            "import { entries, categories } from './src/data.js'; "
            "if(entries.length!==5||categories.length!==5||"
            "!categories.includes('Film')||!categories.includes('All'))process.exit(1)"
        )
        checks = [
            {"id": "format", "kind": "format", "description": "syntax",
             "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 120},
            {"id": "test", "kind": "test", "description": "unit tests",
             "command": ["npm", "test"], "expectExit": 0, "timeoutSecs": 180},
            {"id": "functional", "kind": "functional", "description": "filter assertion",
             "command": functional.split("\n"), "expectExit": 0, "timeoutSecs": 120},
            {"id": "browser", "kind": "browser", "description": "protected browser oracle",
             "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 240},
        ]
        status, payload = client.call("POST", "/api/v1/action-specs", {
            "projectId": PROJECT_ID, "actionId": action["id"], "goal": goal,
            "baseRevision": vlog_head, "allowedPaths": ALLOWED_PATHS,
            "acceptanceCriteria": checks,
            "negativeCases": [{"id": "negative", "kind": "negative",
                               "description": "fixture regression", "command": negative.split("\n"),
                               "expectExit": 0, "timeoutSecs": 120}],
            "riskClass": "R0", "qualityProfile": "node-browser",
            "budget": {"maxAttempts": 1, "timeoutSecs": 900},
            "executionMode": "trusted-local", "provider": PROVIDER,
            "model": MODEL, "paseoMode": MODE,
        })
        if status != 201:
            fail("ActionSpec creation failed", status=status, body=payload)
        spec = payload["spec"]
        frozen_ok = (
            spec.get("browserTestServer", {}).get("command") == ["npm", "start"]
            and spec.get("browserEndpoints") == [ENDPOINT]
            and "browser" in spec.get("capabilities", [])
            and spec.get("allowedPaths") == ALLOWED_PATHS
            and spec.get("provider") == PROVIDER
            and spec.get("model") == MODEL
            and spec.get("paseoMode") == MODE
        )
        record_preflight("frozen-spec-contracts", frozen_ok, {
            "specId": spec.get("id"), "capabilities": spec.get("capabilities"),
            "browserEndpoints": spec.get("browserEndpoints"),
            "browserTestServer": spec.get("browserTestServer"),
            "allowedPaths": spec.get("allowedPaths"), "bindingHash": spec.get("bindingHash"),
            "provider": spec.get("provider"), "model": spec.get("model"),
            "paseoMode": spec.get("paseoMode"),
        })
        if not frozen_ok:
            fail("frozen spec does not carry the authorized contracts")

        status, payload = client.call("POST", "/api/v1/campaigns", {
            "specId": spec["id"], "maxRepairCycles": MAX_REPAIRS,
            "maxWallClockSecs": 1800, "maxParallel": MAX_PARALLEL,
            "maxImplementerAttempts": MAX_ATTEMPTS,
        })
        if status != 201:
            fail("Campaign creation failed", status=status, body=payload)
        campaign = payload["campaign"]
        result["campaign_id"] = campaign["id"]
        result["action_id"] = action["id"]
        result["spec_id"] = spec["id"]
        result["spec_binding_hash"] = spec.get("bindingHash")
        result["vlog_base"] = vlog_head
        result["campaign_bounds"] = {
            "maxAttempts": campaign.get("maxAttempts"),
            "maxRepairCycles": campaign.get("maxRepairCycles"),
            "maxParallel": campaign.get("maxParallel"),
        }
        if (campaign.get("maxAttempts") != MAX_ATTEMPTS
                or campaign.get("maxRepairCycles") != MAX_REPAIRS
                or campaign.get("maxParallel") != MAX_PARALLEL):
            fail("Campaign bounds are not the authorized C-07 bounds", bounds=result["campaign_bounds"])
        if os.environ.get("FACTORY_S4C07_PREFLIGHT_ONLY") == "1":
            result["status"] = "PREFLIGHT_ONLY"
            result["evidence_path"] = str(tmp / "live-evidence.json")
            (tmp / "live-evidence.json").write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2))
            print(f"preflight evidence: {tmp / 'live-evidence.json'}")
            return 0

        # The Worker starts and verifies the managed server before Paseo
        # StartSession. The deterministic preflight already exercised that
        # path; this assertion ties the live log order to the same ordering.
        result["paid_sessions_started_at"] = time.time()
        status, payload = client.call("POST", f"/api/v1/campaigns/{campaign['id']}/start", {})
        if status not in (200, 202):
            fail("Campaign start failed", status=status, body=payload)

        deadline = time.time() + 1900
        projection = {}
        while time.time() < deadline:
            status, projection = client.call("GET", f"/api/v1/campaigns/{campaign['id']}")
            if status != 200:
                fail("campaign projection failed", status=status, body=projection)
            budget = projection.get("budget") or {}
            if int(budget.get("attemptsUsed") or 0) > MAX_ATTEMPTS:
                client.call("POST", f"/api/v1/campaigns/{campaign['id']}/cancel", {})
                fail("authorization ceiling exceeded; campaign cancelled", budget=budget)
            if (projection.get("campaign") or {}).get("status") in TERMINAL:
                break
            time.sleep(2)
        else:
            client.call("POST", f"/api/v1/campaigns/{campaign['id']}/cancel", {})
            fail("campaign exceeded the bounded wall clock")

        terminal = (projection.get("campaign") or {}).get("status")
        result["campaign_status"] = terminal
        result["campaign_terminal_reason"] = (projection.get("campaign") or {}).get("terminalReason")
        result["budget"] = projection.get("budget")
        result["recoveries"] = projection.get("recoveries", [])
        result["decision"] = projection.get("decision")
        final_candidate = projection.get("candidate") or {}
        result["candidate"] = final_candidate
        result["tasks"] = [
            {k: task.get(k) for k in
             ("key", "role", "cycle", "status", "runId", "candidateId", "outcome", "error")}
            for task in projection.get("tasks", [])
        ]
        implementer_tasks = [t for t in projection.get("tasks", []) if t.get("key", "").startswith("implementer-")]
        attempt_ids = [t.get("runId") for t in implementer_tasks if t.get("runId")]
        result["implementer_runs"] = attempt_ids

        # Preserve every real Implementer Run/Attempt/session binding.
        runs = []
        sessions = []
        for run_id in attempt_ids:
            status, run_payload = client.call("GET", f"/api/v1/runs/{run_id}")
            if status != 200:
                fail("failed to load implementer Run", runId=run_id, status=status)
            status, attempts_payload = client.call("GET", f"/api/v1/runs/{run_id}/attempts")
            if status != 200:
                fail("failed to load implementer Attempts", runId=run_id, status=status)
            status, sessions_payload = client.call("GET", f"/api/v1/runs/{run_id}/sessions")
            if status != 200:
                fail("failed to load session bindings", runId=run_id, status=status)
            run_record = run_payload["run"]
            runs.append({"run": run_record, "attempts": attempts_payload.get("attempts", [])})
            sessions.extend(sessions_payload.get("sessions", []))
        result["runs"] = runs
        result["session_bindings"] = sessions
        result["independent_verification"] = {}
        result["review"] = None
        candidates = []
        for run_id in attempt_ids:
            status, candidate_payload = client.call("GET", f"/api/v1/candidates?runId={run_id}")
            if status != 200:
                fail("failed to list Run candidates", runId=run_id, status=status)
            candidates.extend(candidate_payload.get("candidates", []))
        result["candidates"] = candidates
        candidate_evidence = {}
        for candidate in candidates:
            cid = candidate.get("id")
            if not cid:
                continue
            status, bundle_payload = client.call("GET", f"/api/v1/candidates/{cid}/evidence")
            if status == 200:
                candidate_evidence[cid] = bundle_payload.get("bundle")
        result["candidate_evidence"] = candidate_evidence
        if final_candidate.get("id"):
            bundle = candidate_evidence.get(final_candidate["id"], {})
            verifications = bundle.get("verifications", [])
            reviews = bundle.get("reviews", [])
            result["independent_verification"] = verifications
            result["review"] = reviews[0].get("verdict") if reviews else None

        # External Paseo identity and cleanup observations.
        external = []
        for binding in sessions:
            sid = binding.get("sessionId") or binding.get("SessionID") or ""
            if not sid:
                continue
            proc = run(["paseo", "inspect", sid, "--json"], check=False)
            if proc.returncode == 0:
                try:
                    observed = json.loads(proc.stdout)
                except json.JSONDecodeError:
                    observed = {"raw_error": proc.stdout[:300]}
            else:
                observed = {"inspect_error": proc.stderr[-500:]}
            external.append({"sessionId": sid, "inspect": observed})
        result["external_sessions"] = external

        final_run_result = {}
        if runs:
            final_run_result = runs[-1]["run"].get("result") or {}
        result["final_implementer_result"] = final_run_result
        successful = [s.strip() for s in str(final_run_result.get("browserSuccessfulCommands", "")).split(",") if s.strip()]
        nudges = int(final_run_result.get("capabilityNudges") or 0)
        result["browser_certification"] = {
            "successful_commands": successful,
            "invoked": bool(final_run_result.get("browserInvoked")),
            "initial_turn": nudges == 0 and "open" in successful and bool(set(successful) & {
                "snapshot", "find", "fill", "click", "eval", "screenshot", "select", "type", "press", "reload"}),
            "same_session_continuation": nudges == 1 and "open" in successful and bool(set(successful) & {
                "snapshot", "find", "fill", "click", "eval", "screenshot", "select", "type", "press", "reload"}),
            "capabilityNudges": nudges,
            "testServerManaged": bool(final_run_result.get("testServerManaged")),
            "testServerEndpoint": final_run_result.get("testServerEndpoint"),
            "testServerReused": final_run_result.get("testServerReused"),
            "sessionId": final_run_result.get("sessionId"),
            "workspaceId": final_run_result.get("workspaceId"),
        }

        # Managed server cleanup: no state row for these Attempts and no listener.
        states = managed_server_states(set(attempt_ids))
        time.sleep(2)
        # Session archive is asynchronous in Paseo; observe it bounded.
        archive_deadline = time.time() + 30
        while time.time() < archive_deadline:
            pending = [
                item for item in external
                if not (item.get("inspect") or {}).get("Archived")
            ]
            if not pending:
                break
            time.sleep(1)
            for item in pending:
                sid = item.get("sessionId")
                proc = run(["paseo", "inspect", sid, "--json"], check=False)
                if proc.returncode == 0:
                    try:
                        item["inspect"] = json.loads(proc.stdout)
                    except json.JSONDecodeError:
                        pass
        result["cleanup"] = {
            "managed_server_state_rows": states,
            "managed_server_port_open": port_open(4173),
            "managed_server_clean": not states and not port_open(4173),
            "session_cleanup": [
                {
                    "sessionId": item.get("sessionId"),
                    "archived": (item.get("inspect") or {}).get("Archived"),
                    "status": (item.get("inspect") or {}).get("Status"),
                }
                for item in external
            ],
            "workspace_active": sorted(
                {str(r["run"].get("result", {}).get("workspaceId", "")) for r in runs}
                & active_paseo_workspaces()
            ),
        }

        # Certification conditions. The Core verifier is the independent
        # functional/Chromium authority; this script does not replace it.
        verifier_checks = {
            (check.get("id")): check
            for verification in result.get("independent_verification", [])
            for check in verification.get("checks", [])
        }
        candidate_paths = set(final_candidate.get("changedPaths") or [])
        final_run_identity_ok = bool(runs) and (
            (runs[-1].get("run") or {}).get("provider") == PROVIDER
            and (runs[-1].get("run") or {}).get("model") == MODEL
            and (runs[-1].get("run") or {}).get("paseoMode") == MODE
        )
        external_identity_ok = all(
            (item.get("inspect") or {}).get("Provider") == PROVIDER
            and (item.get("inspect") or {}).get("Model") == MODEL
            and (item.get("inspect") or {}).get("Mode") == MODE
            for item in external
        ) if external else False
        browser_ok = (
            result["browser_certification"]["initial_turn"]
            or result["browser_certification"]["same_session_continuation"]
        )
        server_ok = (
            result["browser_certification"]["testServerManaged"] is True
            and result["browser_certification"]["testServerEndpoint"] == ENDPOINT
        )
        verification_ok = (
            verifier_checks.get("browser", {}).get("status") == "PASS"
            and "chromium" in str(verifier_checks.get("browser", {}).get("detail", "")).lower()
            and verifier_checks.get("functional", {}).get("status") == "PASS"
        )
        gate_ok = terminal == "accepted" and (result.get("decision") or {}).get("status") == "ACCEPTED"
        cleanup_ok = (
            result["cleanup"]["managed_server_clean"]
            and not result["cleanup"]["workspace_active"]
            and all(item.get("archived") for item in result["cleanup"]["session_cleanup"])
        )
        result["certification"] = {
            "identity": final_run_identity_ok,
            "external_identity": external_identity_ok,
            "candidate_allowed_paths": bool(candidate_paths) and candidate_paths <= set(ALLOWED_PATHS),
            "browser_interaction": browser_ok,
            "managed_server": server_ok,
            "independent_verification": verification_ok,
            "review": result.get("review") == "APPROVE",
            "gate_accepted": gate_ok,
            "cleanup": cleanup_ok,
            "initial_vs_continuation": (
                "initial" if result["browser_certification"]["initial_turn"] else
                "same-session-continuation" if result["browser_certification"]["same_session_continuation"] else
                "none"
            ),
            "real_corrective_attempt": len(bound_session_ids) > 1 and len(attempt_ids) > 1,
        }
        bound_session_ids = {
            str(item.get("sessionId") or item.get("SessionID") or "")
            for item in sessions
        } - {""}
        result["paid_sessions_spent"] = len(bound_session_ids)
        result["implementer_attempts_used"] = len(attempt_ids)
        result["status"] = (
            "MERGE_READY_FOR_PRODUCT_OWNER_REVIEW"
            if all(v for k, v in result["certification"].items()
                   if k not in {"initial_vs_continuation", "real_corrective_attempt"})
            else "HUMAN_REQUIRED"
        )

        # Worker log ordering: managed server ready precedes any session
        # observation. If the managed start failed, no Paseo session starts.
        worker_log.flush()
        log_text = (tmp / "worker.log").read_text(errors="replace")
        ready_at = log_text.find("managed browser test server ready")
        session_at = log_text.find("paseo session observed")
        result["managed_server_log_order"] = {
            "ready": ready_at >= 0,
            "ready_before_session": ready_at >= 0 and (session_at < 0 or ready_at < session_at),
        }

        if result["paid_sessions_spent"] > MAX_ATTEMPTS:
            fail("authorization ceiling exceeded", paid_sessions=result["paid_sessions_spent"])
        if result["status"] != "MERGE_READY_FOR_PRODUCT_OWNER_REVIEW":
            result["operator_cleanup"] = cleanup_operator(external, attempt_ids)
        result["evidence_path"] = str(tmp / "live-evidence.json")
        (tmp / "live-evidence.json").write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2)[-12000:])
        print(f"evidence: {tmp / 'live-evidence.json'}")
        print(f"core log: {tmp / 'core.log'}")
        print(f"worker log: {tmp / 'worker.log'}")
        return 0 if result["status"] == "MERGE_READY_FOR_PRODUCT_OWNER_REVIEW" else 1
    finally:
        for proc in (worker, core):
            if proc and proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    proc.kill()
        core_log.close()
        worker_log.close()


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except RuntimeError as exc:
        fail(str(exc))
