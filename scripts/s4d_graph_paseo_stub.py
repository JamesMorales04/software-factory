#!/usr/bin/env python3
"""Deterministic Paseo CLI stub for the S4-D Graph runner dry run.

No daemon and no provider calls. It returns one valid two-unit Lead
assessment and performs the two fixture edits when an implementer run uses
the corresponding unit prompt.
"""
from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

STATE = Path(os.environ.get("S4D_GRAPH_STUB_DIR", "/tmp/s4d-graph-paseo-stub"))
STATE.mkdir(parents=True, exist_ok=True)
STATE_FILE = STATE / "state.json"


def load():
    try:
        return json.loads(STATE_FILE.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {"workspaces": {}, "sessions": {}, "nextWorkspace": 1, "nextSession": 1}


def save(state):
    STATE_FILE.write_text(json.dumps(state), encoding="utf-8")


def parse_flags(args):
    flags, rest, i = {}, [], 0
    while i < len(args):
        if args[i].startswith("--"):
            key = args[i]
            if key in {"--json"}:
                flags[key] = "true"
                i += 1
            elif i + 1 < len(args) and not args[i + 1].startswith("--"):
                flags[key] = args[i + 1]
                i += 2
            else:
                flags[key] = "true"
                i += 1
        else:
            rest.append(args[i])
            i += 1
    return flags, rest


def assessment_for(prompt: str) -> dict:
    marker = "INPUT_JSON:\n"
    data = {}
    if marker in prompt:
        try:
            data = json.loads(prompt.split(marker, 1)[1])
        except json.JSONDecodeError:
            data = {}
    return {
        "schemaRevision": 1,
        "projectId": data.get("projectId", "fixture"),
        "actionId": data.get("actionId", "0" * 32),
        "requestId": data.get("requestId", "1" * 32),
        "evidenceRefs": ["repository-files"],
        "intent": "change",
        "complexity": "medium",
        "effortEstimate": {
            "status": "estimated", "unit": "person-hours",
            "minPersonHours": 2, "maxPersonHours": 6,
            "assumptions": ["Two independent bounded units plus deterministic integration."],
            "unknownReason": "",
        },
        "knowledgeGaps": [],
        "changeImpact": ["src/greeting.js and src/app.js are updated."],
        "validationNeeds": ["Run format, unit, functional and browser acceptance."],
        "assumptions": ["The fixture is a disposable external Project."],
        "uncertainty": ["Integration conflict risk is low because unit paths are disjoint."],
        "suggestedNextAction": "Start the authorized two-unit Campaign.",
        "proposal": {
            "projectId": data.get("projectId", "fixture"),
            "actionId": data.get("actionId", "0" * 32),
            "baseRevision": data.get("baseRevision", "a" * 40),
            "projectConfigHash": data.get("projectConfigHash", "b" * 64),
            "objective": data.get("objective", "Implement greeting and UI."),
            "class": "feature",
            "assumptions": ["Units modify disjoint paths."],
            "risks": ["Integration may expose a contract mismatch."],
            "acceptanceCriteria": [
                {"id": "format", "kind": "format", "description": "Full syntax check succeeds", "command": ["npm", "run", "format"], "expectExit": 0, "timeoutSecs": 120},
                {"id": "test", "kind": "test", "description": "Full unit and static tests pass", "command": ["node", "--test", "tests/unit/greeting.test.mjs", "tests/unit/app.test.mjs"], "expectExit": 0, "timeoutSecs": 120},
                {"id": "functional", "kind": "functional", "description": "Integrated greeting behavior works", "command": ["node", "--input-type=module", "-e", "import('./src/greeting.js').then(m=>{if(m.greeting('Factory')!=='Hello, Factory!')process.exit(1)})"], "expectExit": 0, "timeoutSecs": 120},
                {"id": "browser", "kind": "browser", "description": "Browser renders the greeting", "command": ["node", "tests/acceptance/browser-check.mjs"], "expectExit": 0, "timeoutSecs": 120},
            ],
            "negativeCases": [
                {"id": "negative", "kind": "negative", "description": "Empty name is rejected", "command": ["node", "--input-type=module", "-e", "import('./src/greeting.js').then(m=>{try{m.greeting('')}catch(e){process.exit(e instanceof TypeError?0:1)}process.exit(1)})"], "expectExit": 0, "timeoutSecs": 120},
            ],
            "qualityProfile": data.get("qualityProfile", "node-basic"),
            "riskClass": "R0",
            "budget": {"maxAttempts": 1, "timeoutSecs": 600},
            "provider": "opencode", "model": "opencode-go/deepseek-v4.1-flash", "paseoMode": "build",
            "maxRepairCycles": 0, "maxWallClockSecs": 600, "maxParallel": 2, "maxSessions": 3,
            "workUnits": [
                {"id": "backend", "kind": "implementation", "description": "Implement greeting in src/greeting.js with validation.", "dependsOn": [], "allowedPaths": ["src/greeting.js"], "capabilities": ["repository-edit", "node-toolchain"]},
                {"id": "frontend", "kind": "implementation", "description": "Wire greeting into the browser UI in src/app.js.", "dependsOn": [], "allowedPaths": ["src/app.js"], "capabilities": ["repository-edit", "node-toolchain"]},
            ],
        },
    }


def edit(workspace: Path, prompt: str):
    lowered = prompt.lower()
    if "workunit backend:" in lowered:
        (workspace / "src/greeting.js").write_text(
            'export function greeting(name) {\n'
            '  if (typeof name !== "string" || name.trim() === "") {\n'
            '    throw new TypeError("name must be a non-empty string");\n'
            '  }\n'
            '  return `Hello, ${name}!`;\n'
            '}\n',
            encoding="utf-8",
        )
    if "workunit frontend:" in lowered:
        (workspace / "src/app.js").write_text(
            'import { greeting } from "./greeting.js";\n'
            'const target = document.querySelector("#greeting");\n'
            'if (target) {\n'
            '  target.textContent = greeting("Factory");\n'
            '}\n',
            encoding="utf-8",
        )


def main(argv):
    import fcntl
    lock_path = STATE / "lock"
    with open(lock_path, "w", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            return _main(argv)
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def _main(argv):
    log_path = os.environ.get("S4D_GRAPH_STUB_LOG")
    if log_path:
        with open(log_path, "a", encoding="utf-8") as log:
            log.write(json.dumps({"argv": argv, "time": time.time()}) + "\n")
    state = load()
    if not argv:
        return 3
    if argv[0] == "--version":
        print("paseo-stub 0.0.0-s4d-graph")
        return 0
    if argv[0] == "workspace" and len(argv) > 1:
        if argv[1] == "create":
            flags, _ = parse_flags(argv[2:])
            path = flags.get("--path", "")
            wid = f"ws_{state['nextWorkspace']}"
            state["nextWorkspace"] += 1
            state["workspaces"][wid] = path
            save(state)
            print(json.dumps({"workspaceId": wid}))
            return 0
        if argv[1] == "archive":
            if len(argv) > 2:
                state["workspaces"].pop(argv[2], None)
                save(state)
            return 0
        if argv[1] == "ls":
            print("[]")
            return 0
        return 3
    if argv[0] == "run":
        flags, rest = parse_flags(argv[1:])
        wid = flags.get("--workspace", "")
        path = state["workspaces"].get(wid)
        if not path:
            print("stub: unknown workspace", file=sys.stderr)
            return 1
        sid = f"sess_{state['nextSession']}"
        state["nextSession"] += 1
        labels = {}
        for i, arg in enumerate(argv):
            if arg == "--label" and i + 1 < len(argv):
                key, _, value = argv[i + 1].partition("=")
                labels[key] = value
        prompt = rest[-1] if rest else ""
        state["sessions"][sid] = {
            "id": sid, "workspace": wid, "workspacePath": path, "prompt": prompt,
            "provider": flags.get("--provider", ""), "model": flags.get("--model", ""),
            "mode": flags.get("--mode", ""), "labels": labels,
            "assessment": "--output-schema" in flags,
        }
        save(state)
        try:
            if "--output-schema" in flags:
                print(json.dumps(assessment_for(prompt)))
            else:
                edit(Path(path), prompt)
                print(json.dumps({"id": sid}))
        except Exception as exc:
            log_path = os.environ.get("S4D_GRAPH_STUB_LOG")
            if log_path:
                with open(log_path, "a", encoding="utf-8") as log:
                    log.write(json.dumps({"error": repr(exc), "argv": argv}) + "\n")
            raise
        if log_path := os.environ.get("S4D_GRAPH_STUB_LOG"):
            with open(log_path, "a", encoding="utf-8") as log:
                log.write(json.dumps({"run": sid, "assessment": "--output-schema" in flags, "rc": 0}) + "\n")
        return 0
    if argv[0] == "inspect":
        session = state["sessions"].get(argv[1]) if len(argv) > 1 else None
        if not session:
            return 1
        print(json.dumps({
            "Id": session["id"], "Provider": session["provider"], "Model": session["model"],
            "Mode": session["mode"], "Cwd": session["workspacePath"], "Status": "idle", "Archived": False,
        }))
        return 0
    if argv[0] == "ls":
        wanted = {}
        for i, arg in enumerate(argv):
            if arg == "--label" and i + 1 < len(argv):
                key, _, value = argv[i + 1].partition("=")
                wanted[key] = value
        out = []
        for session in state["sessions"].values():
            if all(session.get("labels", {}).get(k) == v for k, v in wanted.items()):
                out.append({
                    "id": session["id"], "provider": session["provider"], "model": session["model"],
                    "status": "idle", "cwd": session["workspacePath"],
                })
        print(json.dumps(out))
        return 0
    if argv[0] == "logs":
        print("[Read] src/greeting.js\n[Edit] src/greeting.js\n[Shell] npm test\n[Text] done")
        return 0
    if argv[0] in {"stop", "archive", "wait", "send"}:
        return 0
    return 3


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
