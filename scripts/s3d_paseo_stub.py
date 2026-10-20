#!/usr/bin/env python3
"""Deterministic Paseo 0.9.1 CLI simulator for S3-D recovery tests.

No daemon, provider or network is used. The simulator implements only the
surface the Factory adapter uses: workspace create/ls/archive, run with
labels/title, inspect, ls with exact label filters, logs, stop, archive and
wait. State is a JSON file under S3D_STUB_DIR guarded by an flock.

Modes (S3D_STUB_MODE):
  ok                      normal success behavior
  fail-once               first `run` fails before a session exists
  mismatch                inspect reports a different provider/model/mode
  wrong-title             inspect reports a non-Factory title
  hang-logs               `logs` blocks until the parent Worker dies
  hang-run                `run` blocks until the parent Worker dies
  kill-parent-after-create  create the session, SIGKILL the Worker parent,
                            then exit (crash before bind)
The `daemon` file in S3D_STUB_DIR may contain `down` to make every Paseo
operation fail, simulating a daemon disconnect without touching the owner's
real daemon.
"""
import fcntl
import json
import os
import signal
import sys
import time
import uuid

STUB_DIR = os.environ.get("S3D_STUB_DIR")
if not STUB_DIR:
    print("S3D_STUB_DIR is required", file=sys.stderr)
    sys.exit(2)
STATE_PATH = os.path.join(STUB_DIR, "state.json")
LOCK_PATH = os.path.join(STUB_DIR, "state.lock")
CALLS_PATH = os.path.join(STUB_DIR, "calls.log")
MODE_PATH = os.path.join(STUB_DIR, "mode")
DAEMON_PATH = os.path.join(STUB_DIR, "daemon")


def mode():
    try:
        with open(MODE_PATH, encoding="utf-8") as handle:
            return handle.read().strip() or "ok"
    except FileNotFoundError:
        return "ok"


def daemon_down():
    try:
        with open(DAEMON_PATH, encoding="utf-8") as handle:
            return handle.read().strip() == "down"
    except FileNotFoundError:
        return False


def fail_if_down():
    if daemon_down():
        print("simulated daemon unavailable", file=sys.stderr)
        sys.exit(1)


def record_call(args):
    with open(CALLS_PATH, "a", encoding="utf-8") as handle:
        handle.write(" ".join(args) + "\n")


def hang_until_parent_dies(seconds=120):
    deadline = time.time() + seconds
    while time.time() < deadline:
        if os.getppid() == 1:
            return
        time.sleep(0.2)


class State:
    def __init__(self):
        os.makedirs(STUB_DIR, exist_ok=True)
        self.lock = open(LOCK_PATH, "a+", encoding="utf-8")
        fcntl.flock(self.lock, fcntl.LOCK_EX)
        try:
            with open(STATE_PATH, encoding="utf-8") as handle:
                self.data = json.load(handle)
        except FileNotFoundError:
            self.data = {"workspace_seq": 0, "session_seq": 0,
                         "workspaces": {}, "sessions": {}}
        self.data.setdefault("workspaces", {})
        self.data.setdefault("sessions", {})

    def save(self):
        tmp = STATE_PATH + ".tmp"
        with open(tmp, "w", encoding="utf-8") as handle:
            json.dump(self.data, handle)
        os.replace(tmp, STATE_PATH)
        fcntl.flock(self.lock, fcntl.LOCK_UN)
        self.lock.close()


def take_value(args, index):
    if index + 1 >= len(args):
        return "", index + 1
    return args[index + 1], index + 2


def parse_flags(args):
    flags = {"labels": []}
    index = 0
    rest = []
    while index < len(args):
        arg = args[index]
        if arg == "--label":
            value, index = take_value(args, index)
            flags["labels"].append(value)
            continue
        if arg in ("--provider", "--model", "--mode", "--workspace", "--title",
                   "--path", "--isolation", "--project", "--wait-timeout",
                   "--worktree-mode", "--worktree-slug", "--new-branch",
                   "--base", "--branch", "--pr-number", "--forge"):
            value, index = take_value(args, index)
            flags[arg[2:]] = value
            continue
        if arg == "--json":
            index += 1
            continue
        rest.append(arg)
        index += 1
    return flags, rest


def cmd_workspace(args):
    fail_if_down()
    if not args:
        sys.exit(3)
    sub = args[0]
    flags, _ = parse_flags(args[1:])
    state = State()
    if sub == "create":
        state.data["workspace_seq"] += 1
        wid = f"ws_s3d_{state.data['workspace_seq']}"
        state.data["workspaces"][wid] = {
            "id": wid,
            "title": flags.get("title", ""),
            "path": flags.get("path", ""),
            "isolation": flags.get("isolation", "local"),
            "archived": False,
        }
        state.save()
        print(json.dumps({"workspaceId": wid}))
        return
    if sub == "ls":
        out = [{"workspaceId": ws["id"], "name": ws["title"],
                "cwd": ws["path"], "isolation": ws["isolation"]}
               for ws in state.data["workspaces"].values() if not ws["archived"]]
        state.save()
        print(json.dumps(out))
        return
    if sub == "archive":
        wid = args[1] if len(args) > 1 else ""
        if mode() == "slow-cleanup":
            time.sleep(3)
        record_call(["workspace-archive", wid])
        ws = state.data["workspaces"].get(wid)
        if not ws:
            state.save()
            print("workspace not found", file=sys.stderr)
            sys.exit(1)
        ws["archived"] = True
        for session in state.data["sessions"].values():
            if session["workspaceId"] == wid:
                session["archived"] = True
                session["status"] = "idle"
        state.save()
        return
    state.save()
    sys.exit(3)


