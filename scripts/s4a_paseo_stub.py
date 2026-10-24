#!/usr/bin/env python3
"""Deterministic Paseo CLI simulator for F1-S4-A acceptance.

No daemon and no provider calls are made. The simulator performs a real file
edit in the workspace the Factory Worker cloned from the authorized fixture
repository, so the Worker's commit/push/Core-validation path is exercised
end to end. Behavior is selected by the file in $S4A_STUB_DIR/mode:
  good          - add a correct multiply export
  failing       - add a multiply export with wrong behavior
  outside-scope - modify only a file outside the frozen allowed scope
  empty         - perform no source change
"""
import json
import os
import sys
import time
from pathlib import Path

STATE_DIR = Path(os.environ.get("S4A_STUB_DIR", "."))
STATE = STATE_DIR / "state.json"


def load_state():
    if STATE.exists():
        return json.loads(STATE.read_text(encoding="utf-8"))
    return {"workspaces": {}, "sessions": {}, "counter": 0}


def save_state(state):
    STATE.write_text(json.dumps(state), encoding="utf-8")


def mode():
    path = STATE_DIR / "mode"
    try:
        return path.read_text(encoding="utf-8").strip() or "good"
    except OSError:
        return "good"


def next_mode():
    """Consume one sequence entry when present, else use the static mode.

    S4-B uses the sequence to drive a deterministic repair cycle
    (failing first candidate, good repaired candidate) without an external
    operator switching modes mid-campaign.
    """
    path = STATE_DIR / "sequence"
    try:
        lines = [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    except OSError:
        return mode()
    if not lines:
        return mode()
    current = lines.pop(0)
    if lines:
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    else:
        path.unlink(missing_ok=True)
    return current


def delay_seconds():
    try:
        return max(0.0, min(30.0, float((STATE_DIR / "delay").read_text(encoding="utf-8").strip())))
    except (OSError, ValueError):
        return 0.0


def next_id(state, prefix):
    state["counter"] = int(state.get("counter", 0)) + 1
    return f"{prefix}_{state['counter']}"


def parse_flags(args, names):
    values = {}
    rest = []
    i = 0
    while i < len(args):
        arg = args[i]
        if arg in names:
            if i + 1 >= len(args):
                raise SystemExit(f"missing value for {arg}")
            values[arg] = args[i + 1]
            i += 2
        elif arg.startswith("--") and "=" in arg:
            key, value = arg.split("=", 1)
            values[key] = value
            i += 1
        else:
            rest.append(arg)
            i += 1
    return values, rest


def edit_add(project, body):
    add = project / "add.js"
    text = add.read_text(encoding="utf-8")
    if "multiply" not in text:
        add.write_text(text + "\n" + body, encoding="utf-8")


def edit_app(project, correct):
    app = project / "app.js"
    if not app.exists():
        return
    text = app.read_text(encoding="utf-8")
    if "function multiply" in text:
        return
    multiply = """/** Multiply two numbers. */
function multiply(a, b) {
  return a * b;
}

"""
    text = text.replace("function compute(a, b, op) {", multiply + "function compute(a, b, op) {")
    if correct:
        text = text.replace(
            "function compute(a, b, op) {\n  return add(a, b);\n}",
            'function compute(a, b, op) {\n  if (op === "multiply") return multiply(a, b);\n  return add(a, b);\n}',
        )
    app.write_text(text, encoding="utf-8")


def edit(workspace_path, current):
    project = Path(workspace_path)
    correct = "/** Multiply two numbers. */\nfunction multiply(a, b) {\n  return a * b;\n}\nmodule.exports.multiply = multiply;"
    wrong = "/** Multiply two numbers. */\nfunction multiply(a, b) {\n  return a + b;\n}\nmodule.exports.multiply = multiply;"
    if current == "good":
        edit_add(project, correct)
        edit_app(project, True)
    elif current == "failing":
        edit_add(project, wrong)
        edit_app(project, True)
    elif current == "browser-failing":
        edit_add(project, correct)
    elif current == "outside-scope":
        (project / "outside.txt").write_text("outside the frozen scope\n", encoding="utf-8")
    elif current == "critical":
        target = project / "core" / "internal" / "quality" / "policy.txt"
        target.parent.mkdir(parents=True, exist_ok=True)
        text = target.read_text(encoding="utf-8") if target.exists() else "protected policy fixture\n"
        target.write_text(text + "\nsimulated protected change\n", encoding="utf-8")
    elif current == "empty":
        return
    else:
        raise SystemExit(f"unknown S4-A stub mode {current}")


def main(argv):
    if not argv:
        return 3
    if argv[0] == "--version":
        print("paseo-stub 0.0.0-s4a")
        return 0
    state = load_state()
    if argv[0] == "workspace":
        if len(argv) < 2:
            return 3
        if argv[1] == "create":
            flags, _ = parse_flags(argv[2:], {"--path", "--title", "--isolation"})
            path = flags.get("--path")
            if not path:
                return 3
            workspace_id = next_id(state, "ws")
            state["workspaces"][workspace_id] = path
            save_state(state)
            print(json.dumps({"workspaceId": workspace_id}))
            return 0
        if argv[1] == "archive":
            if len(argv) > 2:
                state["workspaces"].pop(argv[2], None)
                save_state(state)
            return 0
        if argv[1] == "ls":
            print("[]")
            return 0
        return 3
    if argv[0] == "run":
        flags, rest = parse_flags(
            argv[1:],
            {"--provider", "--workspace", "--mode", "--model", "--wait-timeout", "--title", "--label"},
        )
        workspace_id = flags.get("--workspace")
        workspace_path = state["workspaces"].get(workspace_id)
        if not workspace_path:
            print("stub: unknown workspace", file=sys.stderr)
            return 1
        session_id = next_id(state, "sess")
        state["sessions"][session_id] = {
            "workspace": workspace_id,
            "workspacePath": workspace_path,
            "provider": flags.get("--provider", ""),
            "model": flags.get("--model", ""),
            "mode": flags.get("--mode", ""),
            "prompt": rest[-1] if rest else "",
        }
        save_state(state)
        if (pause := delay_seconds()) > 0:
            time.sleep(pause)
        edit(workspace_path, next_mode())
        print(json.dumps({"id": session_id}))
        return 0
    if argv[0] == "inspect":
        if len(argv) < 2:
            return 3
        session = state["sessions"].get(argv[1], {})
        if not session:
            return 1
        print(
            json.dumps(
                {
                    "Id": argv[1],
                    "Provider": session.get("provider", ""),
                    "Model": session.get("model", ""),
                    "Mode": session.get("mode", ""),
                    "Cwd": session.get("workspacePath", ""),
                    "Status": "idle",
                    "Archived": False,
                }
            )
        )
        return 0
    if argv[0] == "logs":
        print("[Read] add.js\n[Edit] add.js\n[Shell] npm test\n[Text] done")
        return 0
    if argv[0] in {"wait", "send", "stop", "archive"}:
        return 0
    if argv[0] == "ls":
        print("[]")
        return 0
    return 3


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
