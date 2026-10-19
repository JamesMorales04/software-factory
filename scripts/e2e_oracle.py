#!/usr/bin/env python3
"""S1 independent acceptance oracle (stdlib only).

Usage: e2e_oracle.py BASE_URL TOKEN [--skip-sse] [--expect-actions N] [--expect-events N]

Exercises the frozen S1 contracts against a running Core and exits nonzero
on the first violation. A separate process (scripts/e2e-s1.sh) owns Core
lifecycle (start/stop/restart); this oracle only speaks HTTP.
"""
import json
import os
import queue
import sys
import threading
import time
import urllib.request
import urllib.error

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8471"
TOKEN = sys.argv[2] if len(sys.argv) > 2 else ""
ARGS = sys.argv[3:]
FAILURES = []
# Unique per oracle run: pass 2 must not replay pass 1's keys.
RUN_KEY = f"e2e-key-{int(time.time()*1000)}"
RESTART_MODE = "--expect-actions" in ARGS


def check(name, cond, detail=""):
    status = "PASS" if cond else "FAIL"
    print(f"[{status}] {name}" + (f" — {detail}" if detail and not cond else ""), flush=True)
    if not cond:
        FAILURES.append(name)


def call(method, path, body=None, token=TOKEN, extra_headers=None):
    url = BASE + path
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Content-Type": "application/json",
               "X-Correlation-ID": f"e2e-{int(time.time()*1000)}"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if extra_headers:
        headers.update(extra_headers)
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=15) as res:
            raw = res.read().decode()
            return res.status, dict(res.headers), json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        raw = e.read().decode()
        try:
            return e.code, dict(e.headers), json.loads(raw) if raw else None
        except json.JSONDecodeError:
            return e.code, dict(e.headers), {"raw": raw}


def sse_watch(after, wanted_title, timeout=15):
    """Subscribe, then report whether an ActionCreated for wanted_title arrives."""
    found = queue.Queue()

    def run():
        try:
            req = urllib.request.Request(
                f"{BASE}/api/v1/events/stream?after={after}",
                headers={"Authorization": f"Bearer {TOKEN}",
                         "X-Correlation-ID": "e2e-sse"},
                method="GET")
            with urllib.request.urlopen(req, timeout=timeout) as res:
                buf = ""
                start = time.time()
                while time.time() - start < timeout:
                    chunk = res.read(1)
                    if not chunk:
                        break
                    buf += chunk.decode(errors="replace")
                    while "\n\n" in buf:
                        frame, buf = buf.split("\n\n", 1)
                        for line in frame.split("\n"):
                            if line.startswith("data:"):
                                try:
                                    ev = json.loads(line[5:].strip())
                                except json.JSONDecodeError:
                                    continue
                                if (ev.get("eventType") == "ActionCreated"
                                        and ev.get("payload", {}).get("title") == wanted_title):
                                    found.put(ev)
                                    return
        except Exception as e:  # noqa - reported as failure below
            found.put(e)

    t = threading.Thread(target=run, daemon=True)
    t.start()
    return found


