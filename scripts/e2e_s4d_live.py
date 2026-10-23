#!/usr/bin/env python3
"""Bounded S4-D Lead-to-Campaign live acceptance.

Default: local negative authorization preflight only. Live path requires an
explicit Product Owner envelope, exact frozen Factory SHA, and opt-in flag.
Never run the live path without separate owner authorization.
"""
from __future__ import annotations

import json
import os
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CAMPAIGN = "F1-S4-D-LIVE"
PROJECT = "factory-fixture-lead"
PROVIDER, MODEL, MODE = "opencode", "opencode-go/deepseek-v4.1-flash", "build"
ROUTE = f"{PROVIDER}/{MODEL}"
SESSIONS, ATTEMPTS, REPAIRS, PARALLEL = 2, 1, 0, 1
AUTHORIZATION_ID = "s4d-live-owner-auth-1"
FIXTURE = ROOT / "scripts/fixtures/s4a-ui"
PATHS = ["add.js"]
OBJECTIVE = (
    "Update only add.js so add(a,b) accepts finite numbers and throws TypeError otherwise. "
    "Use one implementation WorkUnit with path add.js, class feature, risk R0, one attempt, "
    "zero repair cycles, one parallel slot, and two total sessions including this assessment. "
    "Preserve node-basic. Checks exactly: format node --check add.js; test npm test; functional "
    "node -e \"const {add}=require('./add.js');if(add(2,3)!==5||add(-2,2)!==0)process.exit(1)\"; "
    "negative node -e \"const {add}=require('./add.js');for(const a of [['2',3],[Infinity,1],[NaN,1]]){let x=false;try{add(...a)}catch(e){x=e instanceof TypeError}if(!x)process.exit(1)}\". "
    "Every check expects exit 0."
)
TERMINAL = {"accepted", "repair_required", "blocked", "human_required", "failed", "cancelled"}


def cmd(args, *, cwd=ROOT, env=None, timeout=60, check=True):
    p = subprocess.run(args, cwd=cwd, env=env, text=True, capture_output=True, timeout=timeout)
    if check and p.returncode:
        raise RuntimeError(f"{args[0]} exited {p.returncode}: {p.stderr[-1000:]} {p.stdout[-1000:]}")
    return p


def guard(env, *, requested=True):
    args = [sys.executable, str(ROOT / "scripts/live-e2e-authorization.py"),
            "--campaign", CAMPAIGN, "--project", PROJECT, "--max-sessions", str(SESSIONS),
            "--route", ROUTE]
    if requested:
        args += ["--requested-route", ROUTE]
    return cmd(args, env=env, check=False)


def safe_preflight():
    env = os.environ.copy()
    for key in ("FACTORY_LIVE_MODE", "FACTORY_LIVE_AUTHORIZATION", "FACTORY_LIVE_AUTH_CAMPAIGN",
                "FACTORY_LIVE_AUTH_PROJECT", "FACTORY_LIVE_AUTH_SESSION_BUDGET",
                "FACTORY_LIVE_AUTH_ROUTES", "FACTORY_LIVE_AUTH_FACTORY_SHA",
                "FACTORY_LIVE_AUTHORIZATION_ID"):
        env.pop(key, None)
    result = guard(env)
    rejected = result.returncode == 2 and "[BLOCKED]" in result.stderr
    print(json.dumps({"status": "PREFLIGHT_SAFE" if rejected else "PREFLIGHT_FAILED",
                      "providerCalls": 0, "guardRejectedMissingOwnerAuthorization": rejected,
                      "liveRunnerOptIn": "FACTORY_S4D_LIVE=1", "exactFactorySHARequired": True,
                      "route": ROUTE, "sessionBudget": {"total": 2, "assessment": 1, "implementer": 1}}, indent=2))
    return 0 if rejected else 1


class API:
    def __init__(self, base, token):
        self.base, self.token = base, token

    def call(self, method, path, body=None, timeout=40, idem=None):
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


