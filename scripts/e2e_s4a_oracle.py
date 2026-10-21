#!/usr/bin/env python3
"""F1-S4-A deterministic quality-pipeline acceptance oracle.

Owns a real Core and Worker process against a disposable bare repository and
a deterministic Paseo CLI simulator. Proves frozen spec -> implementer ->
immutable candidate -> independent verifier (including real Chromium) ->
independent reviewer -> deterministic Quality Gate, plus negative,
independence, repair, replay and browser journeys. No daemon, no provider
calls, no paid sessions.
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
API_TOKEN = sys.argv[2]
WORKER_TOKEN = sys.argv[3]
VERIFIER_TOKEN = sys.argv[4]
REVIEWER_TOKEN = sys.argv[5]
CORE_BIN = sys.argv[6]
WORKER_BIN = sys.argv[7]
VERIFIER_BIN = sys.argv[8]
REVIEWER_BIN = sys.argv[9]
WORK_ROOT = Path(sys.argv[10])
CHROMIUM_EXE = sys.argv[11]
PLAYWRIGHT_CORE = sys.argv[12]
PORT = sys.argv[13]
WEB_DIR = sys.argv[14]

DB = str(WORK_ROOT / "factory.db")
STUB_DIR = WORK_ROOT / "stub"
STUB_BIN = WORK_ROOT / "stubbin" / "paseo"
EVIDENCE = WORK_ROOT / "evidence"
CORE_LOG = WORK_ROOT / "core.log"
WORKER_LOG = WORK_ROOT / "worker.log"
FAILURES = []


def check(name, cond, detail=""):
    print(f"[{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))
    if not cond:
        FAILURES.append(name)
    return cond


def call(method, path, body=None, token=None, expect=None):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={
            "Authorization": f"Bearer {token or API_TOKEN}",
            "Content-Type": "application/json",
            "X-Correlation-ID": f"s4a-oracle-{time.time_ns()}",
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


def wait_health(timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            status, _, _ = call("GET", "/api/v1/health")
            if status == 200:
                return True
        except Exception:
            pass
        time.sleep(0.2)
    return False


class Processes:
    def __init__(self):
        self.core = None
        self.worker = None

    def start_core(self):
        env = os.environ.copy()
        env.update({
            "FACTORY_API_TOKEN": API_TOKEN,
            "FACTORY_WORKER_TOKEN": WORKER_TOKEN,
            "FACTORY_VERIFIER_TOKEN": VERIFIER_TOKEN,
            "FACTORY_REVIEWER_TOKEN": REVIEWER_TOKEN,
        })
        log = open(CORE_LOG, "ab")
        self.core = subprocess.Popen(
            [CORE_BIN, "--addr", f"127.0.0.1:{PORT}", "--db", DB, "--web-dir", WEB_DIR],
            stdout=log, stderr=log, env=env,
        )
        if not wait_health():
            raise RuntimeError("Core did not become ready")

    def stop_core(self):
        if self.core and self.core.poll() is None:
            self.core.send_signal(signal.SIGTERM)
            try:
                self.core.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.core.kill()
                self.core.wait(timeout=5)
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
             "--exec-mode", "local", "--recovery-interval", "0"],
            stdout=log, stderr=log, env=env,
        )
        time.sleep(1)
        if self.worker.poll() is not None:
            raise RuntimeError("Worker failed to start; see worker.log")

    def stop_worker(self):
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
    base = git(["rev-parse", "HEAD"], bare)
    return bare, base


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
    set_mode("good")


def set_mode(mode):
    (STUB_DIR / "mode").write_text(mode, encoding="utf-8")


def queue_run(spec_id):
    status, body, _ = call("POST", "/api/v1/runs", {"specId": spec_id})
    if status != 201:
        raise RuntimeError(f"queue run failed: {status} {body}")
    return body["run"]["id"]


def wait_run(run_id, timeout=120):
    deadline = time.time() + timeout
    status = ""
    while time.time() < deadline:
        code, body, _ = call("GET", f"/api/v1/runs/{run_id}")
        if code == 200:
            status = body["run"]["status"]
            if status in ("succeeded", "failed", "cancelled"):
                return body["run"]
        time.sleep(1)
    raise RuntimeError(f"run {run_id} did not finish (last {status})")


def run_candidate(run_id):
    code, body, _ = call("GET", f"/api/v1/runs/{run_id}/candidate")
    if code != 200:
        return None
    return body["candidate"]


def run_verifier(candidate_id, expect_ok=True):
    env = {k: v for k, v in os.environ.items() if "TOKEN" not in k}
    env.update({
        "FACTORY_VERIFIER_TOKEN": VERIFIER_TOKEN,
        "CHROMIUM_EXE": CHROMIUM_EXE,
        "FACTORY_PLAYWRIGHT_CORE": PLAYWRIGHT_CORE,
    })
    result = subprocess.run(
        [VERIFIER_BIN, "--core", BASE, "--candidate", candidate_id,
         "--instance-id", "verifier-1"],
        capture_output=True, text=True, timeout=300, env=env,
    )
    if expect_ok and result.returncode != 0:
        raise RuntimeError(f"verifier failed: {result.stderr[-500:]}")
    return result


def run_reviewer(candidate_id, expect_ok=True):
    env = {k: v for k, v in os.environ.items() if "TOKEN" not in k}
    env["FACTORY_REVIEWER_TOKEN"] = REVIEWER_TOKEN
    result = subprocess.run(
        [REVIEWER_BIN, "--core", BASE, "--candidate", candidate_id,
         "--instance-id", "reviewer-1"],
        capture_output=True, text=True, timeout=120, env=env,
    )
    if expect_ok and result.returncode != 0:
        raise RuntimeError(f"reviewer failed: {result.stderr[-500:]}")
    return result


def bundle(candidate_id):
    code, body, _ = call("GET", f"/api/v1/candidates/{candidate_id}/evidence")
    if code != 200:
        raise RuntimeError(f"bundle {candidate_id}: {code} {body}")
    return body["bundle"]


def wait_decision(candidate_id, timeout=60):
    deadline = time.time() + timeout
    while time.time() < deadline:
        b = bundle(candidate_id)
        if b.get("decision"):
            return b["decision"]
        time.sleep(0.5)
    return None


def verify_candidate(candidate_id, expect_gate):
    run_verifier(candidate_id)
    run_reviewer(candidate_id)
    decision = wait_decision(candidate_id)
    if decision is None:
        raise RuntimeError(f"no gate decision for {candidate_id}")
    check(f"gate outcome is {expect_gate}", decision["status"] == expect_gate,
          decision["status"])
    return decision, bundle(candidate_id)


def check_evidence_provenance(candidate, b, label):
    verifications = b.get("verifications", [])
    check(f"{label}: independent verifier evidence exists",
          len(verifications) >= 1 and verifications[0]["authority"] == "verifier")
    if verifications:
        v = verifications[0]
        check(f"{label}: verifier actor is separate from implementer",
              v["actor"] != candidate["implementerWorkerId"], v["actor"])
        check(f"{label}: verifier ran on a clean resource",
              v["cleanResource"].startswith("git-clone:"), v["cleanResource"])
        check(f"{label}: oracle digest is candidate-bound",
              len(v["oracleDigest"]) == 64)
        browser = [c for c in v["checks"] if c["kind"] == "browser"]
        check(f"{label}: browser check recorded", len(browser) == 1)
        if browser:
            check(f"{label}: browser metadata recorded",
                  "chromium" in browser[0].get("detail", "").lower(),
                  browser[0].get("detail", ""))
    reviews = b.get("reviews", [])
    check(f"{label}: independent reviewer verdict exists",
          len(reviews) >= 1 and reviews[0]["authority"] == "reviewer")
    if reviews:
        check(f"{label}: reviewer actor is separate from implementer",
              reviews[0]["actor"] != candidate["implementerWorkerId"], reviews[0]["actor"])


def main():
    WORK_ROOT.mkdir(parents=True, exist_ok=True)
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    install_stub()
    repo, base = create_fixture_repo()
    procs = Processes()
    state = {}
    try:
        procs.start_core()
        check("Core is healthy", True)

        # Action + frozen spec.
        status, body, _ = call("POST", "/api/v1/actions", {
            "title": "F1-S4-A quality pipeline",
            "description": "Real implementer, immutable candidate, independent verification.",
        })
        assert status == 201, (status, body)
        action = body["action"]
        functional = ("const {multiply}=require('./add.js');"
                      "if(multiply(2,3)!==6) process.exit(1);")
        negative = ("const {add}=require('./add.js');"
                    "if(add(2,3)!==5) process.exit(1);")
        spec_body = {
            "actionId": action["id"],
            "goal": ("Append a correct multiply(a,b) export to add.js and wire the "
                     "operation selector in app.js so multiply computes a*b. Keep add "
                     "behavior and the existing UI contract unchanged. Never modify "
                     "browser-check.mjs, add.test.js, index.html, or package.json."),
            "repository": str(repo),
            "baseRevision": base,
            "allowedPaths": ["add.js", "app.js"],
            "acceptanceCriteria": [
                {"id": "format", "kind": "format", "description": "syntax",
                 "command": ["node", "--check", "add.js"], "expectExit": 0, "timeoutSecs": 30},
                {"id": "test", "kind": "test", "description": "unit tests",
                 "command": ["node", "--test", "add.test.js"], "expectExit": 0, "timeoutSecs": 60},
                {"id": "functional", "kind": "functional", "description": "multiply works",
                 "command": ["node", "-e", functional], "expectExit": 0, "timeoutSecs": 30},
                {"id": "browser", "kind": "browser", "description": "UI multiply journey",
                 "command": ["node", "browser-check.mjs"], "expectExit": 0, "timeoutSecs": 120},
            ],
            "negativeCases": [
                {"id": "negative", "kind": "negative", "description": "add regression",
                 "command": ["node", "-e", negative], "expectExit": 0, "timeoutSecs": 30},
            ],
            "riskClass": "R0",
            "qualityProfile": "node-browser",
            "budget": {"maxAttempts": 1, "timeoutSecs": 600},
            "executionMode": "trusted-local",
            "provider": "opencode",
            "model": "opencode-go/deepseek-v4.1-flash",
            "paseoMode": "build",
        }
        status, body, _ = call("POST", "/api/v1/action-specs", spec_body)
        assert status == 201, (status, body)
        spec = body["spec"]
        check("frozen ActionSpec bound to policy/profile", spec["contentHash"] != "" and
              spec["policyRevision"] == "quality-baseline-s4a-v1" and spec["qualityProfile"] == "node-browser")
        state["specId"] = spec["id"]
        state["actionId"] = action["id"]
        state["repo"] = str(repo)
        state["base"] = base

        procs.start_worker()
        check("Worker registered and polling", True)

        # A1-A4: real implementer -> immutable candidate -> verifier -> reviewer -> ACCEPTED.
        set_mode("good")
        run_id = queue_run(spec["id"])
        run = wait_run(run_id)
        check("A1 real implementer Run succeeded with spec binding",
              run["status"] == "succeeded" and run.get("specId") == spec["id"], run["status"])
        candidate = run_candidate(run_id)
        check("A1 CandidateRevision exists and is immutable", candidate is not None and
              candidate["state"] == "frozen" and len(candidate["commitSha"]) in (40, 64))
        if not candidate:
            raise RuntimeError("no candidate for good run")
        check("A1 candidate has a meaningful nonempty source diff",
              len(candidate["changedPaths"]) >= 1 and
              set(candidate["changedPaths"]) >= {"add.js", "app.js"},
              ",".join(candidate["changedPaths"]))
        check("A1 candidate parent is the frozen base revision",
              candidate["baseRevision"] == base)
        state["acceptedRunId"] = run_id
        state["acceptedCandidateId"] = candidate["id"]
        state["acceptedCandidateSha"] = candidate["commitSha"]
        decision, b = verify_candidate(candidate["id"], "ACCEPTED")
        check("A2 independent verifier exercised the exact frozen candidate",
              len(b["verifications"]) == 1 and
              b["verifications"][0]["candidateSha"] == candidate["commitSha"])
        check("A2 functional and negative checks passed",
              all(c["status"] == "PASS" for c in b["verifications"][0]["checks"]))
        check("A3 independent reviewer approved the exact candidate",
              len(b["reviews"]) == 1 and b["reviews"][0]["verdict"] == "APPROVE")
        check("A4 Core Quality Gate accepted the verified candidate",
              decision["candidateSha"] == candidate["commitSha"] and
              decision["actor"] == "core-quality-gate")
        check_evidence_provenance(candidate, b, "A15/A17")

        # A5: a failing mandatory check is never accepted, even with reviewer APPROVE.
        set_mode("failing")
        fail_run = queue_run(spec["id"])
        wait_run(fail_run)
        failing = run_candidate(fail_run)
        check("A5 failing candidate froze (implementer claimed success)", failing is not None)
        if failing:
            d5, b5 = verify_candidate(failing["id"], "REPAIR_REQUIRED")
            functional_check = next(c for c in b5["verifications"][0]["checks"]
                                    if c["id"] == "functional")
            check("A5 mandatory functional check is FAIL", functional_check["status"] == "FAIL")
            check("A5 reviewer APPROVE cannot override a failing check",
                  b5["reviews"][0]["verdict"] == "APPROVE")
            state["rejectedRunId"] = fail_run
            state["rejectedCandidateId"] = failing["id"]

        # A6: evidence with a different SHA is rejected. Use the candidate
        # that is currently frozen (A5's failed candidate).
        target = failing if failing else candidate
        target_bundle = bundle(target["id"])
        target_oracle = target_bundle["verifications"][0]["oracleDigest"]
        forged = {
            "instanceId": "verifier-1", "candidateId": target["id"],
            "candidateSha": "0" * len(target["commitSha"]),
            "specId": spec["id"], "specHash": spec["contentHash"],
            "policyRevision": spec["policyRevision"], "oracleDigest": target_oracle,
            "cleanResource": "git-clone:forged", "checks": [], "findings": [],
            "idempotencyKey": "forged-sha",
        }
        status, body, _ = call("POST", "/api/v1/verifier/evidence", forged, token=VERIFIER_TOKEN)
        check("A6 mismatched candidate SHA evidence rejected", status in (400, 403, 409),
              f"status={status}")
        mismatch = dict(forged)
        mismatch["candidateSha"] = target["commitSha"]
        mismatch["specHash"] = "0" * 64
        mismatch["idempotencyKey"] = "forged-spec"
        status, body, _ = call("POST", "/api/v1/verifier/evidence", mismatch, token=VERIFIER_TOKEN)
        check("A6 mismatched spec identity rejected", status in (400, 403, 409),
              f"status={status}")

        # A7: scoped authority and no self-submitted verifier evidence.
        status, _, _ = call("POST", "/api/v1/verifier/evidence", forged, token=WORKER_TOKEN)
        check("A7 implementer/worker credential cannot submit verifier evidence", status == 401)
        self_verifier = dict(forged)
        self_verifier["candidateSha"] = target["commitSha"]
        self_verifier["specHash"] = spec["contentHash"]
        self_verifier["oracleDigest"] = target_oracle
        self_verifier["instanceId"] = "local-1"
        self_verifier["idempotencyKey"] = "self-verifier"
        status, _, _ = call("POST", "/api/v1/verifier/evidence", self_verifier, token=VERIFIER_TOKEN)
        check("A7 verifier evidence naming the implementer identity is rejected", status == 403,
              f"status={status}")
        status, _, _ = call("POST", "/api/v1/reviewer/verdict", {"candidateId": target["id"]},
                            token=WORKER_TOKEN)
        check("A7 worker credential cannot submit reviewer verdicts", status == 401)
        status, _, _ = call("POST", f"/api/v1/candidates/{target['id']}/deliver", {},
                            token=API_TOKEN)
        check("A10 unapproved delivery route does not exist", status == 404, f"status={status}")

        # A16: browser acceptance failure is mandatory.
        set_mode("browser-failing")
        browser_run = queue_run(spec["id"])
        wait_run(browser_run)
        browser_candidate = run_candidate(browser_run)
        check("A16 browser-failing candidate froze", browser_candidate is not None)
        if browser_candidate:
            d16, b16 = verify_candidate(browser_candidate["id"], "REPAIR_REQUIRED")
            browser_check = next(c for c in b16["verifications"][0]["checks"]
                                 if c["kind"] == "browser")
            check("A16 browser check detected the UI regression", browser_check["status"] == "FAIL")
            check("A16 non-browser checks still passed",
                  all(c["status"] == "PASS" for c in b16["verifications"][0]["checks"]
                      if c["kind"] != "browser"))
            state["browserRejectedRunId"] = browser_run
            state["browserRejectedCandidateId"] = browser_candidate["id"]

        # A8: repair gets a new identity; prior evidence is invalidated.
        set_mode("good")
        repair_run = queue_run(spec["id"])
        wait_run(repair_run)
        repaired = run_candidate(repair_run)
        check("A8 repaired candidate has a new identity",
              repaired is not None and repaired["id"] != failing["id"] and
              repaired["commitSha"] != failing["commitSha"])
        old = bundle(failing["id"])["candidate"]
        check("A8 prior candidate is superseded", old["state"] == "superseded")
        check("A8 repaired candidate requires fresh independent evidence",
              len(bundle(repaired["id"])["verifications"]) == 0)
        state["acceptedRunId"] = repair_run
        state["acceptedCandidateId"] = repaired["id"]
        state["acceptedCandidateSha"] = repaired["commitSha"]
        d8, b8 = verify_candidate(repaired["id"], "ACCEPTED")
        check("A8 repaired candidate accepted only after revalidation",
              d8["candidateSha"] == repaired["commitSha"])

        # A10: unauthorized repository and out-of-scope diff are rejected.
        bad_repo = dict(spec_body)
        bad_repo["repository"] = "/tmp/definitely-not-an-authorized-repo"
        status, _, _ = call("POST", "/api/v1/action-specs", bad_repo)
        check("A10 unauthorized repository rejected", status == 400, f"status={status}")
        set_mode("outside-scope")
        scope_run = queue_run(spec["id"])
        scope_final = wait_run(scope_run)
        check("A10 out-of-scope candidate fails closed", scope_final["status"] == "failed",
              scope_final["status"])
        check("A10 no CandidateRevision exists for the rejected scope",
              run_candidate(scope_run) is None)

        # A9: restart, replay, and exactly one authoritative decision.
        before = bundle(repaired["id"])
        procs.stop_core()
        procs.start_core()
        after = bundle(repaired["id"])
        check("A9 decision survives Core restart",
              after.get("decision", {}).get("id") == before["decision"]["id"])
        verification = after["verifications"][0]
        replay_body = {
            "instanceId": verification["actor"], "candidateId": verification["candidateId"],
            "candidateSha": verification["candidateSha"], "specId": verification["specId"],
            "specHash": verification["specHash"], "policyRevision": verification["policyRevision"],
            "oracleDigest": verification["oracleDigest"],
            "cleanResource": verification["cleanResource"],
            "checks": verification["checks"], "findings": verification["findings"],
            "idempotencyKey": verification["idempotencyKey"],
        }
        status, body, headers = call("POST", "/api/v1/verifier/evidence", replay_body,
                                     token=VERIFIER_TOKEN)
        check("A9 repeated evidence delivery is an idempotent replay",
              status in (200, 201) and headers.get("idempotent-replayed") == "true",
              f"status={status} replayed={headers.get('idempotent-replayed')}")
        all_events = call("GET", "/api/v1/events?after=0&limit=1000")[1]["events"]
        gate_events = [e for e in all_events if e["eventType"] == "QualityGateDecided"
                       and e["aggregateId"] == repaired["id"]]
        check("A9 exactly one QualityGateDecided event exists", len(gate_events) == 1,
              str(len(gate_events)))
        status, body, headers = call("POST", f"/api/v1/candidates/{repaired['id']}/gate", {},
                                     token=API_TOKEN)
        check("A9 explicit re-evaluation returns the same decision",
              status == 200 and body["decision"]["id"] == after["decision"]["id"] and
              body.get("replayed") is True)

        # A13/A14: agent-native Playwright CLI can drive the real Office UI.
        agent_env = os.environ.copy()
        agent_env.update({
            "BASE_URL": BASE, "FACTORY_API_TOKEN": API_TOKEN,
            "CHROMIUM_EXE": CHROMIUM_EXE,
            "S4A_BROWSER_EVIDENCE_DIR": str(EVIDENCE),
            "S4A_CANDIDATE_ID": repaired["id"],
            "S4A_BROWSER_ACTION_TITLE": "Agent browser F1-S4-A",
        })
        agent = subprocess.run(
            ["node", str(ROOT / "scripts" / "agent-browser-office.mjs")],
            capture_output=True, text=True, timeout=300, env=agent_env,
        )
        check("A13 agent-native playwright-cli operated real Chromium", agent.returncode == 0,
              agent.stdout[-300:] + agent.stderr[-300:])
        if agent.returncode == 0:
            evidence = json.loads((EVIDENCE / "agent-browser-evidence.json").read_text())
            check("A13 pinned tool version recorded", evidence.get("toolVersion") == "0.1.22")
            check("A14 real UI control created an authoritative Action",
                  evidence.get("ok") is True and evidence.get("actionId"))
            check("A14 no console/network failures observed",
                  evidence.get("consoleErrors") == 0 and evidence.get("failedRequests") == [])

        # A11: candidate/verifier/reviewer/gate states in the real browser.
        journey_env = os.environ.copy()
        journey_env.update({
            "BASE_URL": BASE, "FACTORY_API_TOKEN": API_TOKEN,
            "CHROMIUM_EXE": CHROMIUM_EXE,
            "S4A_EVIDENCE_DIR": str(EVIDENCE),
            "S4A_ACCEPTED_RUN": repair_run,
            "S4A_ACCEPTED_CANDIDATE": repaired["id"],
            "S4A_ACCEPTED_SHA": repaired["commitSha"],
            "S4A_REJECTED_RUN": browser_run,
            "S4A_REJECTED_CANDIDATE": browser_candidate["id"],
            "S4A_REJECTED_SHA": browser_candidate["commitSha"],
        })
        journey = subprocess.run(
            ["node", str(ROOT / "office" / "e2e-browser" / "s4a-journey.mjs")],
            capture_output=True, text=True, timeout=300, env=journey_env,
        )
        check("A11 browser journey proves accepted + rejected candidate states",
              journey.returncode == 0, journey.stdout[-300:] + journey.stderr[-300:])

        # A17: no Core credentials in durable browser/verifier evidence.
        durable = json.dumps(bundle(repaired["id"]))
        check("A17 API token is not present in durable evidence", API_TOKEN not in durable)
        check("A17 worker token is not present in durable evidence", WORKER_TOKEN not in durable)

        state["browserActionEvidence"] = str(EVIDENCE / "agent-browser-evidence.json")
        (WORK_ROOT / "state.json").write_text(json.dumps(state, indent=2))
    finally:
        procs.close()

    if FAILURES:
        print(f"\n{len(FAILURES)} check(s) FAILED: {FAILURES}")
        return 1
    print("\nAll F1-S4-A deterministic acceptance checks passed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        print(f"[FAIL] oracle aborted: {e}")
        traceback = __import__("traceback")
        traceback.print_exc()
        sys.exit(1)
