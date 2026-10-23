#!/usr/bin/env python3
"""Bounded real F1-S4-D multi-WorkUnit certification.

Default: local negative authorization preflight only; no provider call.
Live: requires explicit campaign authorization, exact clean SHA, the fixed
route and at most four Factory-managed provider sessions. The integration
node is deterministic and must not create a provider session.
"""
from __future__ import annotations

import json
import os
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
CAMPAIGN = "F1-S4-D-GRAPH-LIVE"
PROJECT = "factory-fixture-s4d-graph"
PROVIDER, MODEL, MODE = "opencode", "opencode-go/deepseek-v4.1-flash", "build"
ROUTE = f"{PROVIDER}/{MODEL}"
AUTHORIZATION_ID = "s4d-graph-owner-auth-1"
SESSIONS = 4
ATTEMPTS = 3
REPAIRS = 0
PARALLEL = 2
MAX_WALL_CLOCK_SECS = 1800
RUNNER_DEADLINE_SECS = 2400
PATHS = ["src/greeting.js", "src/app.js"]
FIXTURE = ROOT / "scripts/fixtures/s4d-graph"
TERMINAL = {"accepted", "repair_required", "blocked", "human_required", "failed", "cancelled"}
OBJECTIVE = (
    "Implement the disposable S4-D graph fixture as exactly two independent implementation WorkUnits. "
    "WorkUnit backend updates only src/greeting.js so greeting(name) returns 'Hello, <name>!' and throws TypeError for empty or non-string names. "
    "WorkUnit frontend updates only src/app.js so the browser renders greeting('Factory') in the #greeting element. "
    "Use class feature, risk R0, node-basic profile, one attempt with a 600-second timeout, zero repair cycles, maximum parallel two, and four total sessions including this assessment. "
    "The two units must have no dependency; Core will add deterministic integration. "
    "Acceptance checks exactly: format 'npm run format'; test 'node --test tests/unit/greeting.test.mjs tests/unit/app.test.mjs'; functional node --input-type=module -e \"import('./src/greeting.js').then(m=>{if(m.greeting('Factory')!=='Hello, Factory!')process.exit(1)})\"; browser 'node tests/acceptance/browser-check.mjs'. "
    "Negative check: empty name is rejected as TypeError and exits 0. "
    "Because the units are independent, provide per-unit acceptanceCriteria and negativeCases: backend uses node --check src/greeting.js, node --test tests/unit/greeting.test.mjs, the greeting functional command, and the empty-name TypeError negative; frontend uses node --check src/app.js, node --test tests/unit/app.test.mjs, a DOM-stub module load, and a source check that app.js uses greeting. "
    "The global checks above apply to the final integrated candidate. Do not modify tests. Cite only supplied evidence references."
)


def cmd(args, *, cwd=ROOT, env=None, timeout=120, check=True):
    p = subprocess.run(args, cwd=cwd, env=env, text=True, capture_output=True, timeout=timeout)
    if check and p.returncode:
        raise RuntimeError(f"{args[0]} exited {p.returncode}: {p.stderr[-1200:]} {p.stdout[-1200:]}")
    return p


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def guard(env, requested=True):
    args = [sys.executable, str(ROOT / "scripts/live-e2e-authorization.py"),
            "--campaign", CAMPAIGN, "--project", PROJECT, "--max-sessions", str(SESSIONS),
            "--route", ROUTE]
    if requested:
        args += ["--requested-route", ROUTE]
    return cmd(args, env=env, check=False)