def main():
    # --- static bundle is public when served; API stays gated either way ---
    try:
        with urllib.request.urlopen(BASE + "/", timeout=15) as res:
            shell = res.read().decode(errors="replace")
            check("office bundle served without token",
                  res.status == 200 and ("Factory Office" in shell or "root" in shell),
                  f"status={res.status}")
    except urllib.error.HTTPError as e:
        if e.code == 404:
            # API-only mode (no --web-dir): bundle serving is covered by
            # the Go static test instead. Not a pass, not a failure.
            print("[SKIP] office bundle check (core runs API-only)", flush=True)
        else:
            check("office bundle served without token", False, f"status={e.code}")
    except Exception as e:
        check("office bundle served without token", False, str(e)[:200])

    # --- health & auth ---
    s, _, b = call("GET", "/api/v1/health")
    check("health 200 + version", s == 200 and (b or {}).get("version", "").startswith("0.1.0"),
          f"status={s} body={b}")

    s, _, _ = call("GET", "/api/v1/actions", token="")
    check("unauthenticated list rejected", s == 401, f"status={s}")
    s, _, _ = call("POST", "/api/v1/actions", {"title": "x"}, token="wrong-token")
    check("wrong token rejected", s == 401, f"status={s}")

    s, _, b = call("GET", "/api/v1/projects")
    projs = (b or {}).get("projects", [])
    check("default project seeded", s == 200 and any(p["id"] == "default" for p in projs),
          f"status={s} body={b}")

    # --- negatives: invalid input ---
    for name, body in [("empty title", {"title": "  "}),
                       ("unknown field", {"title": "x", "nope": 1}),
                       ("unknown project", {"title": "x", "projectId": "ghost"}),
                       ("malformed status filter", None)]:
        if body is None:
            s, _, _ = call("GET", "/api/v1/actions?status=bogus")
        else:
            s, _, _ = call("POST", "/api/v1/actions", body)
        check(f"invalid rejected: {name}", s == 400, f"status={s}")
    s, _, _ = call("GET", "/api/v1/actions/does-not-exist")
    check("missing action 404", s == 404, f"status={s}")

    # --- positive: create / list / detail / transition ---
    title = f"s1-e2e-{RUN_KEY}"
    s, headers, b = call("POST", "/api/v1/actions",
                         {"title": title, "description": "oracle probe"},
                         extra_headers={"Idempotency-Key": RUN_KEY})
    action = (b or {}).get("action", {})
    aid = action.get("id", "")
    check("create 201 + correlation echo",
          s == 201 and bool(aid) and "X-Correlation-ID" in headers
          or "x-correlation-id" in {k.lower(): v for k, v in headers.items()},
          f"status={s} body={b}")
    check("create defaults", action.get("status") == "pending"
          and action.get("projectId") == "default", f"{action}")

    s, _, b = call("GET", "/api/v1/actions")
    ids = [a["id"] for a in (b or {}).get("actions", [])]
    check("list contains created action", s == 200 and aid in ids, f"{ids}")

    s, _, b = call("GET", f"/api/v1/actions/{aid}")
    check("detail round-trips", s == 200 and (b or {}).get("action", {}).get("title") == title,
          f"status={s}")

    s, _, _ = call("POST", f"/api/v1/actions/{aid}/transition", {"to": "completed"})
    check("illegal jump pending->completed is 409", s == 409, f"status={s}")
    s, _, b = call("POST", f"/api/v1/actions/{aid}/transition", {"to": "in_progress"})
    check("legal transition 200", s == 200
          and (b or {}).get("action", {}).get("status") == "in_progress", f"status={s} {b}")

    s, _, b = call("GET", f"/api/v1/actions/{aid}/events")
    evs = (b or {}).get("events", [])
    seqs = [e["sequence"] for e in evs]
    check("aggregate replay ordered 1..2",
          s == 200 and [e["eventType"] for e in evs] == ["ActionCreated", "ActionTransitioned"]
          and seqs == [1, 2], f"{evs}")

    # --- idempotency: duplicate must not fork state ---
    s, _, b = call("GET", "/api/v1/events?after=0&limit=500")
    n_events_before = len((b or {}).get("events", []))
    s, headers, b = call("POST", "/api/v1/actions",
                          {"title": title, "description": "oracle probe"},
                          extra_headers={"Idempotency-Key": RUN_KEY})
    dup = (b or {}).get("action", {})
    replayed = headers.get("Idempotent-Replayed") or headers.get("idempotent-replayed")
    check("duplicate key replays original", s == 200 and dup.get("id") == aid
          and replayed == "true", f"status={s} {b} {headers}")
    s, _, b = call("GET", "/api/v1/actions")
    s2, _, b2 = call("GET", "/api/v1/events?after=0&limit=500")
    count_ids = [a["id"] for a in (b or {}).get("actions", [])].count(aid)
    check("no duplicate action/event",
          count_ids == 1 and len((b2 or {}).get("events", [])) == n_events_before,
          f"actions={b} events={len((b2 or {}).get('events', []))} before={n_events_before}")

    # --- global replay cursor ---
    s, _, b = call("GET", "/api/v1/events?after=0&limit=500")
    all_evs = (b or {}).get("events", [])
    gseqs = [e["globalSeq"] for e in all_evs]
    check("global order contiguous from 1",
          s == 200 and gseqs == list(range(1, len(gseqs) + 1)), f"{gseqs}")
    if gseqs:
        s, _, b = call("GET", f"/api/v1/events?after={gseqs[0]}&limit=500")
        resume = [e["globalSeq"] for e in (b or {}).get("events", [])]
        check("cursor resume returns exact suffix", resume == gseqs[1:], f"{resume}")

    # --- live SSE tail (skip when asked, e.g. offline lanes) ---
    if "--skip-sse" not in ARGS:
        cursor = gseqs[-1] if gseqs else 0
        live_title = f"s1-live-{int(time.time())}"
        found = sse_watch(cursor, live_title)
        time.sleep(0.5)
        s, _, _ = call("POST", "/api/v1/actions", {"title": live_title})
        try:
            got = found.get(timeout=14)
            check("live stream delivers new ActionCreated",
                  isinstance(got, dict) and got.get("globalSeq", 0) > cursor, f"{got}")
        except queue.Empty:
            check("live stream delivers new ActionCreated", False, "timeout waiting for frame")

    # --- restart expectations (same DB, second oracle pass) ---
    if RESTART_MODE:
        i = ARGS.index("--expect-actions")
        retained = int(ARGS[i + 1])
        s, _, b = call("GET", "/api/v1/actions")
        got_actions = (b or {}).get("actions", [])
        check(f"restart retains pre-existing + new actions",
              s == 200 and len(got_actions) == retained, f"{got_actions}")
        ok = True
        for a in got_actions:
            s, _, b = call("GET", f"/api/v1/actions/{a['id']}/events")
            seqs = [e["sequence"] for e in (b or {}).get("events", [])]
            if seqs != list(range(1, len(seqs) + 1)):
                ok = False
        check("restart retains contiguous per-aggregate history", ok)
        s, _, b = call("GET", "/api/v1/events?after=0&limit=500")
        gs = [e["globalSeq"] for e in (b or {}).get("events", [])]
        check("restart keeps gapless global ledger",
              gs == list(range(1, len(gs) + 1)), f"{gs}")
        boots = [e for e in (b or {}).get("events", [])
                 if e.get("eventType") == "CoreStarted"]
        check("restart appended a CoreStarted marker", len(boots) >= 2, f"{len(boots)} boots")

    print(f"\n{len(FAILURES)} failure(s).", flush=True)
    return 1 if FAILURES else 0


if __name__ == "__main__":
    sys.exit(main())