def cmd_run(args):
    fail_if_down()
    flags, rest = parse_flags(args)
    labels = {}
    for raw in flags.get("labels", []):
        if "=" in raw:
            key, value = raw.split("=", 1)
            labels[key] = value
    state = State()
    wid = flags.get("workspace", "")
    ws = state.data["workspaces"].get(wid)
    if not ws or ws["archived"]:
        state.save()
        print("workspace unavailable", file=sys.stderr)
        sys.exit(1)
    if mode() == "fail-once" and not state.data.get("failed_once"):
        state.data["failed_once"] = True
        state.save()
        print("simulated provider failure", file=sys.stderr)
        sys.exit(1)
    state.data["session_seq"] += 1
    sid = str(uuid.uuid4())
    state.data["sessions"][sid] = {
        "id": sid,
        "title": flags.get("title", ""),
        "labels": labels,
        "provider": flags.get("provider", ""),
        "model": flags.get("model", ""),
        "mode": flags.get("mode", ""),
        "workspaceId": wid,
        "cwd": ws["path"],
        "status": "idle",
        "archived": False,
    }
    if mode() == "ok" and ws["path"]:
        add_js = os.path.join(ws["path"], "add.js")
        try:
            with open(add_js, encoding="utf-8") as handle:
                body = handle.read()
        except FileNotFoundError:
            body = ""
        if body and "multiply" not in body:
            with open(add_js, "a", encoding="utf-8") as handle:
                handle.write("\n/** Multiply two numbers. */\n"
                             "function multiply(a, b) {\n  return a * b;\n}\n"
                             "module.exports.multiply = multiply;\n")
    state.save()
    if mode() == "hang-run":
        hang_until_parent_dies()
        return
    if mode() == "kill-parent-after-create":
        os.kill(os.getppid(), signal.SIGKILL)
        time.sleep(0.2)
        return
    print(json.dumps({"Id": sid}))


def cmd_inspect(args):
    fail_if_down()
    sid = args[0] if args else ""
    state = State()
    session = state.data["sessions"].get(sid)
    state.save()
    if not session:
        print("agent not found", file=sys.stderr)
        sys.exit(1)
    provider = session["provider"]
    model = session["model"]
    paseo_mode = session["mode"]
    title = session["title"]
    if mode() == "mismatch":
        provider = "opencode"
        model = "opencode-go/deepseek-v4.1-flash"
        paseo_mode = "build"
    if mode() == "wrong-title":
        title = "factory-not-the-owner"
    print(json.dumps({
        "Id": session["id"],
        "Name": title,
        "Provider": provider,
        "Model": model,
        "Mode": paseo_mode,
        "Cwd": session["cwd"],
        "Status": session["status"],
        "Archived": session["archived"],
    }))


def cmd_ls(args):
    fail_if_down()
    flags, _ = parse_flags(args)
    include_archived = "-a" in args or "--all" in args
    wanted = {}
    for raw in flags.get("labels", []):
        if "=" in raw:
            key, value = raw.split("=", 1)
            wanted[key] = value
    state = State()
    out = []
    for session in state.data["sessions"].values():
        if session["archived"] and not include_archived:
            continue
        if any(session["labels"].get(k) != v for k, v in wanted.items()):
            continue
        out.append({"id": session["id"], "name": session["title"],
                    "provider": session["provider"], "status": session["status"],
                    "cwd": session["cwd"]})
    state.save()
    print(json.dumps(out))


def cmd_logs(args):
    fail_if_down()
    sid = args[0] if args else ""
    state = State()
    session = state.data["sessions"].get(sid)
    state.save()
    if not session:
        print("agent not found", file=sys.stderr)
        sys.exit(1)
    if mode() == "hang-logs":
        hang_until_parent_dies()
        return
    print("[User] task\n[Read] add.js\n[Edit] add.js\n[Shell] npm test\n[Text] done")


def cmd_session_op(op, args):
    fail_if_down()
    sid = args[0] if args else ""
    state = State()
    session = state.data["sessions"].get(sid)
    if not session:
        state.save()
        print("agent not found", file=sys.stderr)
        sys.exit(1)
    if mode() == "slow-cleanup":
        time.sleep(3)
    record_call([op, sid])
    if op == "stop":
        session["status"] = "idle"
    elif op == "archive":
        session["archived"] = True
        session["status"] = "idle"
    elif op == "send":
        pass
    elif op == "wait":
        pass
    state.save()


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(3)
    command = args[0]
    if command == "--version":
        print("paseo-stub 0.9.1-s3d")
        return
    if command == "workspace":
        cmd_workspace(args[1:])
        return
    if command == "run":
        cmd_run(args[1:])
        return
    if command == "inspect":
        cmd_inspect(args[1:])
        return
    if command == "ls":
        cmd_ls(args[1:])
        return
    if command == "logs":
        cmd_logs(args[1:])
        return
    if command in ("stop", "archive", "send", "wait"):
        # Strip options before the identity argument for send/wait.
        positional = [a for a in args[1:] if not a.startswith("--")]
        cmd_session_op(command, positional)
        return
    sys.exit(3)


if __name__ == "__main__":
    main()
