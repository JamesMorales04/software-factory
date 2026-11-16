"""The harness assigns the test-support environment placeholder per scenario.

The autonomous journey binds a real EnvironmentPlan, so the placeholder is
assigned "0" there and "1" only for the fixture-only journeys. The value is
always written explicitly, so a host variable can never leak into the stack.
"""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.e2e.common import Config  # noqa: E402

KEY = "FACTORY_TEST_ENVIRONMENT_BINDING"


def config(scenario: str) -> Config:
    root = Path("/tmp/factory-harness-optin-test")
    return Config(
        root=root,
        run_id="run-1",
        run_dir=root / "run",
        artifacts_dir=root / "artifacts",
        project="software-factory-e2e-run-1",
        port=12345,
        provider="fixture",
        github="fake",
        cassette=None,
        browser="none",
        from_stage="prepare",
        to_stage="summary",
        keep=False,
        snapshot=False,
        restore=False,
        snapshot_dir=root / "snaps",
        no_build=False,
        operation_timeout=60,
        readiness_timeout=60,
        run_timeout=60,
        scenario=scenario,
    )


class EnvironmentOptInTests(unittest.TestCase):
    def test_autonomous_journey_assigns_zero(self) -> None:
        self.assertEqual(config("autonomous-intake").environment().get(KEY), "0")

    def test_fixture_only_journey_assigns_one(self) -> None:
        self.assertEqual(config("compose-intake").environment().get(KEY), "1")

    def test_a_host_value_never_leaks(self) -> None:
        for host, autonomous, fixture in [
            ("1", "0", "1"),
            ("0", "0", "1"),
            (None, "0", "1"),
        ]:
            with mock.patch.dict(os.environ, {}, clear=False):
                os.environ.pop(KEY, None)
                if host is not None:
                    os.environ[KEY] = host
                self.assertEqual(
                    config("autonomous-intake").environment().get(KEY), autonomous
                )
                self.assertEqual(config("compose-intake").environment().get(KEY), fixture)


class NoEnvironmentDumpTests(unittest.TestCase):
    """The harness must never print the environment, only compare one key.

    A failing ``assertNotIn`` against the whole environment dict dumped every
    variable into the tool transcript once. These checks are the regression: the
    sources must not contain such a dump, and building the environment must not
    write any value to stdout or stderr.
    """

    FORBIDDEN = [
        "pprint(",
        "print(environment)",
        "repr(os.environ)",
        "repr(dict(os.environ))",
        "print(os.environ)",
    ]

    def test_harness_sources_do_not_dump_the_environment(self) -> None:
        offenders = []
        for path in sorted((ROOT / "scripts" / "e2e").rglob("*.py")):
            if path.name == "test_harness_environment_optin.py":
                # This file lists the forbidden patterns on purpose.
                continue
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                if any(pattern in line for pattern in self.FORBIDDEN):
                    offenders.append(f"{path.name}:{number}: {line.strip()[:60]}")
                lowered = line.lower()
                if "assertnotin(" in lowered and (
                    "environment" in lowered or "os.environ" in lowered or "env" in lowered
                ):
                    offenders.append(f"{path.name}:{number}: environment assert dump")
        self.assertEqual(offenders, [], "environment dump patterns are forbidden")

    def test_building_the_environment_writes_nothing(self) -> None:
        import subprocess

        sentinel = "cr15-synthetic-sentinel-value"
        script = (
            "import sys; sys.path.insert(0, %r);"
            "from pathlib import Path;"
            "from scripts.e2e.common import Config;"
            "root = Path('/tmp/factory-harness-optin-test');"
            "cfg = Config(root=root, run_id='r', run_dir=root, artifacts_dir=root,"
            " project='software-factory-e2e-r', port=1, provider='fixture',"
            " github='fake', cassette=None, browser='none', from_stage='prepare',"
            " to_stage='summary', keep=False, snapshot=False, restore=False,"
            " snapshot_dir=root, no_build=False, operation_timeout=1,"
            " readiness_timeout=1, run_timeout=1, scenario='autonomous-intake');"
            "cfg.environment()"
        ) % str(ROOT)
        result = subprocess.run(
            [sys.executable, "-c", script],
            capture_output=True,
            text=True,
            env={**os.environ, "FACTORY_TEST_ENVIRONMENT_BINDING": sentinel},
            timeout=60,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr[:200])
        self.assertNotIn(sentinel, result.stdout)
        self.assertNotIn(sentinel, result.stderr)


if __name__ == "__main__":
    unittest.main()
