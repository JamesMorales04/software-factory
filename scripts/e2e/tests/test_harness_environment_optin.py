"""The harness opts into the test-support environment placeholder per scenario.

The autonomous journey binds a real EnvironmentPlan, so it must not inherit the
fixture ActionSpec binding; the fixture-only journeys keep their explicit
contract by setting the variable themselves.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.e2e.common import Config  # noqa: E402


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
    def test_autonomous_journey_leaves_the_placeholder_off(self) -> None:
        self.assertNotIn(
            "FACTORY_TEST_ENVIRONMENT_BINDING", config("autonomous-intake").environment()
        )

    def test_fixture_only_journey_opts_in_explicitly(self) -> None:
        environment = config("compose-intake").environment()
        self.assertEqual(environment["FACTORY_TEST_ENVIRONMENT_BINDING"], "1")

    def test_the_opt_in_is_scenario_scoped_not_global(self) -> None:
        autonomous = config("autonomous-intake").environment()
        fixture = config("compose-intake").environment()
        self.assertNotEqual(
            autonomous.get("FACTORY_TEST_ENVIRONMENT_BINDING"),
            fixture.get("FACTORY_TEST_ENVIRONMENT_BINDING"),
        )


if __name__ == "__main__":
    unittest.main()
