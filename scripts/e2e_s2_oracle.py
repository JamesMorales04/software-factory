#!/usr/bin/env python3
"""F1-S2 end-to-end oracle: real Core + real Worker over real HTTP.

Stages (run by scripts/e2e-s2.sh against a live Core):
  stage-positive  queue a synthetic Run, wait for the Worker's terminal
                  report, verify Run/Attempt/event lineage and ledger replay.
  stage-fencing   stale-generation, duplicate-replay, unknown-identity and
                  cancel-race negatives over the same live Core.
  stage-history   verify Runs/Attempts/Workers survive and reconcile.

Exits nonzero on the first failing check; every check prints PASS/FAIL.
"""
import json
import sys
import time
import urllib.request

BASE = sys.argv[1]
API_TOKEN = sys.argv[2]
WORKER_TOKEN = sys.argv[3]
STAGE = sys.argv[4] if len(sys.argv) > 4 else "stage-positive"

FAILURES = []


def check(name, cond, detail=""):
    print(f"[{'PASS' if cond else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))
    if not cond:
        FAILURES.append(name)


def call(method, path, body=None, token=None, expect=(200,)):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={
            "Authorization": f"Bearer {token or API_TOKEN}",
            "Content-Type": "application/json",
            "X-Correlation-ID": f"s2-oracle-{time.time_ns()}",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=20) as res:
            payload = json.loads(res.read().decode() or "{}")
            return res.status, payload, res.headers
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            return e.code, json.loads(raw or "{}"), e.headers
        except json.JSONDecodeError:
            return e.code, {"raw": raw}, e.headers


def wait_terminal(run_id, timeout=90):
    deadline = time.time() + timeout
    while time.time() < deadline:
        s, body, _ = call("GET", f"/api/v1/runs/{run_id}")
        if s == 200 and body["run"]["status"] in ("succeeded", "failed", "cancelled"):
            return body["run"]
        time.sleep(1)
    return None


def stage_positive():
    s, body, _ = call("POST", "/api/v1/runs", {
        "workloadKind": "synthetic-echo",
        "workloadInput": {"message": "s2-proof", "steps": 2, "stepMs": 5},
    })
    check("queue synthetic run 201", s == 201, f"status={s}")
    if s != 201:
        return
    run = body["run"]
    check("run queued durably", run["status"] == "queued" and len(run["id"]) == 32)
    check("attempt budget recorded", run["maxAttempts"] == 3 and run["attemptCount"] == 0)

    final = wait_terminal(run["id"])
    check("worker completed the run", final is not None and final["status"] == "succeeded",
          f"status={final['status'] if final else 'TIMEOUT'}")
    if not final:
        return
    check("bounded result receipt", (final.get("result") or {}).get("receipt", "").count("/") == 2,
          json.dumps(final.get("result")))
    check("single attempt on happy path", final["attemptCount"] == 1, f"attempts={final['attemptCount']}")

    s, body, _ = call("GET", f"/api/v1/runs/{run['id']}/attempts")
    attempts = body.get("attempts", []) if s == 200 else []
    check("attempt history preserved", s == 200 and len(attempts) == 1 and attempts[0]["status"] == "succeeded")
    if attempts:
        check("fencing generation is 1", attempts[0]["leaseGeneration"] == 1)
        check("heartbeats observed", attempts[0]["heartbeats"] >= 1, f"hb={attempts[0]['heartbeats']}")

    s, body, _ = call("GET", f"/api/v1/runs/{run['id']}/events")
    types = [e["eventType"] for e in body.get("events", [])] if s == 200 else []
    check("run event lineage complete",
          types == ["RunQueued", "RunLeased", "RunStarted", "RunCompleted"], f"{types}")

    # Ledger replay from cursor 0 must contain the run lineage in order.
    s, body, _ = call("GET", "/api/v1/events?after=0&limit=500")
    seqs = [e["globalSeq"] for e in body.get("events", [])] if s == 200 else []
    check("ledger replay ordered", s == 200 and seqs == sorted(seqs) and len(seqs) >= 4)

    s, body, _ = call("GET", "/api/v1/workers")
    workers = body.get("workers", []) if s == 200 else []
    check("worker observed online", s == 200 and any(w["status"] == "online" for w in workers),
          json.dumps(workers))

    # Print the run id for later stages.
    print(f"RUN_ID={run['id']}")
    with open("/tmp/factory-s2-run.env", "w") as f:
        f.write(f"S2_RUN_ID={run['id']}\n")


def stage_fencing():
    # Fresh run for the negative battery.
    s, body, _ = call("POST", "/api/v1/runs", {
        "workloadKind": "synthetic-echo",
        "workloadInput": {"message": "fence-proof", "steps": 1},
    })
    check("fencing run queued", s == 201, f"status={s}")
    run_id = body["run"]["id"]

    def wcall(method, path, payload):
        return call(method, path, payload, token=WORKER_TOKEN)

    final = wait_terminal(run_id)
    check("fencing run terminal", final is not None, f"status={final['status'] if final else 'TIMEOUT'}")
    if not final:
        return
    s, body, _ = call("GET", f"/api/v1/runs/{run_id}/attempts")
    attempt = body["attempts"][-1]
    gen = attempt["leaseGeneration"]
    report = {"workerId": attempt["workerId"], "runId": run_id,
              "attemptId": attempt["id"], "generation": gen, "outcome": "completed",
              "output": attempt.get("result") or {}}

    # Duplicate delivery replays without a new event.
    s0, b0, _ = call("GET", "/api/v1/events?after=0&limit=500")
    n0 = len(b0.get("events", []))
    s, body, headers = wcall("POST", "/api/v1/workers/report", report)
    check("duplicate report replays 200", s == 200, f"status={s}")
    s1, b1, _ = call("GET", "/api/v1/events?after=0&limit=500")
    check("duplicate adds no event", len(b1.get("events", [])) == n0)

    # Stale generation rejected.
    stale = dict(report, generation=gen + 99)
    s, body, _ = wcall("POST", "/api/v1/workers/report", stale)
    check("stale generation rejected", s == 409 and body.get("error", {}).get("code") == "stale_generation",
          f"status={s} body={body}")

    # Unknown identities rejected.
    s, body, _ = wcall("POST", "/api/v1/workers/poll", {"workerId": "ghost-worker"})
    check("unknown worker rejected", s == 404, f"status={s}")
    s, body, _ = wcall("POST", "/api/v1/workers/heartbeat",
                       {"workerId": attempt["workerId"], "runId": "0" * 32,
                        "attemptId": attempt["id"], "generation": gen})
    check("unknown run rejected", s == 404, f"status={s} body={body}")

    # Scoped auth: operator token on worker routes, worker token on operator routes.
    s, body, _ = call("POST", "/api/v1/workers/poll", {"workerId": "local-1"})
    check("operator token rejected on worker route", s == 401, f"status={s}")
    s, body, _ = call("POST", "/api/v1/runs", {"workloadKind": "synthetic-echo",
                                               "workloadInput": {"message": "x"}}, token=WORKER_TOKEN)
    check("worker token rejected on operator route", s == 401, f"status={s}")

    # Cancel racing completion: queue, cancel while queued, prove terminal wins.
    s, body, _ = call("POST", "/api/v1/runs", {
        "workloadKind": "synthetic-echo",
        "workloadInput": {"message": "cancel-race", "steps": 10, "stepMs": 200},
        "maxAttempts": 1,
    })
    race_id = body["run"]["id"] if s == 201 else None
    check("race run queued", s == 201, f"status={s}")
    if race_id:
        s, body, _ = call("POST", f"/api/v1/runs/{race_id}/cancel", {})
        check("cancel accepted", s == 200 and body["run"]["status"] == "cancelled", f"status={s}")
        s, body, _ = call("GET", f"/api/v1/runs/{race_id}")
        check("cancel is authoritative", s == 200 and body["run"]["status"] == "cancelled")


def stage_history(expected_min_runs=1):
    s, body, _ = call("GET", "/api/v1/runs?limit=200")
    runs = body.get("runs", []) if s == 200 else []
    check("history survives restart", s == 200 and len(runs) >= expected_min_runs, f"runs={len(runs)}")
    for r in runs:
        if r["status"] in ("leased", "running"):
            check(f"no unfinished run leaks ({r['id'][:8]})", False, f"status={r['status']}")
            return
    check("no unfinished runs leak after recovery", True)
    s, body, _ = call("GET", "/api/v1/workers")
    check("workers endpoint reachable", s == 200, f"status={s}")


if __name__ == "__main__":
    import urllib.error  # noqa: E402  (deferred so stage import errors surface first)
    {"stage-positive": stage_positive, "stage-fencing": stage_fencing,
     "stage-history": stage_history}[STAGE]()
    if FAILURES:
        print(f"\n{len(FAILURES)} check(s) FAILED: {FAILURES}")
        sys.exit(1)
    print("\nAll S2 oracle checks passed.")
