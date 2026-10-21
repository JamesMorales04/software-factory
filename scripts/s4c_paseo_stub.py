#!/usr/bin/env python3
"""Deterministic Paseo CLI simulator for F1-S4-C acceptance.

No daemon and no provider calls are made. The simulator is the stand-in for
a Paseo agent process: it edits the real workspace the Factory Worker cloned
from the registered Project, and for browser-enabled projects it invokes the
Factory-generated Playwright wrapper against an authorized loopback endpoint.
Mode is read from $S4C_STUB_DIR/mode:
  good           implement the task
  empty          leave no diff
  browser-broken implement filtering but break the no-results oracle
  outside-scope  modify a protected oracle outside the allowed scope
"""
import fcntl
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

STATE_DIR = Path(os.environ.get("S4C_STUB_DIR", "."))
STATE = STATE_DIR / "state.json"
VLOG_FILTER = '''// Category filtering implemented by the deterministic Factory simulator.
export function filterEntries(entries, category) {
  if (!category || category === "All") return entries;
  return entries.filter((entry) => entry.category === category);
}
'''
CALC = '''export function add(a, b) {
  return a + b;
}

export function subtract(a, b) {
  return a - b;
}

export function multiply(a, b) {
  return a * b;
}

export function divide(a, b) {
  if (b === 0) return null;
  return a / b;
}

export function calculate(a, b, operation) {
  switch (operation) {
    case "add":
      return add(a, b);
    case "subtract":
      return subtract(a, b);
    case "multiply":
      return multiply(a, b);
    case "divide":
      return divide(a, b);
    default:
      throw new Error(`unknown operation ${operation}`);
  }
}

export function formatResult(value) {
  if (value === null || value === undefined || Number.isNaN(value)) {
    return "Cannot divide by zero";
  }
  if (typeof value !== "number") return String(value);
  return String(Math.round(value * 1e10) / 1e10);
}
'''


def load_state():
    if STATE.exists():
        return json.loads(STATE.read_text(encoding="utf-8"))
    return {"workspaces": {}, "sessions": {}, "counter": 0}


def save_state(state):
    STATE.write_text(json.dumps(state), encoding="utf-8")


def mode():
    try:
        return (STATE_DIR / "mode").read_text(encoding="utf-8").strip() or "good"
    except OSError:
        return "good"


def sequence_for(workspace_path):
    project = Path(workspace_path)
    if (project / "src" / "calc.js").exists():
        return STATE_DIR / "sequence-calculator"
    return STATE_DIR / "sequence-vlog"


def next_mode(workspace_path, prompt):
    if "deliberately broken" in prompt.lower():
        return "browser-broken"
    path = sequence_for(workspace_path)
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


def next_id(state, prefix):
    state["counter"] = int(state.get("counter", 0)) + 1
    return f"{prefix}_{state['counter']}"


def parse_flags(args, names):
    values, rest = {}, []
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


def edit_vlog(project, current):
    filter_path = project / "src" / "filter.js"
    app_path = project / "src" / "app.js"
    if filter_path.exists():
        filter_path.write_text(VLOG_FILTER, encoding="utf-8")
    text = app_path.read_text(encoding="utf-8")
    if 'from "./filter.js"' not in text:
        text = text.replace('import { renderEntries } from "./render.js";',
                            'import { renderEntries } from "./render.js";\nimport { filterEntries } from "./filter.js";')
    text = text.replace("  // The baseline renders every entry and ignores the selected category.\n  const visible = entries;",
                        "  const visible = filterEntries(entries, category);")
    if current == "browser-broken":
        text = text.replace("  empty.hidden = visible.length > 0;",
                            "  empty.hidden = true;")
    app_path.write_text(text, encoding="utf-8")


def edit_calculator(project):
    (project / "src" / "calc.js").write_text(CALC, encoding="utf-8")


def server_ready(endpoint):
    try:
        with urllib.request.urlopen(endpoint + "/", timeout=2) as response:
            return 200 <= response.status < 300
    except Exception:
        return False