def require_authority():
    if os.environ.get("FACTORY_S4D_LIVE") != "1":
        raise RuntimeError("FACTORY_S4D_LIVE=1 is required")
    p = guard(os.environ.copy())
    if p.returncode:
        raise RuntimeError("live-e2e-authorization.py rejected owner envelope: " + p.stderr.strip())
    if os.environ.get("FACTORY_LIVE_AUTH_SESSION_BUDGET") != str(SESSIONS):
        raise RuntimeError(f"owner must authorize exactly {SESSIONS} total sessions")
    if os.environ.get("FACTORY_LIVE_AUTHORIZATION_ID") != AUTHORIZATION_ID:
        raise RuntimeError(f"owner envelope must explicitly bind authorization ID {AUTHORIZATION_ID}")
    frozen = os.environ.get("FACTORY_LIVE_AUTH_FACTORY_SHA", "")
    head = cmd(["git", "rev-parse", "HEAD"]).stdout.strip()
    if len(frozen) != 40 or frozen != head:
        raise RuntimeError(f"FACTORY_LIVE_AUTH_FACTORY_SHA must exactly match frozen HEAD {head}")
    if cmd(["git", "status", "--porcelain"]).stdout.strip():
        raise RuntimeError("Factory worktree must be clean")
    return head


def check(condition, message):
    if not condition:
        raise RuntimeError(message)


