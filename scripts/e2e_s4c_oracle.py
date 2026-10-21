#!/usr/bin/env python3
"""F1-S4-C deterministic multi-project acceptance oracle.

Real Core, real Worker, real Core-spawned verifier/reviewer, real Chromium,
two external Git Project fixtures, and a deterministic Paseo CLI simulator.
No provider calls and no paid sessions. The only simulated component is the
language-model agent decision; real git edits, browser CLI invocation,
candidate freezing, independent browser verification and the Quality Gate
all execute for real.
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
VLOG = Path(sys.argv[1])
CALC = Path(sys.argv[2])
CORE_BIN = sys.argv[3]
WORKER_BIN = sys.argv[4]
VERIFIER_BIN = sys.argv[5]
REVIEWER_BIN = sys.argv[6]
WORK_ROOT = Path(sys.argv[7])
CHROMIUM_EXE = sys.argv[8]
PLAYWRIGHT_CORE = sys.argv[9]
PORT = sys.argv[10]
WEB_DIR = sys.argv[11]
BROWSER_CLI = sys.argv[12]
OAUTH = sys.argv[13] if len(sys.argv) > 13 else ""

BASE = f"http://127.0.0.1:{PORT}"
API_TOKEN = "s4c-api-" + os.urandom(8).hex()
WORKER_TOKEN = "s4c-worker-" + os.urandom(8).hex()
VERIFIER_TOKEN = "s4c-verifier-" + os.urandom(8).hex()
REVIEWER_TOKEN = "s4c-reviewer-" + os.urandom(8).hex()
DB = str(WORK_ROOT / "factory.db")
STUB_DIR = WORK_ROOT / "stub"
STUB_BIN = WORK_ROOT / "stubbin" / "paseo"
EVIDENCE = WORK_ROOT / "evidence"
JOURNEY_FILE = EVIDENCE / "office-journey.json"
TERMINAL = {"accepted", "repair_required", "blocked", "human_required", "failed", "cancelled"}
FAILURES = []
CHECKS = []


def check(name, cond, detail=""):
    print(f"[{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""), flush=True)
    CHECKS.append({"name": name, "pass": cond, **({"detail": detail} if detail else {})})
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
            "X-Correlation-ID": f"s4c-oracle-{time.time_ns()}",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as res:
            raw = res.read().decode()
            payload = json.loads(raw or "{}")
            return res.status, payload
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            payload = json.loads(raw or "{}")
        except json.JSONDecodeError:
            payload = {"raw": raw}
        return e.code, payload


def wait_health(timeout=40):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            status, _ = call("GET", "/api/v1/health")
            if status == 200:
                return True
        except Exception:
            pass
        time.sleep(0.2)
    return False


def git_sha(repo):
    return subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()


def set_stub(mode="good", delay=None):
    (STUB_DIR / "mode").write_text(mode + "\n", encoding="utf-8")
    if delay is None:
        (STUB_DIR / "delay").unlink(missing_ok=True)
    else:
        (STUB_DIR / "delay").write_text(str(delay) + "\n", encoding="utf-8")


def set_sequence(project, modes):
    (STUB_DIR / f"sequence-{project}").write_text("\n".join(modes) + "\n", encoding="utf-8")


def clear_sequences():
    for project in ("vlog", "calculator"):
        (STUB_DIR / f"sequence-{project}").unlink(missing_ok=True)


def browser_env():
    return {
        "CHROMIUM_EXE": CHROMIUM_EXE,
        "FACTORY_PLAYWRIGHT_CORE": PLAYWRIGHT_CORE,
        "FACTORY_BROWSER_CLI": BROWSER_CLI,
    }


class Processes:
    def __init__(self):
        self.core = None
        self.worker = None

    def core_env(self):
        env = os.environ.copy()
        env.update({
            "FACTORY_API_TOKEN": API_TOKEN,
            "FACTORY_WORKER_TOKEN": WORKER_TOKEN,
            "FACTORY_VERIFIER_TOKEN": VERIFIER_TOKEN,
            "FACTORY_REVIEWER_TOKEN": REVIEWER_TOKEN,
        })
        env.update(browser_env())
        return env

    def start_core(self):
        log = open(WORK_ROOT / "core.log", "ab")
        self.core = subprocess.Popen(
            [CORE_BIN, "--addr", f"127.0.0.1:{PORT}", "--db", DB,
             "--web-dir", WEB_DIR, "--verifier-bin", VERIFIER_BIN,
             "--reviewer-bin", REVIEWER_BIN, "--campaign-interval", "250ms"],
            env=self.core_env(), stdout=log, stderr=log, start_new_session=True,
        )
        log.close()
        if not wait_health():
            raise RuntimeError("Core did not become healthy")

    def start_worker(self, worker_id="s4c-worker", browser=True, max_concurrent=2):
        env = {
            "PATH": os.environ.get("PATH", ""),
            "HOME": os.environ.get("HOME", ""),
            "TMPDIR": os.environ.get("TMPDIR", ""),
            "FACTORY_WORKER_TOKEN": WORKER_TOKEN,
            "PASEO_BIN": STUB_BIN,
            "S4C_STUB_DIR": str(STUB_DIR),
        }
        if browser:
            env.update(browser_env())
        log = open(WORK_ROOT / f"worker-{worker_id}.log", "ab")
        self.worker = subprocess.Popen(
            [WORKER_BIN, "--core", BASE, "--worker-id", worker_id,
             "--exec-mode", "local", "--max-concurrent", str(max_concurrent),
             "--recovery-interval", "3s"],
            env=env, stdout=log, stderr=log, start_new_session=True,
        )
        log.close()

    def stop_worker(self, graceful=True):
        if self.worker and self.worker.poll() is None:
            if graceful:
                self.worker.terminate()
                try:
                    self.worker.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    self.worker.kill()
            else:
                self.worker.kill()
        self.worker = None

    def kill_core(self):
        if self.core and self.core.poll() is None:
            self.core.kill()
            try:
                self.core.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
        self.core = None

    def stop(self):
        self.stop_worker()
        if self.core and self.core.poll() is None:
            self.core.terminate()
            try:
                self.core.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.core.kill()
        self.core = None


def register_project(project_id, name, repo, profile, capabilities, endpoints, base_refs=None):
    status, payload = call("POST", "/api/v1/projects", {
        "id": project_id, "name": name, "repositoryPath": str(repo),
        "baseRefs": base_refs or ["main"], "qualityProfile": profile,
        "workspaceScope": ".", "capabilities": capabilities,
        "browserEndpoints": endpoints, "executionMode": "trusted-local",
    })
    return status, payload


def create_action(project_id, title, description=""):
    status, payload = call("POST", "/api/v1/actions", {
        "projectId": project_id, "title": title, "description": description,
    })
    if status != 201:
        raise RuntimeError(f"create action failed: {status} {payload}")
    return payload["action"]


def create_spec(action, repo, base_revision, goal, allowed_paths, profile, checks, negative, max_attempts=1):
    status, payload = call("POST", "/api/v1/action-specs", {
        "projectId": action["projectId"], "actionId": action["id"],
        "goal": goal, "baseRevision": base_revision,
        "allowedPaths": allowed_paths,
        "acceptanceCriteria": checks, "negativeCases": negative,
        "riskClass": "R0", "qualityProfile": profile,
        "budget": {"maxAttempts": max_attempts, "timeoutSecs": 600},
        "executionMode": "trusted-local", "provider": "opencode",
        "model": "opencode-go/deepseek-v4.1-flash", "paseoMode": "build",
    })
    if status != 201:
        raise RuntimeError(f"create spec failed: {status} {payload}")
    return payload["spec"]


def start_campaign(spec_id, repairs=0, parallel=2, max_attempts=0):
    body = {
        "specId": spec_id, "maxRepairCycles": repairs,
        "maxWallClockSecs": 1800, "maxParallel": parallel,
    }
    if max_attempts:
        body["maxImplementerAttempts"] = max_attempts
    status, payload = call("POST", "/api/v1/campaigns", body)
    if status != 201:
        raise RuntimeError(f"create campaign failed: {status} {payload}")
    campaign = payload["campaign"]
    status, payload = call("POST", f"/api/v1/campaigns/{campaign['id']}/start", {})
    if status != 200:
        raise RuntimeError(f"start campaign failed: {status} {payload}")
    return campaign


def campaign(campaign_id):
    status, payload = call("GET", f"/api/v1/campaigns/{campaign_id}")
    if status != 200:
        raise RuntimeError(f"get campaign failed: {status} {payload}")
    return payload


def wait_campaign(campaign_id, timeout=300):
    deadline = time.time() + timeout
    latest = {}
    while time.time() < deadline:
        latest = campaign(campaign_id)
        if latest["campaign"]["status"] in TERMINAL:
            return latest
        time.sleep(0.3)
    return latest


def run(campaign_id):
    projection = campaign(campaign_id)
    task = next((t for t in projection["tasks"] if t["key"] == "implementer-0"), None)
    if not task or not task.get("runId"):
        return None
    status, payload = call("GET", f"/api/v1/runs/{task['runId']}")
    return payload.get("run") if status == 200 else None


def verifier_browser_check(bundle):
    return next(
        (c for c in bundle["verifications"][0]["checks"] if c["kind"] == "browser"),
        None,
    )


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    STUB_DIR.mkdir(parents=True, exist_ok=True)
    (WORK_ROOT / "stubbin").mkdir(parents=True, exist_ok=True)
    stub_source = WORK_ROOT / "stubbin" / "s4c_paseo_stub.py"
    shutil.copy2(ROOT / "scripts" / "s4c_paseo_stub.py", stub_source)
    STUB_BIN.write_text(
        "#!/bin/sh\nexport S4C_STUB_DIR=\"%s\"\nexec python3 \"%s\" \"$@\"\n"
        % (STUB_DIR, stub_source),
        encoding="utf-8",
    )
    STUB_BIN.chmod(0o755)
    set_stub("good")

    processes = Processes()
    result = {"checks": CHECKS, "failures": FAILURES}
    try:
        processes.start_core()
        processes.start_worker()

        # C17: the whole Project -> Action -> Spec -> Campaign journey is
        # performed through Factory Office in real Chromium.
        journey_env = os.environ.copy()
        journey_env.update({
            "BASE_URL": BASE, "FACTORY_API_TOKEN": API_TOKEN,
            "CHROMIUM_EXE": CHROMIUM_EXE, "S4C_VLOG_REPO": str(VLOG),
            "S4C_JOURNEY_FILE": str(JOURNEY_FILE),
            "S4C_JOURNEY_EVIDENCE": str(EVIDENCE / "office"),
        })
        journey = subprocess.run(
            ["node", str(ROOT / "office" / "e2e-browser" / "s4c-journey.mjs")],
            env=journey_env, capture_output=True, text=True, timeout=900,
        )
        print("[journey]", journey.stdout[-4000:], flush=True)
        if journey.returncode != 0:
            print("[journey stderr]", journey.stderr[-2000:], flush=True)
        check("C17 Office journey; register Project A, Action, spec, Campaign",
              journey.returncode == 0, journey.stderr[-300:] if journey.returncode else "")
        journey_result = json.loads(JOURNEY_FILE.read_text(encoding="utf-8")) if JOURNEY_FILE.exists() else {}
        a_action_id = journey_result.get("actionId", "")
        a_spec_id = journey_result.get("specId", "")
        a_campaign_id = journey_result.get("campaignId", "")
        if not check("C17 journey identifiers", bool(a_action_id and a_spec_id and a_campaign_id)):
            raise RuntimeError("journey did not produce identifiers")

        a_projection = campaign(a_campaign_id)
        a_candidate = a_projection.get("candidate") or {}
        a_bundle = call("GET", f"/api/v1/candidates/{a_candidate['id']}/evidence")[1]["bundle"]
        a_run = run(a_campaign_id)
        a_spec = call("GET", f"/api/v1/action-specs/{a_spec_id}")[1]["spec"]

        check("C1 Project A durably registered with repository identity",
              len(a_spec.get("repositoryIdentity", "")) == 64 and a_spec.get("projectConfigRevision") == 1)
        check("C4 frozen spec records the exact Project configuration revision",
              a_spec.get("projectConfigRevision") == 1 and len(a_spec.get("bindingHash", "")) == 64)
        check("C5 Implementer produced a candidate from Project A's repository",
              a_candidate.get("repository") == str(VLOG.resolve()) and
              a_candidate.get("baseRevision") == git_sha(VLOG))
        check("C7 Project A Run received the browser capability",
              "browser" in (a_run or {}).get("capabilities", []))
        check("C9 Factory-managed Implementer invoked Playwright CLI through the wrapper",
              ((a_run or {}).get("result") or {}).get("browserInvoked") is True,
              json.dumps((a_run or {}).get("result")))
        browser_check = verifier_browser_check(a_bundle)
        check("C10 Independent verifier ran a real Chromium browser check",
              browser_check is not None and browser_check.get("status") == "PASS" and
              "chromium" in (browser_check.get("detail") or ""),
              json.dumps(browser_check))
        check("C13 native orchestration; all four role tasks succeeded without external orchestration",
              all(t["status"] == "succeeded" for t in a_projection["tasks"]))
        check("C17 terminal Quality Gate decision is ACCEPTED",
              (a_projection.get("decision") or {}).get("status") == "ACCEPTED" and
              a_projection["decision"]["candidateSha"] == a_candidate["commitSha"])
        check("Quality Gate decision is bound to the Project binding hash",
              a_candidate.get("specBindingHash") == a_spec.get("bindingHash"))

        # C2 negative registration/identity cases.
        alias = WORK_ROOT / "vlog-alias"
        alias.symlink_to(VLOG)
        status, _ = register_project("vlog-alias", "Alias", alias, "node-browser",
                                     ["repository-edit", "node-toolchain", "browser"],
                                     ["http://127.0.0.1:4173"])
        check("C2 symlink alias of the same repository identity is rejected", status == 409, str(status))
        status, _ = register_project("missing", "Missing", WORK_ROOT / "not-a-repo", "node-basic",
                                     ["repository-edit", "node-toolchain"], [])
        check("C2 invalid repository path is rejected", status == 400, str(status))

        # Project B: independent non-browser Project and ActionSpec.
        status, payload = register_project("calculator", "Pocket Calculator", CALC, "node-basic",
                                           ["repository-edit", "node-toolchain"], [])
        check("C1 Project B registered independently", status == 201, json.dumps(payload) if status != 201 else "")
        b_action = create_action("calculator", "Correct decimal precision and division by zero")
        b_base = git_sha(CALC)
        b_functional = ("node\n--input-type=module\n-e\n"
                        "import {calculate,formatResult} from './src/calc.js';"
                        "if(formatResult(calculate(0.1,0.2,'add'))!=='0.3')process.exit(1);"
                        "if(formatResult(calculate(1,0,'divide'))!=='Cannot divide by zero')process.exit(1)")
        b_negative = ("node\n--input-type=module\n-e\n"
                      "import {add,subtract,multiply,divide} from './src/calc.js';"
                      "if(add(2,3)!==5||subtract(9,4)!==5||multiply(6,7)!==42||divide(8,2)!==4)process.exit(1)")
        b_checks = [
            {"id": "format", "kind": "format", "description": "syntax",
             "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 60},
            {"id": "test", "kind": "test", "description": "unit tests",
             "command": ["npm", "test"], "expectExit": 0, "timeoutSecs": 120},
            {"id": "functional", "kind": "functional", "description": "precision and division by zero",
             "command": b_functional.split("\n"), "expectExit": 0, "timeoutSecs": 60},
            {"id": "browser", "kind": "browser", "description": "protected calculator browser oracle",
             "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 180},
        ]
        b_spec = create_spec(b_action, CALC, b_base,
                             "Correct decimal precision handling and division-by-zero behavior while preserving all arithmetic operations and user interaction. Modify only src/calc.js and src/app.js.",
                             ["src/calc.js", "src/app.js"], "node-basic", b_checks,
                             [{"id": "negative", "kind": "negative", "description": "integer regression",
                               "command": b_negative.split("\n"), "expectExit": 0, "timeoutSecs": 60}])
        check("C7 Project B spec does not require browser capability",
              "browser" not in b_spec["capabilities"] and b_spec["capabilities"] == ["repository-edit", "node-toolchain"])
        b_campaign = start_campaign(b_spec["id"], repairs=0)
        b_projection = wait_campaign(b_campaign["id"], timeout=300)
        b_candidate = b_projection.get("candidate") or {}
        b_bundle = call("GET", f"/api/v1/candidates/{b_candidate['id']}/evidence")[1]["bundle"] if b_candidate else {}
        b_run = run(b_campaign["id"])
        check("C3 Project B completes with its own candidate lineage",
              b_projection["campaign"]["status"] == "accepted" and
              b_candidate.get("projectId") == "calculator" and
              b_candidate.get("commitSha") != a_candidate.get("commitSha"))
        check("C5 Project B candidate is bound to its own repository and base",
              b_candidate.get("repository") == str(CALC.resolve()) and
              b_candidate.get("baseRevision") == b_base)
        check("C12 Project B did not receive browser capability",
              "browser" not in (b_run or {}).get("capabilities", []) and
              "browserInvoked" not in ((b_run or {}).get("result") or {}))
        b_browser = verifier_browser_check(b_bundle)
        check("C12 independent verifier still ran Project B browser acceptance",
              b_browser is not None and b_browser.get("status") == "PASS")
        check("C3/C24 Project evidence remains separate",
              a_bundle["candidate"]["projectId"] == "vlog" and
              b_bundle["candidate"]["projectId"] == "calculator" and
              a_bundle["decision"]["candidateSha"] == a_candidate["commitSha"] and
              b_bundle["decision"]["candidateSha"] == b_candidate["commitSha"])

        # C6 forged/cross-project relationships fail closed.
        status, _ = call("POST", "/api/v1/runs", {
            "specId": b_spec["id"], "projectId": "vlog", "actionId": a_action_id,
            "workloadKind": "paseo-task", "workloadInput": {"task": "forged"},
        })
        check("C6 forged Run Project/Action/Spec relationship is rejected", status in (400, 403, 409), str(status))
        forged_action = create_action("vlog", "Forged cross-project spec")
        status, _ = call("POST", "/api/v1/action-specs", {
            "projectId": "vlog", "actionId": forged_action["id"], "goal": "forged",
            "repository": str(CALC), "baseRevision": b_base,
            "allowedPaths": ["src"], "riskClass": "R0", "qualityProfile": "node-basic",
            "acceptanceCriteria": b_checks, "negativeCases": [{"id": "negative", "kind": "negative",
                "description": "x", "command": ["node", "-e", "process.exit(0)"], "expectExit": 0,
                "timeoutSecs": 30}],
            "budget": {"maxAttempts": 1, "timeoutSecs": 600},
            "executionMode": "trusted-local", "provider": "opencode",
            "model": "opencode-go/deepseek-v4.1-flash", "paseoMode": "build",
        })
        check("C6 Project A ActionSpec cannot reference Project B's repository", status == 403, str(status))
        status, _ = call("POST", "/api/v1/verifier/evidence", {
            "instanceId": "forged-verifier", "candidateId": a_candidate["id"],
            "candidateSha": a_candidate["commitSha"], "specId": b_spec["id"],
            "specHash": b_spec["contentHash"], "policyRevision": b_spec["policyRevision"],
            "oracleDigest": "0" * 64, "cleanResource": "forged",
            "checks": [], "findings": [], "idempotencyKey": "forged-cross-project",
        }, token=VERIFIER_TOKEN)
        check("C6 cross-project verifier evidence is rejected", status in (400, 403, 409), str(status))

        # C4 a later Project configuration change cannot alter the frozen spec/candidate.
        status, updated = call("PATCH", "/api/v1/projects/vlog", {"name": "Field Notes Vlog (renamed)"})
        after_spec = call("GET", f"/api/v1/action-specs/{a_spec_id}")[1]["spec"]
        after_bundle = call("GET", f"/api/v1/candidates/{a_candidate['id']}/evidence")[1]["bundle"]
        check("C4 Project config update advances revision without rewriting the frozen spec",
              status == 200 and updated["project"]["configRevision"] == 2 and
              after_spec["projectConfigRevision"] == 1)
        check("C4 frozen spec content/binding and accepted evidence are unchanged",
              after_spec["bindingHash"] == a_spec["bindingHash"] and
              after_spec["contentHash"] == a_spec["contentHash"] and
              after_bundle["decision"]["candidateSha"] == a_candidate["commitSha"])

        # C8 missing browser tooling is a truthful blocked result, not fallback.
        copy_repo = WORK_ROOT / "vlog-nobrowser"
        shutil.copytree(VLOG, copy_repo, ignore=shutil.ignore_patterns(".git"))
        subprocess.run(["git", "-C", str(copy_repo), "init", "-q", "-b", "main"], check=True)
        subprocess.run(["git", "-C", str(copy_repo), "add", "."], check=True)
        subprocess.run(["git", "-C", str(copy_repo), "-c", "user.name=fixture",
                        "-c", "user.email=fixture@factory.local", "commit", "-qm", "clone baseline"], check=True)
        status, _ = register_project("vlog-nobrowser", "Vlog without browser worker", copy_repo,
                                     "node-browser", ["repository-edit", "node-toolchain", "browser"],
                                     ["http://127.0.0.1:4173"])
        check("C8 browser-project clone registered", status == 201, str(status))
        processes.stop_worker()
        processes.start_worker(worker_id="s4c-nobrowser", browser=False)
        no_action = create_action("vlog-nobrowser", "Filtering without browser tooling")
        no_base = git_sha(copy_repo)
        no_spec = create_spec(no_action, copy_repo, no_base,
                              "Implement category filtering. Keep tests and protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", [
                                  {"id": "format", "kind": "format", "description": "syntax",
                                   "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 60},
                                  {"id": "test", "kind": "test", "description": "tests",
                                   "command": ["npm", "test"], "expectExit": 0, "timeoutSecs": 60},
                                  {"id": "functional", "kind": "functional", "description": "filter",
                                   "command": ["node", "--input-type=module", "-e",
                                               "import {filterEntries} from './src/filter.js'; if(filterEntries([{category:'A'}],'B').length!==0)process.exit(1)"],
                                   "expectExit": 0, "timeoutSecs": 60},
                                  {"id": "browser", "kind": "browser", "description": "protected browser oracle",
                                   "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 180},
                              ], [{"id": "negative", "kind": "negative", "description": "data regression",
                                   "command": ["node", "--input-type=module", "-e",
                                               "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                   "expectExit": 0, "timeoutSecs": 60}])
        no_campaign = start_campaign(no_spec["id"], repairs=0)
        no_projection = wait_campaign(no_campaign["id"], timeout=120)
        check("C8 no eligible browser Worker produces a truthful BLOCKED campaign",
              no_projection["campaign"]["status"] == "blocked" and
              "capabilit" in (no_projection["campaign"].get("terminalReason") or "").lower(),
              no_projection["campaign"].get("terminalReason", ""))
        processes.stop_worker()
        processes.start_worker()

        # C11 a broken UI fails mandatory browser verification and is not accepted.
        set_stub("browser-broken")
        broken_action = create_action("vlog", "Deliberately broken browser behavior")
        broken_spec = create_spec(broken_action, VLOG, git_sha(VLOG),
                                  "Deliberately broken browser behavior: implement filtering but break the no-results state. Keep protected files unchanged.",
                                  ["src/filter.js", "src/app.js"], "node-browser", [
                                      {"id": "format", "kind": "format", "description": "syntax",
                                       "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 60},
                                      {"id": "test", "kind": "test", "description": "tests",
                                       "command": ["npm", "test"], "expectExit": 0, "timeoutSecs": 60},
                                      {"id": "functional", "kind": "functional", "description": "filter",
                                       "command": ["node", "--input-type=module", "-e",
                                                   "import {filterEntries} from './src/filter.js'; if(filterEntries([{category:'A'}],'B').length!==0)process.exit(1)"],
                                       "expectExit": 0, "timeoutSecs": 60},
                                      {"id": "browser", "kind": "browser", "description": "protected browser oracle",
                                       "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 180},
                                  ], [{"id": "negative", "kind": "negative", "description": "data regression",
                                       "command": ["node", "--input-type=module", "-e",
                                                   "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                       "expectExit": 0, "timeoutSecs": 60}])
        broken_campaign = start_campaign(broken_spec["id"], repairs=0)
        broken_projection = wait_campaign(broken_campaign["id"], timeout=300)
        broken_bundle = call("GET", f"/api/v1/candidates/{broken_projection['candidate']['id']}/evidence")[1]["bundle"] if broken_projection.get("candidate") else {}
        broken_browser = verifier_browser_check(broken_bundle) if broken_bundle else None
        check("C11 broken UI fails mandatory browser verification",
              broken_browser is not None and broken_browser.get("status") == "FAIL")
        check("C11 Quality Gate does not accept the broken candidate",
              (broken_projection.get("decision") or {}).get("status") != "ACCEPTED" and
              broken_projection["campaign"]["status"] in {"repair_required", "blocked", "human_required"})
        set_stub("good")

        # C15 cancellation during an active Implementer run.
        set_stub("good", delay=8)
        cancel_action = create_action("calculator", "Cancellation ownership")
        cancel_spec = create_spec(cancel_action, CALC, git_sha(CALC),
                                  "Correct division by zero. Keep protected files unchanged.",
                                  ["src/calc.js", "src/app.js"], "node-basic", b_checks,
                                  [{"id": "negative", "kind": "negative", "description": "integer regression",
                                    "command": b_negative.split("\n"), "expectExit": 0, "timeoutSecs": 60}])
        cancel_campaign = start_campaign(cancel_spec["id"], repairs=0)
        active = {}
        deadline = time.time() + 30
        while time.time() < deadline:
            active = campaign(cancel_campaign["id"])
            impl = next((t for t in active["tasks"] if t["key"] == "implementer-0"), {})
            if impl.get("status") in {"dispatched", "running"}:
                break
            time.sleep(0.2)
        status, _ = call("POST", f"/api/v1/campaigns/{cancel_campaign['id']}/cancel", {})
        cancelled = campaign(cancel_campaign["id"])
        cancel_run = run(cancel_campaign["id"])
        check("C15 cancellation terminates the Campaign", status == 200 and
              cancelled["campaign"]["status"] == "cancelled")
        check("C15 cancellation is scoped to the active Implementer Run",
              cancel_run is None or cancel_run.get("status") == "cancelled",
              json.dumps(cancel_run))
        set_stub("good")

        # C16 crash/restart during an active campaign.
        set_stub("good", delay=6)
        restart_action = create_action("calculator", "Restart durability")
        restart_spec = create_spec(restart_action, CALC, git_sha(CALC),
                                   "Correct decimal precision. Keep protected files unchanged.",
                                   ["src/calc.js", "src/app.js"], "node-basic", b_checks,
                                   [{"id": "negative", "kind": "negative", "description": "integer regression",
                                     "command": b_negative.split("\n"), "expectExit": 0, "timeoutSecs": 60}])
        restart_campaign = start_campaign(restart_spec["id"], repairs=0)
        deadline = time.time() + 30
        while time.time() < deadline:
            active = campaign(restart_campaign["id"])
            impl = next((t for t in active["tasks"] if t["key"] == "implementer-0"), {})
            if impl.get("status") == "running":
                break
            time.sleep(0.2)
        processes.kill_core()
        time.sleep(1)
        processes.start_core()
        restarted = wait_campaign(restart_campaign["id"], timeout=180)
        after_restart_campaigns = call("GET", "/api/v1/campaigns?projectId=calculator&limit=200")[1]["campaigns"]
        after_restart_specs = call("GET", f"/api/v1/actions/{restart_action['id']}/specs")[1]["specs"]
        identifiers = {(c["actionId"], c["specId"]) for c in after_restart_campaigns}
        check("C16 Campaign terminal state is reached after Core restart",
              restarted["campaign"]["status"] in TERMINAL, restarted["campaign"]["status"])
        check("C16 Project, spec, capability and campaign identities survive restart",
              len(after_restart_specs) == 1 and
              after_restart_specs[0]["projectConfigRevision"] >= 1 and
              restart_spec["capabilities"] == ["repository-edit", "node-toolchain"])
        check("C16 restart does not duplicate campaign identities",
              len(identifiers) == len(after_restart_campaigns), f"{len(identifiers)}/{len(after_restart_campaigns)}")
        set_stub("good")

        # C14: two projects make progress concurrently with one Worker.
        set_stub("good", delay=1)
        conc_a_action = create_action("vlog", "Concurrent liveness A")
        conc_a_spec = create_spec(conc_a_action, VLOG, git_sha(VLOG),
                                  "Implement filtering. Keep protected files unchanged.",
                                  ["src/filter.js", "src/app.js"], "node-browser", [
                                      {"id": "format", "kind": "format", "description": "syntax",
                                       "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 60},
                                      {"id": "test", "kind": "test", "description": "tests",
                                       "command": ["npm", "test"], "expectExit": 0, "timeoutSecs": 60},
                                      {"id": "functional", "kind": "functional", "description": "filter",
                                       "command": ["node", "--input-type=module", "-e",
                                                   "import {filterEntries} from './src/filter.js'; if(filterEntries([{category:'A'}],'B').length!==0)process.exit(1)"],
                                       "expectExit": 0, "timeoutSecs": 60},
                                      {"id": "browser", "kind": "browser", "description": "browser oracle",
                                       "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 180},
                                  ], [{"id": "negative", "kind": "negative", "description": "data regression",
                                       "command": ["node", "--input-type=module", "-e",
                                                   "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                       "expectExit": 0, "timeoutSecs": 60}])
        conc_b_action = create_action("calculator", "Concurrent liveness B")
        conc_b_spec = create_spec(conc_b_action, CALC, git_sha(CALC),
                                  "Correct division by zero. Keep protected files unchanged.",
                                  ["src/calc.js", "src/app.js"], "node-basic", b_checks,
                                  [{"id": "negative", "kind": "negative", "description": "integer regression",
                                    "command": b_negative.split("\n"), "expectExit": 0, "timeoutSecs": 60}])
        conc_a = start_campaign(conc_a_spec["id"], repairs=0)
        conc_b = start_campaign(conc_b_spec["id"], repairs=0)
        conc_a_final = wait_campaign(conc_a["id"], timeout=300)
        conc_b_final = wait_campaign(conc_b["id"], timeout=300)
        check("C14 eligible work in both Projects progresses under concurrency",
              conc_a_final["campaign"]["status"] == "accepted" and
              conc_b_final["campaign"]["status"] == "accepted")
        set_stub("good")

        # R1/R2/R3/R8: recoverable pre-candidate failure recovers through a
        # new fenced Run under the same Campaign, preserving the old Run.
        clear_sequences()
        set_sequence("vlog", ["empty", "good"])
        r1_action = create_action("vlog", "Recoverable implementation failure")
        r1_checks = [
            {"id": "format", "kind": "format", "description": "syntax",
             "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 60},
            {"id": "test", "kind": "test", "description": "tests",
             "command": ["npm", "test"], "expectExit": 0, "timeoutSecs": 60},
            {"id": "functional", "kind": "functional", "description": "filter",
             "command": ["node", "--input-type=module", "-e",
                         "import {filterEntries} from './src/filter.js'; if(filterEntries([{category:'A'}],'B').length!==0)process.exit(1)"],
             "expectExit": 0, "timeoutSecs": 60},
            {"id": "browser", "kind": "browser", "description": "protected browser oracle",
             "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 180},
        ]
        r1_spec = create_spec(r1_action, VLOG, git_sha(VLOG),
                              "Implement category filtering. Keep protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", r1_checks,
                              [{"id": "negative", "kind": "negative", "description": "data regression",
                                "command": ["node", "--input-type=module", "-e",
                                            "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                "expectExit": 0, "timeoutSecs": 60}])
        r1_campaign = start_campaign(r1_spec["id"], repairs=0, max_attempts=4)
        r1_final = wait_campaign(r1_campaign["id"], timeout=420)
        r1_recoveries = r1_final.get("recoveries") or []
        r1_attempts = r1_final["budget"]["attemptsUsed"]
        r1_new_run = run(r1_campaign["id"])
        r1_old_run = None
        if r1_recoveries:
            r1_old_run = call("GET", f"/api/v1/runs/{r1_recoveries[0]['previousRunId']}")[1].get("run")
        check("R1 recoverable pre-candidate failure does not terminate the Campaign",
              r1_final["campaign"]["status"] == "accepted", r1_final["campaign"].get("terminalReason", ""))
        check("R2 recovery preserves the failed Run identity and creates a new fenced Run",
              len(r1_recoveries) == 1 and r1_old_run is not None and
              r1_old_run["status"] == "failed" and r1_new_run is not None and
              r1_new_run["id"] != r1_old_run["id"] and r1_new_run["status"] == "succeeded",
              json.dumps(r1_recoveries))
        check("R2 recovery consumes bounded campaign attempts",
              r1_attempts == 2 and r1_final["budget"]["recoveryAttempts"] == 1,
              f"attempts={r1_attempts}")
        check("R3/R8 corrective execution is native and ends ACCEPTED with a valid candidate",
              r1_final.get("candidate") is not None and
              (r1_final.get("decision") or {}).get("status") == "ACCEPTED" and
              all(t["status"] in {"succeeded", "failed"} for t in r1_final["tasks"]),
              json.dumps([(t["key"], t["status"]) for t in r1_final["tasks"]]))
        check("R1/R8 failure history and recovery class remain inspectable",
              r1_recoveries and r1_recoveries[0]["failureClass"] == "RECOVERABLE_IMPLEMENTATION_FAILURE" and
              "no-source-change" in r1_recoveries[0]["reason"].lower())
        clear_sequences()
        set_stub("good")

        # R4: a genuine browser acceptance failure uses a new repair cycle,
        # new candidate and fresh independent evidence.
        set_sequence("vlog", ["browser-broken", "good"])
        r4_action = create_action("vlog", "Quality repair cycle")
        r4_spec = create_spec(r4_action, VLOG, git_sha(VLOG),
                              "Implement category filtering with a no-results state. Keep protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", r1_checks,
                              [{"id": "negative", "kind": "negative", "description": "data regression",
                                "command": ["node", "--input-type=module", "-e",
                                            "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                "expectExit": 0, "timeoutSecs": 60}])
        r4_campaign = start_campaign(r4_spec["id"], repairs=1, max_attempts=4)
        r4_final = wait_campaign(r4_campaign["id"], timeout=420)
        r4_candidates = call("GET", f"/api/v1/candidates?actionId={r4_action['id']}")[1]["candidates"]
        r4_first = next((c for c in r4_candidates if c["state"] == "superseded"), None)
        r4_second = next((c for c in r4_candidates if c["state"] == "frozen"), None)
        check("R4 genuine acceptance failure creates a new candidate and fresh evidence",
              r4_final["campaign"]["status"] == "accepted" and len(r4_candidates) == 2 and
              r4_first is not None and r4_second is not None and
              r4_first["commitSha"] != r4_second["commitSha"] and
              r4_second.get("parentCandidateId") == r4_first["id"],
              json.dumps([(c["state"], c["commitSha"]) for c in r4_candidates]))
        clear_sequences()
        set_stub("good")

        # R5: exhausted implementer budget stops with a truthful reason.
        set_sequence("vlog", ["empty", "empty"])
        r5_action = create_action("vlog", "Exhausted implementer budget")
        r5_spec = create_spec(r5_action, VLOG, git_sha(VLOG),
                              "Implement category filtering. Keep protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", r1_checks,
                              [{"id": "negative", "kind": "negative", "description": "data regression",
                                "command": ["node", "--input-type=module", "-e",
                                            "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                "expectExit": 0, "timeoutSecs": 60}])
        r5_campaign = start_campaign(r5_spec["id"], repairs=0, max_attempts=2)
        r5_final = wait_campaign(r5_campaign["id"], timeout=300)
        check("R5 exhausted budget terminates with a truthful inspectable reason",
              r5_final["campaign"]["status"] == "failed" and
              "attempt budget exhausted" in (r5_final["campaign"].get("terminalReason") or "").lower() and
              r5_final["budget"]["attemptsUsed"] == 2 and len(r5_final.get("recoveries") or []) == 1,
              json.dumps(r5_final["campaign"].get("terminalReason")))
        clear_sequences()
        set_stub("good")

        # R6: protected oracle modification is not retried.
        set_sequence("vlog", ["outside-scope"])
        r6_action = create_action("vlog", "Protected oracle modification")
        r6_spec = create_spec(r6_action, VLOG, git_sha(VLOG),
                              "Implement category filtering. Keep protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", r1_checks,
                              [{"id": "negative", "kind": "negative", "description": "data regression",
                                "command": ["node", "--input-type=module", "-e",
                                            "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                "expectExit": 0, "timeoutSecs": 60}])
        r6_campaign = start_campaign(r6_spec["id"], repairs=0, max_attempts=4)
        r6_final = wait_campaign(r6_campaign["id"], timeout=180)
        check("R6 protected/unauthorized modification remains blocked without retry",
              r6_final["campaign"]["status"] == "blocked" and
              "POLICY_OR_SECURITY_BLOCKED" in (r6_final["campaign"].get("terminalReason") or "") and
              len(r6_final.get("recoveries") or []) == 0,
              json.dumps(r6_final["campaign"].get("terminalReason")))
        clear_sequences()
        set_stub("good")

        # R7: an oracle defect is corrected only by a new frozen spec; old
        # evidence is never rewritten.
        bad_checks = json.loads(json.dumps(b_checks))
        for check_item in bad_checks:
            if check_item["id"] == "functional":
                check_item["command"] = ["node", "-e", "process.exit(1)"]
        r7_action = create_action("calculator", "Oracle defect correction")
        r7_bad = create_spec(r7_action, CALC, git_sha(CALC),
                             "Correct division by zero. Keep protected files unchanged.",
                             ["src/calc.js", "src/app.js"], "node-basic", bad_checks,
                             [{"id": "negative", "kind": "negative", "description": "integer regression",
                               "command": b_negative.split("\n"), "expectExit": 0, "timeoutSecs": 60}])
        r7_campaign = start_campaign(r7_bad["id"], repairs=0, max_attempts=1)
        r7_first = wait_campaign(r7_campaign["id"], timeout=300)
        r7_bundle_before = call("GET", f"/api/v1/candidates/{r7_first['candidate']['id']}/evidence")[1]["bundle"] if r7_first.get("candidate") else None
        r7_evidence_before = (
            [v["id"] for v in r7_bundle_before.get("verifications", [])],
            [r["id"] for r in r7_bundle_before.get("reviews", [])],
            (r7_bundle_before.get("decision") or {}).get("id"),
            (r7_bundle_before.get("decision") or {}).get("status"),
        ) if r7_bundle_before else None
        r7_good = create_spec(r7_action, CALC, git_sha(CALC),
                              "Correct division by zero. Keep protected files unchanged.",
                              ["src/calc.js", "src/app.js"], "node-basic", b_checks,
                              [{"id": "negative", "kind": "negative", "description": "integer regression",
                                "command": b_negative.split("\n"), "expectExit": 0, "timeoutSecs": 60}])
        r7_campaign2 = start_campaign(r7_good["id"], repairs=0, max_attempts=1)
        r7_second = wait_campaign(r7_campaign2["id"], timeout=300)
        r7_bundle_after = call("GET", f"/api/v1/candidates/{r7_first['candidate']['id']}/evidence")[1]["bundle"] if r7_first.get("candidate") else None
        r7_evidence_after = (
            [v["id"] for v in r7_bundle_after.get("verifications", [])],
            [r["id"] for r in r7_bundle_after.get("reviews", [])],
            (r7_bundle_after.get("decision") or {}).get("id"),
            (r7_bundle_after.get("decision") or {}).get("status"),
        ) if r7_bundle_after else None
        check("R7 oracle correction requires a new frozen specification and preserves old evidence",
              r7_bad["id"] != r7_good["id"] and
              r7_bad["contentHash"] != r7_good["contentHash"] and
              r7_second["campaign"]["status"] == "accepted" and
              r7_first["campaign"]["status"] == "repair_required" and
              r7_evidence_before == r7_evidence_after and
              r7_bundle_before["candidate"]["commitSha"] == r7_bundle_after["candidate"]["commitSha"],
              json.dumps({"bad": r7_bad["contentHash"], "good": r7_good["contentHash"]}))

        # D1: a productive Implementer that omits browser use receives a
        # same-session capability continuation and completes it without a
        # new Run or paid session.
        clear_sequences()
        set_stub("omit-browser")
        d1_action = create_action("vlog", "Same-session browser completion")
        d1_spec = create_spec(d1_action, VLOG, git_sha(VLOG),
                              "Implement category filtering. Keep protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", r1_checks,
                              [{"id": "negative", "kind": "negative", "description": "data regression",
                                "command": ["node", "--input-type=module", "-e",
                                            "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                "expectExit": 0, "timeoutSecs": 60}])
        d1_campaign = start_campaign(d1_spec["id"], repairs=0, max_attempts=1)
        d1_final = wait_campaign(d1_campaign["id"], timeout=300)
        d1_run = run(d1_campaign["id"])
        d1_result = (d1_run or {}).get("result") or {}
        check("D1 productive Implementer without browser receives same-session continuation",
              d1_final["campaign"]["status"] == "accepted" and
              d1_result.get("capabilityNudges", 0) >= 1 and
              "open" in (d1_result.get("browserSuccessfulCommands") or "") and
              "snapshot" in (d1_result.get("browserSuccessfulCommands") or "") and
              d1_final["budget"]["attemptsUsed"] == 1 and len(d1_final.get("recoveries") or []) == 0,
              json.dumps({"status": d1_final["campaign"]["status"],
                          "capabilityNudges": d1_result.get("capabilityNudges"),
                          "commands": d1_result.get("browserSuccessfulCommands")}))
        set_stub("good")

        # D2: a failed browser command is never reported as success.
        clear_sequences()
        set_stub("browser-fails")
        d2_action = create_action("vlog", "Failed browser command is not success")
        d2_spec = create_spec(d2_action, VLOG, git_sha(VLOG),
                              "Implement category filtering. Keep protected files unchanged.",
                              ["src/filter.js", "src/app.js"], "node-browser", r1_checks,
                              [{"id": "negative", "kind": "negative", "description": "data regression",
                                "command": ["node", "--input-type=module", "-e",
                                            "import {entries} from './src/data.js'; if(entries.length!==5)process.exit(1)"],
                                "expectExit": 0, "timeoutSecs": 60}])
        d2_campaign = start_campaign(d2_spec["id"], repairs=0, max_attempts=1)
        d2_final = wait_campaign(d2_campaign["id"], timeout=300)
        d2_bundle = call("GET", f"/api/v1/candidates/{d2_final['candidate']['id']}/evidence")[1]["bundle"] if d2_final.get("candidate") else {}
        d2_decision = d2_bundle.get("decision") or {}
        d2_run = run(d2_campaign["id"])
        d2_result = (d2_run or {}).get("result") or {}
        check("D2 failed browser commands cannot be accepted as capability success",
              d2_final["campaign"]["status"] == "repair_required" and
              d2_decision.get("status") != "ACCEPTED" and
              "open" not in (d2_result.get("browserSuccessfulCommands") or "") and
              d2_result.get("capabilityNudges", 0) >= 1,
              json.dumps({"status": d2_final["campaign"]["status"],
                          "successful": d2_result.get("browserSuccessfulCommands"),
                          "capabilityNudges": d2_result.get("capabilityNudges")}))
        set_stub("good")

        result.update({
            "projectA": "vlog", "projectB": "calculator",
            "actionA": a_action_id, "specA": a_spec_id, "campaignA": a_campaign_id,
            "candidateA": a_candidate.get("commitSha"), "baseA": git_sha(VLOG),
            "actionB": b_action["id"], "specB": b_spec["id"], "campaignB": b_campaign["id"],
            "candidateB": b_candidate.get("commitSha"), "baseB": b_base,
            "blockedCampaign": no_campaign["id"], "browserFailureCampaign": broken_campaign["id"],
            "cancelCampaign": cancel_campaign["id"], "restartCampaign": restart_campaign["id"],
            "concurrentCampaigns": [conc_a["id"], conc_b["id"]],
            "recoveryCampaign": r1_campaign["id"],
            "sameSessionCapabilityCampaign": d1_campaign["id"],
            "failedBrowserCommandCampaign": d2_campaign["id"],
            "recoveryCampaigns": {
                "recoverable": r1_campaign["id"],
                "qualityRepair": r4_campaign["id"],
                "budgetExhausted": r5_campaign["id"],
                "securityBlocked": r6_campaign["id"],
                "oracleRevisionOld": r7_campaign["id"],
                "oracleRevisionNew": r7_campaign2["id"],
            },
            "journey": journey_result,
        })
    finally:
        processes.stop()
        result["failures"] = FAILURES
        result["checks"] = CHECKS
        (EVIDENCE / "s4c-deterministic.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
        print("\nEvidence:", EVIDENCE / "s4c-deterministic.json", flush=True)
    if FAILURES:
        print(f"\n{len(FAILURES)} FAILURE(S): {FAILURES}", flush=True)
        return 1
    print("\nF1-S4-C deterministic acceptance: ALL CHECKS PASSED", flush=True)
    print("TRUSTED_LOCAL only; not isolated; simulated agent decision-making.", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
