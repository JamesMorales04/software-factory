#!/usr/bin/env python3
"""Exercise paid E2E guards with a fail-fast Paseo stub; never contacts providers."""
from __future__ import annotations

import os
import signal
import socket
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TIMEOUT_SECONDS = 90
LIVE_FLAGS = (
    "FACTORY_S3B_LIVE", "FACTORY_S3D_LIVE", "FACTORY_S4A_LIVE",
    "FACTORY_S4B_LIVE", "FACTORY_S4C_LIVE", "FACTORY_S4D_LIVE",
    "FACTORY_S4D_GRAPH_LIVE", "FACTORY_LIVE_MODE",
    "FACTORY_LIVE_AUTHORIZATION", "FACTORY_LIVE_AUTH_CAMPAIGN",
    "FACTORY_LIVE_AUTH_PROJECT", "FACTORY_LIVE_AUTH_SESSION_BUDGET",
    "FACTORY_LIVE_AUTH_ROUTES",
)
ROUTE_OVERRIDES = (
    "S3D_LIVE_PROVIDER", "S3D_LIVE_MODEL", "S3D_LIVE_MODE",
    "S4A_LIVE_PROVIDER", "S4A_LIVE_MODEL", "S4A_LIVE_MODE",
    "S4B_LIVE_PROVIDER", "S4B_LIVE_MODEL", "S4B_LIVE_MODE",
)


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def run_case(name: str, command: list[str], live_value: str | None,
             *, s3b: bool = False) -> None:
    with tempfile.TemporaryDirectory(prefix="factory-paid-guard-") as temp_name:
        temp = Path(temp_name)
        stub_dir = temp / "bin"
        stub_dir.mkdir()
        calls = temp / "paseo-calls.log"
        stub = stub_dir / "paseo"
        stub.write_text(
            "#!/usr/bin/env python3\n"
            "import os, sys\n"
            "with open(os.environ['PASEO_TEST_LOG'], 'a', encoding='utf-8') as log:\n"
            "    log.write(' '.join(sys.argv[1:]) + '\\n')\n"
            "raise SystemExit(97)\n",
            encoding="utf-8",
        )
        stub.chmod(0o755)

        env = os.environ.copy()
        for key in tuple(env):
            if key in LIVE_FLAGS or key in ROUTE_OVERRIDES or key.startswith("FACTORY_LIVE_AUTH_"):
                env.pop(key, None)
        if live_value:
            env[live_value.split("=", 1)[0]] = live_value.split("=", 1)[1]
        if s3b:
            env["FACTORY_S3B_PORT"] = str(free_port())
        env["PATH"] = f"{stub_dir}{os.pathsep}{env.get('PATH', '')}"
        env["PASEO_TEST_LOG"] = str(calls)

        process = subprocess.Popen(
            command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, start_new_session=True,
        )
        try:
            output, _ = process.communicate(timeout=TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                output, _ = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                output, _ = process.communicate()
            raise AssertionError(f"{name}: timed out; output tail:\n{output[-2000:]}")

        if calls.exists() and calls.stat().st_size:
            raise AssertionError(
                f"{name}: attempted Paseo dispatch despite guarded test mode:\n"
                f"{calls.read_text(encoding='utf-8')}\noutput tail:\n{output[-2000:]}"
            )
        expected = 2 if live_value and live_value.endswith("=1") else 0
        if process.returncode != expected:
            raise AssertionError(
                f"{name}: exit {process.returncode}, expected {expected}; output tail:\n{output[-2000:]}"
            )
        if live_value and live_value.endswith("=1") and "[BLOCKED] live authorization:" not in output:
            raise AssertionError(f"{name}: did not report failed preflight:\n{output[-2000:]}")
        if s3b and "[PASS] s6: synthetic-echo unchanged" not in output:
            raise AssertionError(f"{name}: deterministic S3B lane did not complete:\n{output[-2000:]}")
        print(f"[PASS] {name}: exit={process.returncode}, zero Paseo calls")


def main() -> int:
    cases = [
        ("S3B default", ["bash", "scripts/e2e-s3b.sh"], None, True),
        ("S3B explicit LIVE=0", ["bash", "scripts/e2e-s3b.sh"], "FACTORY_S3B_LIVE=0", True),
        ("S3B live flag without authorization", ["bash", "scripts/e2e-s3b.sh"], "FACTORY_S3B_LIVE=1", True),
        ("S3D default", ["bash", "scripts/e2e-s3d-live.sh"], None, False),
        ("S3D explicit LIVE=0", ["bash", "scripts/e2e-s3d-live.sh"], "FACTORY_S3D_LIVE=0", False),
        ("S3D live flag without authorization", ["bash", "scripts/e2e-s3d-live.sh"], "FACTORY_S3D_LIVE=1", False),
        ("S4A default", ["bash", "scripts/e2e-s4a-live.sh"], None, False),
        ("S4A explicit LIVE=0", ["bash", "scripts/e2e-s4a-live.sh"], "FACTORY_S4A_LIVE=0", False),
        ("S4A live flag without authorization", ["bash", "scripts/e2e-s4a-live.sh"], "FACTORY_S4A_LIVE=1", False),
        ("S4B default", ["bash", "scripts/e2e-s4b-live.sh"], None, False),
        ("S4B explicit LIVE=0", ["bash", "scripts/e2e-s4b-live.sh"], "FACTORY_S4B_LIVE=0", False),
        ("S4B live flag without authorization", ["bash", "scripts/e2e-s4b-live.sh"], "FACTORY_S4B_LIVE=1", False),
        ("S4D default", [sys.executable, "scripts/e2e_s4d_live.py"], None, False),
        ("S4D explicit LIVE=0", [sys.executable, "scripts/e2e_s4d_live.py"], "FACTORY_S4D_LIVE=0", False),
        ("S4D live flag without authorization", [sys.executable, "scripts/e2e_s4d_live.py"], "FACTORY_S4D_LIVE=1", False),
        ("S4D graph default", [sys.executable, "scripts/e2e_s4d_graph_live.py"], None, False),
        ("S4D graph explicit LIVE=0", [sys.executable, "scripts/e2e_s4d_graph_live.py"], "FACTORY_S4D_GRAPH_LIVE=0", False),
        ("S4D graph live flag without authorization", [sys.executable, "scripts/e2e_s4d_graph_live.py"], "FACTORY_S4D_GRAPH_LIVE=1", False),
        ("C-07 Python default", [sys.executable, "scripts/e2e_s4c_07_live.py"], None, False),
        ("C-07 Python explicit LIVE=0", [sys.executable, "scripts/e2e_s4c_07_live.py"], "FACTORY_S4C_LIVE=0", False),
        ("C-07 Python live flag without authorization", [sys.executable, "scripts/e2e_s4c_07_live.py"], "FACTORY_S4C_LIVE=1", False),
        ("C-07 shell default", ["bash", "scripts/e2e-s4c-07-live.sh"], None, False),
        ("C-07 shell explicit LIVE=0", ["bash", "scripts/e2e-s4c-07-live.sh"], "FACTORY_S4C_LIVE=0", False),
        ("C-07 shell live flag without authorization", ["bash", "scripts/e2e-s4c-07-live.sh"], "FACTORY_S4C_LIVE=1", False),
        ("legacy S4C wrapper default", ["bash", "scripts/e2e-s4c-live.sh"], None, False),
        ("legacy S4C wrapper explicit LIVE=0", ["bash", "scripts/e2e-s4c-live.sh"], "FACTORY_S4C_LIVE=0", False),
        ("legacy S4C wrapper live flag without authorization", ["bash", "scripts/e2e-s4c-live.sh"], "FACTORY_S4C_LIVE=1", False),
    ]
    for name, command, live_value, s3b in cases:
        run_case(name, command, live_value, s3b=s3b)
    print("All paid E2E entrypoint integration checks passed; no provider session was started.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