def main_live():
    factory_sha = require_authority()
    for name in ("go", "node", "npm", "paseo"):
        check(shutil.which(name), f"missing executable {name}")
    port = int(os.environ.get("FACTORY_S4D_PORT", "8514"))
    with socket.socket() as sock:
        check(sock.connect_ex(("127.0.0.1", port)) != 0, f"port {port} already in use")

    root = Path(tempfile.mkdtemp(prefix="factory-s4d-live-"))
    repo = root / "project"
    shutil.copytree(FIXTURE, repo, ignore=shutil.ignore_patterns(".git", "node_modules"))
    cmd(["git", "init", "-q", "-b", "main"], cwd=repo)
    cmd(["git", "add", "."], cwd=repo)
    cmd(["git", "-c", "user.name=Factory fixture", "-c", "user.email=factory@localhost",
         "commit", "-qm", "S4-D disposable baseline"], cwd=repo)
    baseline = cmd(["git", "rev-parse", "HEAD"], cwd=repo).stdout.strip()

    ids = {k: "s4d-" + secrets.token_hex(16) for k in ("api", "worker", "verifier", "reviewer")}
    bins = {k: root / ("factory-" + k) for k in ("core", "worker", "verifier", "reviewer")}
    processes, sessions, evidence = [], set(), {
        "campaign": CAMPAIGN, "authorizationId": AUTHORIZATION_ID,
        "factorySha": factory_sha, "projectId": PROJECT,
        "fixtureBaseline": baseline, "route": ROUTE,
        "budget": {"totalSessions": SESSIONS, "assessment": 1, "implementer": 1,
                   "attempts": ATTEMPTS, "repairs": REPAIRS}, "status": "INCOMPLETE"}
    api = None
    request_id = run_id = ""
    log = (root / "services.log").open("w")
    try:
        for name in bins:
            cmd(["go", "build", "-o", str(bins[name]), f"./cmd/factory-{name}"],
                cwd=ROOT / "core", timeout=300)
        env = os.environ.copy()
        env.update({"FACTORY_API_TOKEN": ids["api"], "FACTORY_WORKER_TOKEN": ids["worker"],
                    "FACTORY_VERIFIER_TOKEN": ids["verifier"], "FACTORY_REVIEWER_TOKEN": ids["reviewer"]})
        base = f"http://127.0.0.1:{port}"
        processes.append(subprocess.Popen([str(bins["core"]), "--addr", f"127.0.0.1:{port}",
            "--db", str(root / "factory.db"), "--verifier-bin", str(bins["verifier"]),
            "--reviewer-bin", str(bins["reviewer"]), "--lead-paid-project-id", PROJECT,
            "--lead-paid-base-revision", baseline, "--lead-paid-provider", PROVIDER,
            "--lead-paid-model", MODEL, "--lead-paid-paseo-mode", MODE,
            "--lead-paid-max-sessions", str(SESSIONS),
            "--lead-paid-authorization-id", AUTHORIZATION_ID],
            cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT))
        api = API(base, ids["api"])
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                if api.call("GET", "/api/v1/health", timeout=2)[0] == 200:
                    break
            except (OSError, TimeoutError):
                time.sleep(.2)
        else:
            raise RuntimeError("Core health timeout")
        worker_id = "s4d-" + secrets.token_hex(6)
        processes.append(subprocess.Popen([str(bins["worker"]), "--core", base, "--worker-id", worker_id,
            "--exec-mode", "local", "--max-concurrent", "1", "--recovery-interval", "0"],
            cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT))
        deadline = time.time() + 30
        while time.time() < deadline:
            status, payload = api.call("GET", "/api/v1/workers")
            if status == 200 and any(w.get("id") == worker_id for w in payload.get("workers", [])):
                break
            time.sleep(.25)
        else:
            raise RuntimeError("Worker did not register")

        status, payload = api.call("POST", "/api/v1/projects", {
            "id": PROJECT, "name": "Disposable S4-D Lead fixture", "repositoryPath": str(repo),
            "baseRefs": ["main"], "qualityProfile": "node-basic", "workspaceScope": ".",
            "capabilities": ["repository-edit", "node-toolchain"], "executionMode": "trusted-local"})
        check(status in (200, 201), f"project registration failed: {payload}")
        project = payload["project"]
        expires = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() + 3600))
        status, payload = api.call("POST", "/api/v1/delegations", {
            "projectId": PROJECT, "revision": 1, "objectiveClasses": ["feature"],
            "allowedPaths": PATHS, "capabilities": ["repository-edit", "node-toolchain"],
            "riskClasses": ["R0"], "provider": PROVIDER, "model": MODEL, "paseoMode": MODE,
            "maxAttempts": ATTEMPTS, "maxTimeoutSecs": 120, "maxRepairCycles": REPAIRS,
            "maxWallClockSecs": 1200, "maxParallel": PARALLEL, "maxSessions": SESSIONS,
            "maxRequests": 1, "maxCumulativeSessions": SESSIONS,
            "maxCumulativeAttempts": ATTEMPTS, "autonomousStart": False,
            "autonomousRiskClasses": [], "expiresAt": expires}, idem=AUTHORIZATION_ID)
        check(status in (200, 201), f"grant issue failed: {payload}")
        grant = payload["delegation"]
        check(grant.get("projectId") == PROJECT and grant.get("provider") == PROVIDER
              and grant.get("model") == MODEL and grant.get("paseoMode") == MODE
              and grant.get("allowedPaths") == PATHS and grant.get("maxSessions") == SESSIONS
              and grant.get("maxAttempts") == ATTEMPTS and grant.get("maxRepairCycles") == REPAIRS
              and grant.get("maxParallel") == PARALLEL
              and grant.get("maxCumulativeSessions") == SESSIONS
              and grant.get("maxRequests") == 1,
              "Core grant does not match the exact authorized bounds")
        evidence["grant"] = {k: grant.get(k) for k in ("id", "revision", "contentHash", "expiresAt", "maxSessions")}

        status, payload = api.call("POST", "/api/v1/lead/requests",
            {"projectId": PROJECT, "objective": OBJECTIVE, "delegationId": grant["id"]},
            timeout=360, idem="s4d-live-request")
        check(status in (200, 201), f"Lead submission/assessment failed: {payload}")
        req = payload["request"]
        request_id = req["id"]
        evidence["requestId"] = request_id
        check(req.get("status") == "ready" and req.get("authorizationState") == "permitted",
              "Lead request is not ready/permitted")
        status, sp = api.call("GET", f"/api/v1/action-specs/{req['actionSpecId']}")
        check(status == 200, "frozen ActionSpec unavailable")
        spec = sp.get("spec", sp)
        status, cp = api.call("GET", f"/api/v1/campaigns/{req['campaignId']}")
        check(status == 200, "pending Campaign unavailable")
        campaign = cp.get("campaign", cp)
        plan = req.get("plan") or {}
        units = plan.get("workUnits") or []
        criteria = plan.get("acceptanceCriteria") or []
        negatives = plan.get("negativeCases") or []
        expected_ids = {"format", "test", "functional"}
        check_ids = {c.get("id") for c in criteria}
        negative_ids = {c.get("id") for c in negatives}
        check(plan.get("projectId") == PROJECT and plan.get("baseRevision") == baseline
              and plan.get("projectConfigHash") == project.get("configHash")
              and plan.get("provider") == PROVIDER and plan.get("model") == MODEL
              and plan.get("paseoMode") == MODE and plan.get("riskClass") == "R0"
              and plan.get("maxSessions") == SESSIONS and plan.get("maxRepairCycles") == 0
              and plan.get("maxParallel") == PARALLEL
              and (plan.get("budget") or {}).get("maxAttempts") == ATTEMPTS
              and len(units) == 1 and units[0].get("allowedPaths") == PATHS
              and set(units[0].get("capabilities") or []) == {"repository-edit", "node-toolchain"}
              and check_ids == expected_ids and negative_ids == {"negative"},
              "proposed plan exceeds project/route/path/session constraints")
        check(spec.get("projectId") == PROJECT and spec.get("baseRevision") == baseline
              and spec.get("allowedPaths") == PATHS and spec.get("provider") == PROVIDER
              and spec.get("model") == MODEL and spec.get("paseoMode") == MODE
              and spec.get("qualityProfile") == "node-basic" and campaign.get("status") == "pending"
              and campaign.get("specId") == spec.get("id"), "frozen spec or pending Campaign mismatch")
        evidence.update({"actionId": req.get("actionId"), "actionSpecId": spec.get("id"),
                         "specHash": spec.get("contentHash"), "campaignId": campaign.get("id"),
                         "planHash": req.get("planHash"), "planReviewedBeforeStart": True})
        status, payload = api.call("POST", f"/api/v1/lead/requests/{request_id}/start", {})
        check(status == 200, f"explicit Lead start failed: {payload}")

        deadline = time.time() + 1250
        projection = {}
        while time.time() < deadline:
            status, projection = api.call("GET", f"/api/v1/campaigns/{campaign['id']}")
            check(status == 200, "Campaign lookup failed")
            state = projection.get("campaign", {})
            if int((projection.get("budget") or {}).get("attemptsUsed") or 0) > ATTEMPTS:
                api.call("POST", f"/api/v1/campaigns/{campaign['id']}/cancel", {})
                raise RuntimeError("attempt ceiling exceeded")
            if state.get("status") in TERMINAL:
                break
            time.sleep(1)
        else:
            api.call("POST", f"/api/v1/campaigns/{campaign['id']}/cancel", {})
            raise RuntimeError("Campaign timeout")
        tasks = projection.get("tasks", [])
        runs = list({t.get("runId") for t in tasks if str(t.get("key", "")).startswith("implementer-") and t.get("runId")})
        check(len(runs) == 1, f"expected one Implementer Run, got {len(runs)}")
        run_id = runs[0]
        status, rp = api.call("GET", f"/api/v1/runs/{run_id}")
        check(status == 200, "Run unavailable")
        status, ap = api.call("GET", f"/api/v1/runs/{run_id}/attempts")
        check(status == 200 and len(ap.get("attempts", [])) == 1, "expected one Attempt")
        attempt = ap["attempts"][0]
        candidate = projection.get("candidate") or {}
        candidate_id, candidate_sha = candidate.get("id"), candidate.get("commitSha")
        check(candidate_id and candidate_sha and cmd(["git", "-C", str(repo), "cat-file", "-e",
            str(candidate_sha) + "^{commit}"], check=False).returncode == 0, "frozen candidate missing from fixture")
        status, ep = api.call("GET", f"/api/v1/candidates/{candidate_id}/evidence")
        check(status == 200, "candidate evidence unavailable")
        bundle = ep.get("bundle") or {}
        decision = projection.get("decision") or {}
        verifies = [v for v in bundle.get("verifications", []) if v.get("candidateId") == candidate_id
                    and v.get("candidateSha") == candidate_sha and v.get("specId") == spec.get("id")
                    and v.get("status") == "PASS"
                    and {c.get("id") for c in v.get("checks", [])} >= {"format", "test", "functional", "negative"}
                    and all(c.get("status") == "PASS" for c in v.get("checks", []))]
        reviews = [r for r in bundle.get("reviews", []) if r.get("candidateId") == candidate_id
                   and r.get("candidateSha") == candidate_sha and r.get("specId") == spec.get("id")
                   and r.get("verdict") == "APPROVE"]
        check(state.get("status") == "accepted" and decision.get("status") == "ACCEPTED"
              and decision.get("candidateSha") == candidate_sha and verifies and reviews
              and candidate.get("state") == "frozen" and set(candidate.get("changedPaths", [])) == set(PATHS),
              "candidate lacks exact PASS/APPROVE/ACCEPTED evidence or changed paths")
        check((rp.get("run") or {}).get("status") == "completed" and attempt.get("status") == "succeeded",
              "Run or Attempt did not succeed")
        status, sessions_payload = api.call("GET", f"/api/v1/runs/{run_id}/sessions")
        check(status == 200, "Run session binding unavailable")
        for binding in sessions_payload.get("sessions", []):
            sid = binding.get("sessionId") or binding.get("SessionID")
            if sid:
                sessions.add(str(sid))
        observation = req.get("modelObservation") or {}
        check(observation.get("provider") == PROVIDER and observation.get("model") == MODEL
              and observation.get("mode") == MODE and observation.get("sessionId"),
              "assessment lacks exact route/session observation")
        sessions.add(str(observation["sessionId"]))
        check(len(sessions) == SESSIONS, f"expected exactly {SESSIONS} provider sessions, observed {len(sessions)}")
        observed_routes = []
        for sid in sorted(sessions):
            info = json.loads(cmd(["paseo", "inspect", sid, "--json"]).stdout)
            info = info.get("session", info)
            check(info.get("Provider") == PROVIDER and info.get("Model") == MODEL
                  and info.get("Mode") == MODE, f"Paseo session {sid} route mismatch")
            observed_routes.append({"sessionId": sid, "provider": PROVIDER, "model": MODEL, "mode": MODE})
        evidence.update({"campaignStatus": state.get("status"), "runId": run_id,
            "attemptId": attempt.get("id"), "candidate": {k: candidate.get(k) for k in
            ("id", "commitSha", "treeSha", "contentDigest", "changedPaths")},
            "verifier": [{"status": v.get("status"), "evidenceId": v.get("evidenceId")} for v in verifies],
            "reviewer": [{"verdict": r.get("verdict"), "id": r.get("id")} for r in reviews],
            "gate": {k: decision.get(k) for k in ("status", "candidateId", "candidateSha", "specId")},
            "providerSessionIds": sorted(sessions), "providerSessions": observed_routes,
            "status": "ACCEPTED_CANDIDATE_PENDING_OWNER_REVIEW"})
        return 0
    finally:
        # Only stop/archive session identities bound to this Lead request and Run.
        cleanup_sessions = []
        for sid in sorted(sessions):
            stopped = cmd(["paseo", "stop", sid], check=False)
            archived = cmd(["paseo", "archive", sid], check=False)
            cleanup_sessions.append({"sessionId": sid, "stopExit": stopped.returncode,
                                    "archiveExit": archived.returncode})
        for proc in processes:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=5)
        log.close()
        if request_id:
            evidence["requestId"] = request_id
        if run_id:
            evidence["runId"] = run_id
        evidence["providerSessionIds"] = sorted(sessions)
        shutil.rmtree(repo, ignore_errors=True)
        evidence["cleanup"] = {"servicesStopped": all(p.poll() is not None for p in processes),
                               "fixtureRemoved": not repo.exists(), "sessions": cleanup_sessions,
                               "sessionsAttempted": len(sessions)}
        evidence["status"] = evidence.get("status", "INCOMPLETE")
        (root / "live-evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
        print(f"evidence: {root / 'live-evidence.json'}")


if __name__ == "__main__":
    if os.environ.get("FACTORY_S4D_LIVE") != "1":
        raise SystemExit(safe_preflight())
    try:
        raise SystemExit(main_live())
    except (RuntimeError, OSError, subprocess.SubprocessError, KeyError, ValueError) as exc:
        print(json.dumps({"status": "BLOCKED_OR_INCOMPLETE", "reason": str(exc)}, indent=2), file=sys.stderr)
        raise SystemExit(2)