def safe_preflight() -> int:
    env = os.environ.copy()
    for key in ("FACTORY_LIVE_MODE", "FACTORY_LIVE_AUTHORIZATION", "FACTORY_LIVE_AUTH_CAMPAIGN",
                "FACTORY_LIVE_AUTH_PROJECT", "FACTORY_LIVE_AUTH_SESSION_BUDGET",
                "FACTORY_LIVE_AUTH_ROUTES", "FACTORY_LIVE_AUTH_FACTORY_SHA",
                "FACTORY_LIVE_AUTHORIZATION_ID"):
        env.pop(key, None)
    result = guard(env)
    rejected = result.returncode == 2 and "[BLOCKED]" in result.stderr
    print(json.dumps({
        "status": "PREFLIGHT_SAFE" if rejected else "PREFLIGHT_FAILED",
        "providerCalls": 0, "guardRejectedMissingOwnerAuthorization": rejected,
        "liveRunnerOptIn": "FACTORY_S4D_GRAPH_LIVE=1",
        "liveExecutionImplemented": True, "route": ROUTE,
        "workUnits": 2, "integration": "deterministic-git", "sessionCeiling": SESSIONS,
        "attemptCeiling": ATTEMPTS, "maxRepairCycles": REPAIRS,
        "maxParallel": PARALLEL, "maxWallClockSecs": MAX_WALL_CLOCK_SECS,
        "runnerDeadlineSecs": RUNNER_DEADLINE_SECS,
    }, indent=2))
    return 0 if rejected else 1


def require_authority() -> str:
    if os.environ.get("FACTORY_S4D_GRAPH_LIVE") != "1":
        raise RuntimeError("FACTORY_S4D_GRAPH_LIVE=1 is required")
    p = guard(os.environ.copy())
    if p.returncode:
        raise RuntimeError("live-e2e-authorization.py rejected owner envelope: " + p.stderr.strip())
    if os.environ.get("FACTORY_LIVE_AUTH_SESSION_BUDGET") != str(SESSIONS):
        raise RuntimeError(f"owner must authorize exactly {SESSIONS} total sessions")
    if os.environ.get("FACTORY_LIVE_AUTHORIZATION_ID") != AUTHORIZATION_ID:
        raise RuntimeError(f"owner envelope must bind authorization ID {AUTHORIZATION_ID}")
    frozen = os.environ.get("FACTORY_LIVE_AUTH_FACTORY_SHA", "")
    head = cmd(["git", "rev-parse", "HEAD"]).stdout.strip()
    if len(frozen) != 40 or frozen != head:
        raise RuntimeError(f"FACTORY_LIVE_AUTH_FACTORY_SHA must exactly match frozen HEAD {head}")
    if cmd(["git", "status", "--porcelain"]).stdout.strip():
        raise RuntimeError("Factory worktree must be clean")
    return head


