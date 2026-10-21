#!/usr/bin/env python3
"""F1-S4-B native campaign orchestration acceptance oracle.

Real Core (two concurrent scheduler instances for B10), real Worker,
deterministic Paseo CLI simulator, Core-spawned independent verifier and
reviewer, real Chromium, automatic repair and crash/restart recovery.
No provider calls and no paid sessions.
"""
import json
import os
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "scripts" / "fixtures" / "s4a-ui"

BASE = sys.argv[1]
BASE2 = sys.argv[2]
API_TOKEN = sys.argv[3]
WORKER_TOKEN = sys.argv[4]
VERIFIER_TOKEN = sys.argv[5]
REVIEWER_TOKEN = sys.argv[6]
CORE_BIN = sys.argv[7]
WORKER_BIN = sys.argv[8]
VERIFIER_BIN = sys.argv[9]
REVIEWER_BIN = sys.argv[10]
WORK_ROOT = Path(sys.argv[11])
CHROMIUM_EXE = sys.argv[12]
PLAYWRIGHT_CORE = sys.argv[13]
PORT = sys.argv[14]
PORT2 = sys.argv[15]
WEB_DIR = sys.argv[16]

DB = str(WORK_ROOT / "factory.db")
STUB_DIR = WORK_ROOT / "stub"
STUB_BIN = WORK_ROOT / "stubbin" / "paseo"
EVIDENCE = WORK_ROOT / "evidence"
CORE_LOG = WORK_ROOT / "core.log"
CORE2_LOG = WORK_ROOT / "core2.log"
WORKER_LOG = WORK_ROOT / "worker.log"
FAILURES = []
TERMINAL = {"accepted", "repair_required", "blocked", "human_required", "failed", "cancelled"}


