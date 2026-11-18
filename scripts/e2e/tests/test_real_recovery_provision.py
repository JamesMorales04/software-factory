#!/usr/bin/env python3
"""Consumer test for the provisioning sequence without Podman or Core.

The fake `podman` only records argv and returns canonical command results. It
does not pretend to prove a real prep: the test proves the script starts the
one-shot bundles before Core, waits for their exit and the digest, copies the
row only after Core migrated, and aborts a failed bundle before touching Core.
"""

from __future__ import annotations

import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve()
ROOT = HERE.parents[3]
SCRIPT = ROOT / "scripts" / "e2e" / "real-recovery-provision.sh"

FAKE_PODMAN = r'''#!/usr/bin/env python3
import os
import sys

args = sys.argv[1:]
log = os.environ["FAKE_PODMAN_LOG"]
with open(log, "a", encoding="utf-8") as handle:
    handle.write(" ".join(args) + "\n")

fail_bundles = os.environ.get("FAKE_BUNDLE_FAIL") == "1"

if not args:
    sys.exit(0)
command = args[0]

if command == "volume":
    if len(args) >= 3 and args[1] == "exists":
        # The installed product volumes exist; the disposable destination is new.
        sys.exit(0 if args[2].startswith("software-factory_") else 1)
    sys.exit(0)
if command == "image":
    sys.exit(0)
if command == "compose":
    if "config" in args:
        print("FACTORY_LEAD_FIXTURE_ASSESSOR: 0")
    sys.exit(0)
if command == "ps":
    service = ""
    for index, value in enumerate(args):
        if value.startswith("label=com.docker.compose.service="):
            service = value.split("=", 1)[1]
    if service:
        print(f"fake_{service}")
    # A project-only filter models the "is the source project running?" probe.
    sys.exit(0)
if command == "inspect":
    name = args[1] if len(args) > 1 else ""
    if fail_bundles and ("bundle" in name):
        print("exited:1")
    else:
        print("exited:0")
    sys.exit(0)
if command == "exec":
    if "cat" in args and "/config/operator.token" in args:
        print("test-operator-token")
    sys.exit(0)
if command == "run":
    joined = " ".join(args)
    if "environment-builder.digest" in joined:
        sys.exit(1 if (fail_bundles or os.environ.get("FAKE_DIGEST_MISSING") == "1") else 0)
    if "/dst-data" in joined and "python3" in joined:
        if os.environ.get("FAKE_DB_NOT_MIGRATED") == "1":
            print("DEST_SCHEMA_MISSING provider_settings")
            sys.exit(1)
        if "provider,kind,model" in joined:
            print("ROW_COPY_OK 0")
        else:
            print("DEST_CHECK_OK one opencode-go row, enabled/default, ciphertext present, no plaintext column, integrity ok")
        sys.exit(0)
    if "/src-data" in joined and "python3" in joined:
        print("SOURCE_CHECK_OK one opencode-go row, enabled/default, ciphertext+fingerprint present, master.key 64 hex, integrity ok")
        sys.exit(0)
    sys.exit(0)
sys.exit(0)
'''

FAKE_SLEEP = "#!/bin/sh\nexit 0\n"


class ProvisionSequenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.bin = Path(self.tmp.name) / "bin"
        self.bin.mkdir()
        podman = self.bin / "podman"
        podman.write_text(FAKE_PODMAN, encoding="utf-8")
        podman.chmod(podman.stat().st_mode | stat.S_IXUSR)
        sleep = self.bin / "sleep"
        sleep.write_text(FAKE_SLEEP, encoding="utf-8")
        sleep.chmod(sleep.stat().st_mode | stat.S_IXUSR)
        self.log = Path(self.tmp.name) / "podman.log"
        self.run_root = Path(self.tmp.name) / "real-recovery-run"
        self.env = {
            **os.environ,
            "PATH": f"{self.bin}:{os.environ['PATH']}",
            "FAKE_PODMAN_LOG": str(self.log),
            "FACTORY_RECOVERY_PROJECT": "sfe2e-recovery",
            "FACTORY_RECOVERY_RUN_ROOT": str(self.run_root),
            "FACTORY_RECOVERY_TOOL_IMAGE": "localhost/software-factory-dev:local",
        }

    def run_script(self, extra_env=None):
        env = dict(self.env)
        env.update(extra_env or {})
        return subprocess.run(
            ["bash", str(SCRIPT), "--apply"],
            text=True,
            capture_output=True,
            env=env,
            timeout=120,
        )

    def lines(self):
        return self.log.read_text(encoding="utf-8").splitlines()

    def test_bundles_run_before_core_and_row_copy_follows_migration(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = self.lines()
        bundle_up = next(
            i for i, line in enumerate(lines)
            if "up -d init provider-bundle dependency-bundle environment-builder-bundle" in line
        )
        core_up = next(
            i for i, line in enumerate(lines)
            if "up -d core" in line and "runtime" not in line
        )
        stop = next(i for i, line in enumerate(lines) if "stop core" in line)
        row = next(i for i, line in enumerate(lines) if "provider,kind,model" in line)
        stack_up = next(
            i for i, line in enumerate(lines)
            if "up -d core runtime supervisor" in line
        )
        self.assertLess(bundle_up, core_up)
        self.assertLess(core_up, stop)
        self.assertLess(stop, row)
        self.assertLess(row, stack_up)
        self.assertFalse(any("up -d init core" in line for line in lines))

    def test_failed_bundle_aborts_before_core(self):
        result = self.run_script({"FAKE_BUNDLE_FAIL": "1"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("image bundle", result.stderr)
        self.assertIn("failed before Core started", result.stderr)
        lines = self.lines()
        self.assertFalse(any("up -d core" in line for line in lines))
        self.assertFalse(any("provider,kind,model" in line for line in lines))

    def test_clean_bundles_without_digest_abort_before_core(self):
        result = self.run_script({"FAKE_DIGEST_MISSING": "1"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("environment-builder.digest is missing", result.stderr)
        lines = self.lines()
        self.assertFalse(any("up -d core" in line for line in lines))

    def test_unmigrated_database_aborts_before_the_stack_starts(self):
        result = self.run_script({"FAKE_DB_NOT_MIGRATED": "1"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("destination provider row copy failed", result.stderr)
        lines = self.lines()
        self.assertTrue(any("provider,kind,model" in line for line in lines))
        self.assertFalse(any("up -d core runtime" in line for line in lines))


if __name__ == "__main__":
    unittest.main()