def invoke_browser(project):
    wrapper = project / ".factory" / "bin" / "factory-playwright-cli.mjs"
    if not wrapper.exists():
        return 0
    endpoint = os.environ.get("S4C_BROWSER_ENDPOINT", "http://127.0.0.1:4173")
    owned_server = None
    if server_ready(endpoint):
        # The Worker-managed test server is already serving this workspace.
        server = None
    else:
        server = subprocess.Popen(
            ["node", "scripts/serve.mjs"],
            cwd=project,
            env={**os.environ, "PORT": "4173"},
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        owned_server = server
    try:
        time.sleep(0.5 if server is None else 1.0)
        for args in (["open", endpoint + "/"], ["snapshot"], ["close"]):
            proc = subprocess.run(["node", str(wrapper), *args], cwd=project,
                                  capture_output=True, text=True, timeout=90)
            if proc.returncode != 0:
                detail = (proc.stderr or proc.stdout)[:1000]
                (STATE_DIR / "browser-error.log").write_text(detail, encoding="utf-8")
                print("browser wrapper failed: " + detail[:500], file=sys.stderr)
                return 1
        return 0
    finally:
        if owned_server is not None:
            owned_server.terminate()
            try:
                owned_server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                owned_server.kill()


def invoke_browser_failure(project):
    wrapper = project / ".factory" / "bin" / "factory-playwright-cli.mjs"
    if not wrapper.exists():
        return 0
    proc = subprocess.run(["node", str(wrapper), "open", "http://127.0.0.1:1/"],
                          cwd=project, capture_output=True, text=True, timeout=90)
    # The wrapper records the nonzero outcome; the stub must not convert a
    # failed command into success.
    return 0 if proc.returncode != 0 else 1


def edit(workspace_path, current, prompt):
    project = Path(workspace_path)
    if (project / "src" / "calc.js").exists() and (project / "package.json").read_text(encoding="utf-8").find("calculator") >= 0:
        edit_calculator(project)
        return 0
    if (project / "src" / "filter.js").exists():
        if current == "empty":
            return 0
        if current == "outside-scope":
            oracle = project / "tests" / "acceptance" / "browser-check.mjs"
            oracle.write_text("// tampered oracle\n", encoding="utf-8")
            return 0
        edit_vlog(project, current)
        if current == "omit-browser":
            return 0
        if current == "browser-fails":
            return invoke_browser_failure(project)
        return invoke_browser(project)
    return 0


def main(argv):
    if not argv:
        return 3
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    lock = open(STATE_DIR / ".lock", "w")
    fcntl.flock(lock, fcntl.LOCK_EX)
    if argv[0] == "--version":
        print("paseo-stub 0.0.0-s4c")
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
            docs = [{"workspaceId": key, "cwd": value} for key, value in state["workspaces"].items()]
            print(json.dumps(docs))
            return 0
        return 3
    if argv[0] == "run":
        flags, rest = parse_flags(argv[1:], {"--provider", "--workspace", "--mode", "--model", "--wait-timeout", "--title", "--label"})
        workspace_id = flags.get("--workspace")
        workspace_path = state["workspaces"].get(workspace_id)
        if not workspace_path:
            print("stub: unknown workspace", file=sys.stderr)
            return 1
        session_id = next_id(state, "sess")
        state["sessions"][session_id] = {
            "workspace": workspace_id, "workspacePath": workspace_path,
            "provider": flags.get("--provider", ""), "model": flags.get("--model", ""),
            "mode": flags.get("--mode", ""), "prompt": rest[-1] if rest else "",
        }
        save_state(state)
        prompt = rest[-1] if rest else ""
        current = next_mode(workspace_path, prompt)
        if current not in {"good", "empty", "browser-broken", "outside-scope", "omit-browser", "browser-fails"}:
            print("stub: unknown mode " + current, file=sys.stderr)
            return 1
        rc = edit(workspace_path, current, prompt)
        if rc:
            return rc
        print(json.dumps({"id": session_id}))
        return 0
    if argv[0] == "inspect":
        if len(argv) < 2:
            return 3
        session = state["sessions"].get(argv[1], {})
        if not session:
            return 1
        print(json.dumps({"Id": argv[1], "Provider": session.get("provider", ""),
                          "Model": session.get("model", ""), "Mode": session.get("mode", ""),
                          "Cwd": session.get("workspacePath", ""), "Status": "idle", "Archived": False}))
        return 0
    if argv[0] == "logs":
        print("[Read] src\n[Edit] source\n[Shell] npm test\n[Shell] factory-playwright-cli\n[Text] done")
        return 0
    if argv[0] == "send":
        args = [arg for arg in argv[1:] if not arg.startswith("--")]
        if len(args) >= 2:
            session = state["sessions"].get(args[0], {})
            message = " ".join(args[1:])
            workspace = session.get("workspacePath")
            if workspace and "browser interaction evidence" in message:
                current = mode()
                if current == "browser-fails":
                    return invoke_browser_failure(Path(workspace))
                if current in {"omit-browser", "good"}:
                    return invoke_browser(Path(workspace))
        return 0
    if argv[0] in {"wait", "stop", "archive"}:
        return 0
    if argv[0] == "ls":
        print("[]")
        return 0
    return 3


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - diagnostic only, bounded
        import traceback
        try:
            STATE_DIR.mkdir(parents=True, exist_ok=True)
            (STATE_DIR / "error.log").write_text(traceback.format_exc()[-4000:], encoding="utf-8")
        except OSError:
            pass
        print("stub error: " + str(exc)[:300], file=sys.stderr)
        sys.exit(1)