class API:
    def __init__(self, base, token):
        self.base, self.token = base, token

    def call(self, method, path, body=None, timeout=60, idem=None):
        data = None if body is None else json.dumps(body).encode()
        headers = {"Authorization": "Bearer " + self.token, "Content-Type": "application/json"}
        if idem:
            headers["Idempotency-Key"] = idem
        req = urllib.request.Request(self.base + path, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return r.status, json.loads(r.read().decode() or "{}")
        except urllib.error.HTTPError as e:
            try:
                return e.code, json.loads(e.read().decode() or "{}")
            except json.JSONDecodeError:
                return e.code, {}


def wait_health(api, timeout=40):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            if api.call("GET", "/api/v1/health", timeout=2)[0] == 200:
                return
        except (OSError, TimeoutError):
            pass
        time.sleep(0.2)
    raise RuntimeError("Core health timeout")


def wait_worker(api, worker_id, timeout=40):
    deadline = time.time() + timeout
    while time.time() < deadline:
        status, payload = api.call("GET", "/api/v1/workers")
        if status == 200 and any(w.get("id") == worker_id for w in payload.get("workers", [])):
            return
        time.sleep(0.25)
    raise RuntimeError("Worker did not register")


def wait_lead(api, request_id, timeout):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        status, payload = api.call("GET", f"/api/v1/lead/requests/{request_id}")
        if status == 200:
            last = payload["request"]
            if last.get("status") in TERMINAL or last.get("campaignStatus") in TERMINAL:
                return last
        time.sleep(1)
    raise RuntimeError(f"Lead request did not terminate; last={json.dumps(last)[:1200]}")


def browser_phase(env, phase, result_file, extra=None):
    child = env.copy()
    child.update({"PHASE": phase, "RESULT_FILE": str(result_file)})
    if extra:
        child.update(extra)
    try:
        cmd(["node", "office/e2e-browser/s4d-graph-live.mjs"], env=child, timeout=400)
    except subprocess.TimeoutExpired:
        if Path(result_file).exists():
            return json.loads(Path(result_file).read_text())
        return {"phase": phase, "ok": False, "error": "browser phase timed out"}
    return json.loads(Path(result_file).read_text())


def session_cleanup(session_ids, paseo_bin):
    cleaned, failed = [], []
    for session_id in sorted(set(session_ids)):
        stop = cmd([paseo_bin, "stop", session_id], check=False)
        archive = cmd([paseo_bin, "archive", session_id], check=False)
        item = {"sessionId": session_id, "stopExit": stop.returncode, "archiveExit": archive.returncode}
        (cleaned if stop.returncode == 0 and archive.returncode == 0 else failed).append(item)
    return cleaned, failed


def main() -> int:
    dry_run = os.environ.get("FACTORY_S4D_GRAPH_DRY_RUN") == "1"
    if not dry_run and os.environ.get("FACTORY_S4D_GRAPH_LIVE") != "1":
        return safe_preflight()
    factory_sha = "DRY_RUN" if dry_run else require_authority()
    if not dry_run:
        # Re-check the exact route and total session ceiling before dispatch.
        if not (PROVIDER == "opencode" and MODEL == "opencode-go/deepseek-v4.1-flash" and MODE == "build"):
            raise RuntimeError("authorized route changed")

    root = Path(tempfile.mkdtemp(prefix="factory-s4d-graph-live-"))
    repo = root / "project"
    evidence_path = Path(os.environ.get("FACTORY_S4D_GRAPH_EVIDENCE", root / "live-evidence.json"))
    bins = {name: root / f"factory-{name}" for name in ("core", "worker", "verifier", "reviewer")}
    processes = []
    log = (root / "services.log").open("w")
    evidence = {
        "campaign": CAMPAIGN, "authorizationId": AUTHORIZATION_ID,
        "factorySha": factory_sha, "projectId": PROJECT, "fixture": "s4d-graph",
        "route": ROUTE, "dryRun": dry_run,
        "budget": {"totalSessions": SESSIONS, "attempts": ATTEMPTS,
                   "repairs": REPAIRS, "maxParallel": PARALLEL,
                   "maxWallClockSecs": MAX_WALL_CLOCK_SECS},
        "status": "INCOMPLETE", "sessions": [], "cleanup": {},
    }
    api = None
    request_id = ""
    paseo_bin = "paseo"
    try:
        for name, binary in bins.items():
            cmd(["go", "build", "-o", str(binary), f"./cmd/factory-{name}"],
                cwd=ROOT / "core", timeout=300)
        cmd(["npm", "run", "build"], cwd=ROOT / "office", timeout=300)
        shutil.copytree(FIXTURE, repo, ignore=shutil.ignore_patterns(".git", "node_modules"))
        cmd(["git", "init", "-q", "-b", "main"], cwd=repo)
        cmd(["git", "add", "."], cwd=repo)
        cmd(["git", "-c", "user.name=Factory fixture", "-c", "user.email=factory@localhost",
             "commit", "-qm", "S4-D disposable baseline"], cwd=repo)
        baseline = cmd(["git", "rev-parse", "HEAD"], cwd=repo).stdout.strip()
        evidence["fixtureBaseline"] = baseline

        port = free_port()
        base = f"http://127.0.0.1:{port}"
        ids = {k: "s4d-" + secrets.token_hex(16) for k in ("api", "worker", "verifier", "reviewer")}
        env = os.environ.copy()
        env.update({
            "FACTORY_API_TOKEN": ids["api"], "FACTORY_WORKER_TOKEN": ids["worker"],
            "FACTORY_VERIFIER_TOKEN": ids["verifier"], "FACTORY_REVIEWER_TOKEN": ids["reviewer"],
            "FACTORY_PLAYWRIGHT_CORE": str(ROOT / "office/e2e-browser/node_modules/playwright-core"),
            "CHROMIUM_EXE": os.environ.get("CHROMIUM_EXE", str(Path.home() / ".cache/ms-playwright/chromium-1248/chrome-linux64/chrome")),
        })
        if dry_run:
            paseo_bin = str(ROOT / "scripts/s4d_graph_paseo_stub.py")
            stub_dir = Path("/tmp/s4d-graph-paseo-stub")
            shutil.rmtree(stub_dir, ignore_errors=True)
            stub_dir.mkdir(parents=True, exist_ok=True)
            env["PASEO_BIN"] = paseo_bin
            env["S4D_GRAPH_STUB_DIR"] = str(stub_dir)
        else:
            paseo = shutil.which("paseo")
            if not paseo:
                raise RuntimeError("paseo CLI is required for live certification")
            paseo_bin = paseo
            env["PASEO_BIN"] = paseo_bin

        processes.append(subprocess.Popen([
            str(bins["core"]), "--addr", f"127.0.0.1:{port}", "--db", str(root / "factory.db"),
            "--web-dir", str(ROOT / "office/dist"),
            "--verifier-bin", str(bins["verifier"]), "--reviewer-bin", str(bins["reviewer"]),
            "--lead-paid-project-id", PROJECT, "--lead-paid-base-revision", baseline,
            "--lead-paid-provider", PROVIDER, "--lead-paid-model", MODEL,
            "--lead-paid-paseo-mode", MODE, "--lead-paid-max-sessions", str(SESSIONS),
            "--lead-paid-authorization-id", AUTHORIZATION_ID,
        ], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT))
        api = API(base, ids["api"])
        wait_health(api)
        worker_id = "s4d-graph-worker-" + secrets.token_hex(4)
        processes.append(subprocess.Popen([
            str(bins["worker"]), "--core", base, "--worker-id", worker_id,
            "--exec-mode", "local", "--max-concurrent", "2", "--recovery-interval", "0",
        ], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT))
        wait_worker(api, worker_id)

        status, payload = api.call("POST", "/api/v1/projects", {
            "id": PROJECT, "name": "Disposable S4-D Graph fixture", "repositoryPath": str(repo),
            "baseRefs": ["main"], "qualityProfile": "node-basic", "workspaceScope": ".",
            "capabilities": ["repository-edit", "node-toolchain"], "executionMode": "trusted-local",
        })
        if status not in (200, 201):
            raise RuntimeError(f"project registration failed: {status} {payload}")
        expires = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() + 3600))
        status, payload = api.call("POST", "/api/v1/delegations", {
            "projectId": PROJECT, "revision": 1, "objectiveClasses": ["feature"],
            "allowedPaths": ["src"], "capabilities": ["repository-edit", "node-toolchain"],
            "riskClasses": ["R0"], "provider": PROVIDER, "model": MODEL, "paseoMode": MODE,
            "maxAttempts": 1, "maxTimeoutSecs": 600, "maxRepairCycles": REPAIRS,
            "maxWallClockSecs": MAX_WALL_CLOCK_SECS, "maxParallel": PARALLEL,
            "maxSessions": SESSIONS, "maxRequests": 1, "maxCumulativeSessions": SESSIONS,
            "maxCumulativeAttempts": ATTEMPTS, "autonomousStart": True,
            "autonomousRiskClasses": ["R0"], "expiresAt": expires,
        }, idem=AUTHORIZATION_ID)
        if status not in (200, 201):
            raise RuntimeError(f"delegation creation failed: {status} {payload}")
        grant = payload["delegation"]
        evidence["grant"] = {k: grant.get(k) for k in ("id", "revision", "contentHash", "expiresAt", "maxSessions", "maxCumulativeSessions")}

        browser_env = env.copy()
        browser_env.update({"BASE_URL": base, "FACTORY_API_TOKEN": ids["api"],
                            "PROJECT_ID": PROJECT, "DELEGATION_ID": grant["id"],
                            "OBJECTIVE": OBJECTIVE})
        submit = browser_phase(browser_env, "submit", root / "browser-submit.json")
        evidence["browserSubmit"] = submit
        if not submit.get("ok") or not submit.get("requestId"):
            raise RuntimeError(f"Office submission failed: {submit}")
        request_id = submit["requestId"]
        evidence["requestId"] = request_id
        lead = wait_lead(api, request_id, RUNNER_DEADLINE_SECS)
        evidence["lead"] = {
            "status": lead.get("status"), "startMode": lead.get("startMode"),
            "authorizationState": lead.get("authorizationState"),
            "campaignId": lead.get("campaignId"), "planHash": lead.get("planHash"),
            "plan": lead.get("plan"), "delegationUsage": lead.get("delegationUsage"),
        }
        if lead.get("status") != "accepted":
            raise RuntimeError(f"Lead did not reach accepted: {lead.get('status')} {lead.get('failureReason')}")
        if lead.get("startMode") != "autonomous":
            raise RuntimeError(f"Campaign did not start autonomously: {lead.get('startMode')}")
        plan = lead.get("plan") or {}
        units = plan.get("workUnits") or []
        if len(units) != 2:
            raise RuntimeError(f"expected exactly two WorkUnits, got {len(units)}")
        if any(unit.get("dependsOn") for unit in units):
            raise RuntimeError("expected two independent WorkUnits for deterministic integration")
        if not any(unit.get("allowedPaths") == ["src/greeting.js"] for unit in units) or not any(unit.get("allowedPaths") == ["src/app.js"] for unit in units):
            raise RuntimeError(f"WorkUnit paths do not match the fixture: {units}")
        status, projection = api.call("GET", f"/api/v1/campaigns/{lead['campaignId']}")
        if status != 200:
            raise RuntimeError(f"campaign projection failed: {status} {projection}")
        if projection.get("campaign", {}).get("status") != "accepted" or projection.get("decision", {}).get("status") != "ACCEPTED":
            raise RuntimeError(f"campaign/gate not accepted: {projection.get('campaign', {}).get('status')} {projection.get('decision', {}).get('status')}")
        tasks = projection.get("tasks") or []
        if not any(task.get("role") == "quality_gate" and task.get("status") == "succeeded" for task in tasks):
            raise RuntimeError("quality gate task did not succeed")
        runsByID, candidates = {}, []
        for task in tasks:
            if task.get("runId"):
                status, run = api.call("GET", f"/api/v1/runs/{task['runId']}")
                if status == 200:
                    body = run.get("run") or run
                    runsByID[body["id"]] = body
            if task.get("candidateId"):
                candidates.append(task["candidateId"])
        runs = list(runsByID.values())
        integration_runs = [run for run in runs if run.get("workloadKind") == "git-integration"]
        if len(integration_runs) != 1:
            raise RuntimeError(f"expected one deterministic integration run, got {len(integration_runs)}")
        integration_sessions = api.call("GET", f"/api/v1/runs/{integration_runs[0]['id']}/sessions")[1]
        if integration_sessions.get("sessions"):
            raise RuntimeError("deterministic integration unexpectedly created a provider session")
        candidate_details = []
        for candidate_id in sorted(set(candidates)):
            status, payload = api.call("GET", f"/api/v1/candidates/{candidate_id}")
            if status == 200:
                candidate_details.append(payload.get("candidate") or payload)
        integration_candidates = [c for c in candidate_details if c.get("deterministicIntegration")]
        if len(integration_candidates) != 1:
            raise RuntimeError("expected exactly one deterministic integration candidate")
        integrated = integration_candidates[0]
        if not {"src/greeting.js", "src/app.js"}.issubset(set(integrated.get("changedPaths") or [])):
            raise RuntimeError(f"integrated candidate paths incomplete: {integrated.get('changedPaths')}")
        sessions = []
        assessment_session = (lead.get("modelObservation") or {}).get("sessionId")
        if assessment_session:
            sessions.append(assessment_session)
        for run in runs:
            if run.get("workloadKind") == "paseo-task":
                status, payload = api.call("GET", f"/api/v1/runs/{run['id']}/sessions")
                for session in payload.get("sessions", []):
                    sessions.append(session.get("sessionId"))
        sessions = sorted(set(s for s in sessions if s))
        if len(sessions) > SESSIONS or len(sessions) < 3:
            raise RuntimeError(f"unexpected provider session count: {len(sessions)} {sessions}")
        evidence["sessions"] = sessions
        evidence["runs"] = runs
        evidence["candidates"] = candidate_details
        evidence["campaign"] = projection

        # Restart Core on the same DB and verify durable acceptance/projection.
        processes[0].terminate()
        try:
            processes[0].wait(timeout=20)
        except subprocess.TimeoutExpired:
            processes[0].kill()
        processes[0] = subprocess.Popen([
            str(bins["core"]), "--addr", f"127.0.0.1:{port}", "--db", str(root / "factory.db"),
            "--web-dir", str(ROOT / "office/dist"),
            "--verifier-bin", str(bins["verifier"]), "--reviewer-bin", str(bins["reviewer"]),
            "--lead-paid-project-id", PROJECT, "--lead-paid-base-revision", baseline,
            "--lead-paid-provider", PROVIDER, "--lead-paid-model", MODEL,
            "--lead-paid-paseo-mode", MODE, "--lead-paid-max-sessions", str(SESSIONS),
            "--lead-paid-authorization-id", AUTHORIZATION_ID,
        ], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        wait_health(api)
        status, after_restart = api.call("GET", f"/api/v1/lead/requests/{request_id}")
        if status != 200 or after_restart["request"].get("status") != "accepted":
            raise RuntimeError("accepted Lead projection did not survive Core restart")
        evidence["afterRestart"] = after_restart["request"]
        verify = browser_phase(browser_env, "verify", root / "browser-verify.json", {"REQUEST_ID": request_id})
        evidence["browserVerify"] = verify
        if not verify.get("ok"):
            raise RuntimeError(f"Office post-restart verification failed: {verify}")
        evidence["status"] = "ACCEPTED_AND_VERIFIED"
    except Exception as exc:
        evidence["status"] = "INCOMPLETE"
        evidence["failure"] = str(exc)
        print(f"[FAIL] {exc}", file=sys.stderr)
    finally:
        try:
            if api is not None and request_id:
                status, payload = api.call("GET", f"/api/v1/lead/requests/{request_id}")
                if status == 200:
                    session_id = (payload["request"].get("modelObservation") or {}).get("sessionId")
                    if session_id:
                        evidence["sessions"] = sorted(set(evidence.get("sessions", [])) | {session_id})
                    for task in payload["request"].get("tasks", []) or []:
                        pass
            cleaned, failed = session_cleanup(evidence.get("sessions", []), paseo_bin)
            evidence["cleanup"]["sessions"] = cleaned
            evidence["cleanup"]["failedSessions"] = failed
            evidence["cleanup"]["servicesStopped"] = True
            evidence["cleanup"]["fixtureRemoved"] = False
        except Exception as exc:
            evidence["cleanup"] = {"error": str(exc)}
        for proc in processes:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=5)
        log.close()
        keep_root = os.environ.get("FACTORY_S4D_GRAPH_KEEP") == "1" or evidence["status"] != "ACCEPTED_AND_VERIFIED"
        if keep_root:
            evidence["workRoot"] = str(root)
        else:
            shutil.rmtree(root, ignore_errors=True)
            evidence["cleanup"]["fixtureRemoved"] = not root.exists()
        evidence_path.parent.mkdir(parents=True, exist_ok=True)
        evidence_path.write_text(json.dumps(evidence, indent=2) + "\n")
        print(f"evidence: {evidence_path}")
    return 0 if evidence["status"] == "ACCEPTED_AND_VERIFIED" else 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, OSError, subprocess.SubprocessError) as exc:
        print("[BLOCKED] live authorization: " + str(exc), file=sys.stderr)
        raise SystemExit(2)