def check(name, cond, detail=""):
    print(f"[{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))
    if not cond:
        FAILURES.append(name)
    return cond


def call(method, path, body=None, token=None, base=None, expect=None):
    req = urllib.request.Request(
        (base or BASE) + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={
            "Authorization": f"Bearer {token or API_TOKEN}",
            "Content-Type": "application/json",
            "X-Correlation-ID": f"s4b-oracle-{time.time_ns()}",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as res:
            payload = json.loads(res.read().decode() or "{}")
            headers = {k.lower(): v for k, v in res.headers.items()}
            return res.status, payload, headers
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            payload = json.loads(raw or "{}")
        except json.JSONDecodeError:
            payload = {"raw": raw}
        return e.code, payload, {k.lower(): v for k, v in e.headers.items()}


def wait_health(base, timeout=30):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            status, _, _ = call("GET", "/api/v1/health", base=base)
            if status == 200:
                return True
        except Exception:
            pass
        time.sleep(0.2)
    return False


class Processes:
    def __init__(self):
        self.core = None
        self.core2 = None
        self.worker = None
        self.recovery = None

    def core_env(self):
        env = os.environ.copy()
        env.update({
            "FACTORY_API_TOKEN": API_TOKEN,
            "FACTORY_WORKER_TOKEN": WORKER_TOKEN,
            "FACTORY_VERIFIER_TOKEN": VERIFIER_TOKEN,
            "FACTORY_REVIEWER_TOKEN": REVIEWER_TOKEN,
            "CHROMIUM_EXE": CHROMIUM_EXE,
            "FACTORY_PLAYWRIGHT_CORE": PLAYWRIGHT_CORE,
        })
        return env

    def start_core(self, second=False):
        port = PORT2 if second else PORT
        log_path = CORE2_LOG if second else CORE_LOG
        log = open(log_path, "ab")
        proc = subprocess.Popen(
            [CORE_BIN, "--addr", f"127.0.0.1:{port}", "--db", DB,
             "--web-dir", WEB_DIR, "--verifier-bin", VERIFIER_BIN,
             "--reviewer-bin", REVIEWER_BIN, "--campaign-interval", "300ms"],
            stdout=log, stderr=log, env=self.core_env(),
        )
        base = BASE2 if second else BASE
        if not wait_health(base):
            proc.kill()
            raise RuntimeError(f"Core on port {port} did not become ready")
        if second:
            self.core2 = proc
        else:
            self.core = proc

    def stop_core(self, second=False, crash=False):
        proc = self.core2 if second else self.core
        if proc and proc.poll() is None:
            if crash:
                proc.send_signal(signal.SIGKILL)
            else:
                proc.send_signal(signal.SIGTERM)
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)
        if second:
            self.core2 = None
        else:
            self.core = None

    def start_worker(self):
        env = os.environ.copy()
        env.update({
            "FACTORY_WORKER_TOKEN": WORKER_TOKEN,
            "PASEO_BIN": str(STUB_BIN),
            "S4A_STUB_DIR": str(STUB_DIR),
            "FACTORY_BROWSER_CLI": str(ROOT / "office" / "e2e-browser" / "node_modules" / ".bin" / "playwright-cli"),
            "CHROMIUM_EXE": CHROMIUM_EXE,
            "FACTORY_PLAYWRIGHT_CORE": PLAYWRIGHT_CORE,
        })
        log = open(WORKER_LOG, "ab")
        self.worker = subprocess.Popen(
            [WORKER_BIN, "--core", BASE, "--worker-id", "local-1",
             "--exec-mode", "local", "--recovery-interval", "0",
             "--poll-interval", "200ms", "--heartbeat-interval", "1s"],
            stdout=log, stderr=log, env=env,
        )
        time.sleep(0.8)
        if self.worker.poll() is not None:
            raise RuntimeError("Worker failed to start; see worker.log")

    def start_recovery_worker(self):
        env = os.environ.copy()
        env.update({
            "FACTORY_WORKER_TOKEN": WORKER_TOKEN,
            "PASEO_BIN": str(STUB_BIN),
            "S4A_STUB_DIR": str(STUB_DIR),
        })
        log = open(WORK_ROOT / "recovery.log", "ab")
        self.recovery = subprocess.Popen(
            [WORKER_BIN, "--core", BASE, "--worker-id", "recovery-1",
             "--exec-mode", "local", "--recovery-only",
             "--recovery-interval", "300ms"],
            stdout=log, stderr=log, env=env,
        )
        time.sleep(0.5)
        if self.recovery.poll() is not None:
            raise RuntimeError("recovery Worker failed to start")

    def stop_recovery_worker(self):
        if self.recovery and self.recovery.poll() is None:
            self.recovery.send_signal(signal.SIGTERM)
            try:
                self.recovery.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.recovery.kill()
                self.recovery.wait(timeout=5)
        self.recovery = None

    def stop_worker(self):
        self.stop_recovery_worker()
        if self.worker and self.worker.poll() is None:
            self.worker.send_signal(signal.SIGTERM)
            try:
                self.worker.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.worker.kill()
                self.worker.wait(timeout=5)
        self.worker = None

    def close(self):
        self.stop_worker()
        self.stop_core(second=True)
        self.stop_core()


def git(args, cwd):
    return subprocess.run(["git", "-C", str(cwd)] + args, check=True,
                          capture_output=True, text=True).stdout.strip()


def create_fixture_repo():
    src = WORK_ROOT / "fixture-src"
    bare = WORK_ROOT / "repo.git"
    shutil.copytree(FIXTURE, src)
    git(["init", "-q"], src)
    git(["-c", "user.name=fixture", "-c", "user.email=fixture@factory.local", "add", "."], src)
    git(["-c", "user.name=fixture", "-c", "user.email=fixture@factory.local",
         "commit", "-qm", "fixture baseline"], src)
    subprocess.run(["git", "clone", "--quiet", "--bare", str(src), str(bare)], check=True)
    return bare, git(["rev-parse", "HEAD"], bare)


def install_stub():
    STUB_DIR.mkdir(parents=True, exist_ok=True)
    stubbin = STUB_BIN.parent
    stubbin.mkdir(parents=True, exist_ok=True)
    shutil.copy(ROOT / "scripts" / "s4a_paseo_stub.py", stubbin / "s4a_paseo_stub.py")
    STUB_BIN.write_text(
        "#!/bin/sh\nexport S4A_STUB_DIR=\"%s\"\nexec python3 \"%s\" \"$@\"\n"
        % (STUB_DIR, stubbin / "s4a_paseo_stub.py"),
        encoding="utf-8",
    )
    STUB_BIN.chmod(0o755)


def set_mode(mode):
    (STUB_DIR / "sequence").unlink(missing_ok=True)
    (STUB_DIR / "delay").unlink(missing_ok=True)
    (STUB_DIR / "mode").write_text(mode, encoding="utf-8")


def set_sequence(modes):
    (STUB_DIR / "delay").unlink(missing_ok=True)
    (STUB_DIR / "sequence").write_text("\n".join(modes) + "\n", encoding="utf-8")


def set_delay(seconds):
    (STUB_DIR / "delay").write_text(str(seconds), encoding="utf-8")


def clear_delay():
    (STUB_DIR / "delay").unlink(missing_ok=True)


def create_work_item(title, repo, base, profile="node-basic", allowed=None, goal=None):
    status, body, _ = call("POST", "/api/v1/actions", {"title": title})
    if status != 201:
        raise RuntimeError(f"create action: {status} {body}")
    action = body["action"]
    allowed = allowed or ["add.js", "app.js"]
    if profile == "node-browser":
        acceptance = [
            {"id": "format", "kind": "format", "description": "syntax",
             "command": ["node", "--check", "add.js"], "expectExit": 0, "timeoutSecs": 30},
            {"id": "test", "kind": "test", "description": "unit tests",
             "command": ["node", "--test", "add.test.js"], "expectExit": 0, "timeoutSecs": 60},
            {"id": "functional", "kind": "functional", "description": "multiply works",
             "command": ["node", "-e",
                         "const {multiply}=require('./add.js'); if(multiply(2,3)!==6) process.exit(1);"],
             "expectExit": 0, "timeoutSecs": 30},
            {"id": "browser", "kind": "browser", "description": "UI multiply journey",
             "command": ["node", "browser-check.mjs"], "expectExit": 0, "timeoutSecs": 120},
        ]
        negative = [
            {"id": "negative", "kind": "negative", "description": "add regression",
             "command": ["node", "-e",
                         "const {add}=require('./add.js'); if(add(2,3)!==5) process.exit(1);"],
             "expectExit": 0, "timeoutSecs": 30},
        ]
    else:
        acceptance = [
            {"id": "format", "kind": "format", "description": "syntax",
             "command": ["node", "--check", "add.js"], "expectExit": 0, "timeoutSecs": 30},
            {"id": "test", "kind": "test", "description": "unit tests",
             "command": ["node", "--test", "add.test.js"], "expectExit": 0, "timeoutSecs": 60},
            {"id": "functional", "kind": "functional", "description": "multiply works",
             "command": ["node", "-e",
                         "const {multiply}=require('./add.js'); if(multiply(2,3)!==6) process.exit(1);"],
             "expectExit": 0, "timeoutSecs": 30},
        ]
        negative = [
            {"id": "negative", "kind": "negative", "description": "invalid input",
             "command": ["node", "-e", "if(1+1===3) process.exit(1);"],
             "expectExit": 0, "timeoutSecs": 30},
        ]
    spec_body = {
        "actionId": action["id"],
        "goal": goal or ("Append a correct multiply(a,b) export to add.js and wire the "
                         "operation selector in app.js so multiply computes a*b."),
        "repository": str(repo), "baseRevision": base, "allowedPaths": allowed,
        "acceptanceCriteria": acceptance, "negativeCases": negative,
        "riskClass": "R0", "qualityProfile": profile,
        "budget": {"maxAttempts": 2, "timeoutSecs": 600},
        "executionMode": "trusted-local", "provider": "opencode",
        "model": "opencode-go/deepseek-v4.1-flash", "paseoMode": "build",
    }
    status, body, _ = call("POST", "/api/v1/action-specs", spec_body)
    if status != 201:
        raise RuntimeError(f"create spec: {status} {body}")
    return action, body["spec"]


def create_campaign(spec_id, repairs=1):
    status, body, headers = call("POST", "/api/v1/campaigns", {
        "specId": spec_id, "maxRepairCycles": repairs,
        "maxWallClockSecs": 600, "maxParallel": 2,
    }, token=API_TOKEN)
    if status not in (200, 201):
        raise RuntimeError(f"create campaign: {status} {body}")
    return body, headers


def start_campaign(campaign_id):
    status, body, headers = call("POST", f"/api/v1/campaigns/{campaign_id}/start", {})
    if status != 200:
        raise RuntimeError(f"start campaign: {status} {body}")
    return body, headers


def campaign(campaign_id):
    status, body, _ = call("GET", f"/api/v1/campaigns/{campaign_id}")
    if status != 200:
        raise RuntimeError(f"get campaign: {status} {body}")
    return body


def wait_campaign(campaign_id, predicate, timeout=240, label="campaign"):
    deadline = time.time() + timeout
    latest = None
    while time.time() < deadline:
        latest = campaign(campaign_id)
        if predicate(latest):
            return latest
        if latest["campaign"]["status"] in TERMINAL:
            return latest
        time.sleep(0.4)
    raise RuntimeError(f"{label} {campaign_id} did not reach condition; last {latest and latest['campaign']['status']}")


def task_by_key(projection, key):
    return next(task for task in projection["tasks"] if task["key"] == key)


def global_events():
    return call("GET", "/api/v1/events?after=0&limit=500")[1]["events"]


def campaign_events(campaign_id):
    return call("GET", f"/api/v1/campaigns/{campaign_id}/events")[1]["events"]


def candidate_bundle(candidate_id):
    status, body, _ = call("GET", f"/api/v1/candidates/{candidate_id}/evidence")
    if status != 200:
        raise RuntimeError(f"candidate bundle: {status} {body}")
    return body["bundle"]


def main():
    WORK_ROOT.mkdir(parents=True, exist_ok=True)
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    install_stub()
    set_mode("good")
    repo, base = create_fixture_repo()
    procs = Processes()
    summary = {}
    try:
        procs.start_core()
        procs.start_worker()
        check("Core and Worker are healthy", True)

        # B1/B2/B3/B4/B5/B6: immutable plan, native dispatch, candidate-
        # dependent verification, independent evidence, one ACCEPTED gate.
        action, spec = create_work_item("B1-B6 accepted campaign", repo, base,
                                        profile="node-browser")
        created, _ = create_campaign(spec["id"], repairs=1)
        projection = created
        campaign_id = projection["campaign"]["id"]
        summary["acceptedCampaign"] = campaign_id
        check("B1 campaign binds the exact frozen ActionSpec",
              projection["campaign"]["specId"] == spec["id"] and
              projection["campaign"]["specHash"] == spec["contentHash"] and
              projection["campaign"]["actionId"] == action["id"])
        check("B1 immutable plan carries 4 role tasks per bounded cycle",
              len(projection["campaign"]["plan"]["tasks"]) == 8 and
              projection["campaign"]["planHash"] != "")
        check("B1 plan hash is stable and task dependencies are explicit",
              projection["campaign"]["planHash"] and
              task_by_key(projection, "verifier-0")["dependsOn"] == ["implementer-0"] and
              set(task_by_key(projection, "gate-0")["dependsOn"]) == {"verifier-0", "reviewer-0"})
        duplicate, _ = create_campaign(spec["id"], repairs=1)
        check("B1 duplicate campaign create is idempotent",
              duplicate["campaign"]["id"] == campaign_id)
        escalation_status, _, _ = call("POST", "/api/v1/campaigns", {
            "specId": spec["id"], "maxRepairCycles": 9,
        })
        check("B1 plan/budget escalation is rejected", escalation_status == 400,
              str(escalation_status))

        set_delay(2)
        start_campaign(campaign_id)
        early = wait_campaign(campaign_id, lambda p: task_by_key(p, "implementer-0")["status"] in
                              ("dispatched", "succeeded"), timeout=30)
        impl_task = task_by_key(early, "implementer-0")
        check("B2 one authorized start dispatches the native Implementer",
              impl_task["status"] in ("dispatched", "succeeded") and impl_task["runId"])
        check("B3 verifier cannot run before the frozen candidate exists",
              task_by_key(early, "verifier-0")["status"] == "pending" and
              len(call("GET", f"/api/v1/candidates?actionId={action['id']}")[1]["candidates"]) == 0)
        clear_delay()
        final = wait_campaign(campaign_id, lambda p: p["campaign"]["status"] == "accepted")
        check("B6 campaign reaches ACCEPTED without manual role invocation",
              final["campaign"]["status"] == "accepted", final["campaign"]["status"])
        check("B2/B6 all mandatory tasks succeeded",
              all(task_by_key(final, key)["status"] == "succeeded" for key in
                  ("implementer-0", "verifier-0", "reviewer-0", "gate-0")))
        candidate = final["candidate"]
        check("B3 verification consumed the post-Run frozen candidate",
              candidate and candidate["state"] == "frozen" and
              task_by_key(final, "verifier-0")["candidateId"] == candidate["id"] and
              task_by_key(final, "gate-0")["candidateId"] == candidate["id"])
        bundle = candidate_bundle(candidate["id"])
        verifier = bundle["verifications"][0]
        reviewer = bundle["reviews"][0]
        check("B4 Factory scheduled the separate verifier with scoped authority",
              verifier["authority"] == "verifier" and
              verifier["actor"] == task_by_key(final, "verifier-0")["participantId"] and
              verifier["actor"] != candidate["implementerWorkerId"] and
              verifier["cleanResource"].startswith("git-clone:") and
              verifier["candidateSha"] == candidate["commitSha"])
        check("B5 Factory scheduled the separate reviewer and bound its verdict",
              reviewer["authority"] == "reviewer" and
              reviewer["actor"] == task_by_key(final, "reviewer-0")["participantId"] and
              reviewer["verdict"] == "APPROVE" and
              reviewer["candidateSha"] == candidate["commitSha"])
        check("B6 exactly one Core-authoritative ACCEPTED decision",
              final["decision"]["status"] == "ACCEPTED" and
              final["decision"]["candidateId"] == candidate["id"])
        gate_events = [e for e in global_events() if e["eventType"] == "CampaignFinished" and
                       e["aggregateId"] == campaign_id]
        check("B6 exactly one campaign terminal event exists", len(gate_events) == 1,
              str(len(gate_events)))
        check("B1 resource accounting exposes bounded known/unknown values",
              final["budget"]["providerBudget"] == "unknown" and
              final["budget"]["repairCycles"] == 0 and
              final["budget"]["maxRepairCycles"] == 1 and
              final["budget"]["attemptsUsed"] >= 1)

        # B10: a second concurrent scheduler instance must not duplicate work.
        procs.start_core(second=True)
        check("B10 second Core scheduler instance is healthy", wait_health(BASE2))

        # B7: bounded automatic repair with a new candidate identity.
        repair_action, repair_spec = create_work_item("B7 automatic repair", repo, base)
        set_sequence(["failing", "good"])
        repair_created, _ = create_campaign(repair_spec["id"], repairs=1)
        repair_id = repair_created["campaign"]["id"]
        summary["repairCampaign"] = repair_id
        start_campaign(repair_id)
        repaired = wait_campaign(repair_id, lambda p: p["campaign"]["status"] == "accepted")
        check("B7 repair campaign is accepted after one bounded cycle",
              repaired["campaign"]["status"] == "accepted" and
              repaired["campaign"]["repairCycles"] == 1,
              f"{repaired['campaign']['status']} repairs={repaired['campaign']['repairCycles']}")
        candidates = call("GET", f"/api/v1/candidates?actionId={repair_action['id']}")[1]["candidates"]
        check("B7 repair created a new immutable CandidateRevision",
              len(candidates) == 2 and candidates[0]["id"] != candidates[1]["id"] and
              candidates[0]["commitSha"] != candidates[1]["commitSha"])
        latest = candidates[0] if candidates[0]["createdAt"] > candidates[1]["createdAt"] else candidates[1]
        prior = candidates[1] if latest is candidates[0] else candidates[0]
        check("B7 repaired candidate retains lineage", latest["parentCandidateId"] == prior["id"])
        check("B7 prior candidate evidence cannot validate the repair",
              prior["state"] == "superseded" and
              candidate_bundle(prior["id"])["decision"]["status"] == "REPAIR_REQUIRED" and
              candidate_bundle(latest["id"])["decision"]["status"] == "ACCEPTED")
        dispatch_counts = {}
        for event in campaign_events(repair_id):
            if event["eventType"] == "CampaignTaskDispatched":
                key = event["payload"].get("taskKey", "")
                dispatch_counts[key] = dispatch_counts.get(key, 0) + 1
        check("B10 concurrent schedulers do not duplicate task dispatch",
              all(count == 1 for count in dispatch_counts.values()) and
              dispatch_counts.get("verifier-0") == 1 and
              dispatch_counts.get("reviewer-0") == 1,
              json.dumps(dispatch_counts))

        # B8: HUMAN_REQUIRED cannot silently become a repair authorization.
        human_action, human_spec = create_work_item(
            "B8 protected diff", repo, base, allowed=["core/internal/quality/policy.go"],
            goal="Simulate a protected authority change for the human gate.")
        set_mode("critical")
        human_created, _ = create_campaign(human_spec["id"], repairs=2)
        human_id = human_created["campaign"]["id"]
        summary["humanCampaign"] = human_id
        start_campaign(human_id)
        human = wait_campaign(human_id, lambda p: p["campaign"]["status"] == "human_required")
        check("B8 under-classified protected diff requires a human",
              human["campaign"]["status"] == "human_required", human["campaign"]["status"])
        check("B8 HUMAN_REQUIRED does not authorize repair",
              human["campaign"]["repairCycles"] == 0 and
              task_by_key(human, "implementer-1")["status"] in ("cancelled", "pending") and
              task_by_key(human, "implementer-1")["status"] != "succeeded")
        check("B8 HUMAN_REQUIRED records no repair authorization event",
              not any(e["eventType"] == "CampaignRepairAuthorized" for e in campaign_events(human_id)))
        set_mode("good")

        # B9: Core crash during a live Implementer phase; restart must
        # preserve the graph and resume without duplicate authoritative effects.
        procs.stop_core(second=True)
        crash_action, crash_spec = create_work_item("B9 crash recovery", repo, base)
        set_delay(4)
        crash_created, _ = create_campaign(crash_spec["id"], repairs=1)
        crash_id = crash_created["campaign"]["id"]
        summary["crashCampaign"] = crash_id
        start_campaign(crash_id)
        running = wait_campaign(crash_id, lambda p: task_by_key(p, "implementer-0").get("runId", "") != "",
                                timeout=30)
        run_id = task_by_key(running, "implementer-0")["runId"]
        deadline = time.time() + 30
        run_status = ""
        while time.time() < deadline:
            code, body, _ = call("GET", f"/api/v1/runs/{run_id}")
            if code == 200:
                run_status = body["run"]["status"]
                if run_status in ("leased", "running"):
                    break
            time.sleep(0.2)
        check("B9 crash occurs during a live Implementer phase", run_status in ("leased", "running"),
              run_status)
        procs.stop_core(crash=True)
        procs.start_core()
        clear_delay()
        procs.start_recovery_worker()
        recovered = wait_campaign(crash_id, lambda p: p["campaign"]["status"] == "accepted", timeout=300)
        procs.stop_recovery_worker()
        check("B9 restart resumes the same campaign and reaches ACCEPTED",
              recovered["campaign"]["status"] == "accepted", recovered["campaign"]["status"])
        check("B9 recovery preserves one campaign terminal event",
              len([e for e in campaign_events(crash_id) if e["eventType"] == "CampaignFinished"]) == 1)
        crash_candidates = call("GET", f"/api/v1/candidates?actionId={crash_action['id']}")[1]["candidates"]
        check("B9 recovery does not fabricate duplicate candidates",
              len(crash_candidates) == 1, str(len(crash_candidates)))
        crash_bundle = candidate_bundle(crash_candidates[0]["id"])
        check("B9 recovered candidate has one authoritative decision",
              crash_bundle["decision"]["status"] == "ACCEPTED")

        # B11: a browser user starts and follows a real campaign in Office.
        browser_action, browser_spec = create_work_item("B11 Office campaign journey", repo, base)
        set_mode("good")
        journey_env = os.environ.copy()
        journey_env.update({
            "BASE_URL": BASE, "FACTORY_API_TOKEN": API_TOKEN,
            "CHROMIUM_EXE": CHROMIUM_EXE,
            "S4B_EVIDENCE_DIR": str(EVIDENCE),
            "S4B_ACTION_ID": browser_action["id"],
            "S4B_SPEC_ID": browser_spec["id"],
        })
        journey = subprocess.run(
            ["node", str(ROOT / "office" / "e2e-browser" / "s4b-journey.mjs")],
            capture_output=True, text=True, timeout=300, env=journey_env,
        )
        check("B11 real Chromium starts a campaign in Office and follows its lifecycle",
              journey.returncode == 0, journey.stdout[-500:] + journey.stderr[-500:])
        if journey.returncode == 0:
            browser_evidence = json.loads((EVIDENCE / "s4b-browser-evidence.json").read_text())
            summary["browserCampaign"] = browser_evidence["campaignId"]
            check("B11 browser observed the real task graph and ACCEPTED outcome",
                  browser_evidence["accepted"] and browser_evidence["taskCount"] == 4)

        # B12: forged authority, mismatched identity, unauthorized transition.
        status, _, _ = call("POST", "/api/v1/campaigns", {"specId": spec["id"]}, token=WORKER_TOKEN)
        check("B12 Worker credential cannot create campaigns", status == 401, str(status))
        status, _, _ = call("POST", f"/api/v1/campaigns/{campaign_id}/start", {}, token=VERIFIER_TOKEN)
        check("B12 verifier credential cannot start campaigns", status == 401, str(status))
        status, _, _ = call("POST", f"/api/v1/campaigns/{campaign_id}/tasks/x/status", {"status": "succeeded"})
        check("B12 no operator route can forge a task transition", status == 404, str(status))
        forged = {
            "instanceId": "verifier-forged", "candidateId": crash_candidates[0]["id"],
            "candidateSha": "0" * len(crash_candidates[0]["commitSha"]),
            "specId": crash_spec["id"], "specHash": crash_spec["contentHash"],
            "policyRevision": crash_spec["policyRevision"],
            "oracleDigest": "", "cleanResource": "git-clone:forged",
            "checks": [], "findings": [], "idempotencyKey": "forged-s4b",
        }
        status, _, _ = call("POST", "/api/v1/verifier/evidence", forged, token=VERIFIER_TOKEN)
        check("B12 forged/mismatched verifier evidence is rejected",
              status in (400, 403, 409), str(status))
        after_forge = candidate_bundle(crash_candidates[0]["id"])
        check("B12 forged evidence did not alter the authoritative decision",
              after_forge["decision"]["status"] == "ACCEPTED" and
              len(after_forge["verifications"]) == len(crash_bundle["verifications"]))

        (WORK_ROOT / "summary.json").write_text(json.dumps(summary, indent=2))
    finally:
        procs.close()

    required = [
        "B1 campaign binds the exact frozen ActionSpec",
        "B2 one authorized start dispatches the native Implementer",
        "B3 verifier cannot run before the frozen candidate exists",
        "B4 Factory scheduled the separate verifier with scoped authority",
        "B5 Factory scheduled the separate reviewer and bound its verdict",
        "B6 campaign reaches ACCEPTED without manual role invocation",
        "B7 repair campaign is accepted after one bounded cycle",
        "B8 HUMAN_REQUIRED does not authorize repair",
        "B9 restart resumes the same campaign and reaches ACCEPTED",
        "B10 concurrent schedulers do not duplicate task dispatch",
        "B11 real Chromium starts a campaign in Office and follows its lifecycle",
        "B12 forged/mismatched verifier evidence is rejected",
    ]
    missing = [name for name in required if name in FAILURES]
    if FAILURES:
        print(f"\n{len(FAILURES)} check(s) FAILED: {FAILURES}")
        return 1
    print("\nAll F1-S4-B deterministic acceptance checks passed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        import traceback
        print(f"[FAIL] oracle aborted: {e}")
        traceback.print_exc()
        sys.exit(1)
