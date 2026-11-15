"""Deterministic ordering test for the AUTONOMOUS intake model context.

The product may dispatch the Lead assessment milliseconds after the intake POST
returns. This test pins the harness contract: the static fixture context exists
*before* the POST is sent, so a racing dispatch never sees a missing file, and
the exact ids are rewritten afterwards as the authoritative context.
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
E2E = HERE.parent
if str(E2E) not in sys.path:
    sys.path.insert(0, str(E2E))

import compose_intake  # noqa: E402


class FakeApi:
    def __init__(self, context_path: Path) -> None:
        self.context_path = context_path
        self.observed_context = None

    def post(self, url, payload):
        assert url == "/intake/requests", url
        # This is the exact moment Core may dispatch the assessment: the file
        # must already be readable and parseable.
        self.observed_context = json.loads(self.context_path.read_text(encoding="utf-8"))
        return {
            "id": "intake-1",
            "actionId": "action-1",
            "leadRequestId": "request-1",
            "mode": "AUTONOMOUS",
        }


class FakeState:
    def __init__(self, values):
        self.values = dict(values)

    def require(self, *keys):
        for key in keys:
            if not self.values.get(key):
                raise AssertionError(f"missing state {key}")

    def get(self, key, default=None):
        return self.values.get(key, default)

    def set(self, key, value):
        self.values[key] = value


class IntakeContextOrderTests(unittest.TestCase):
    def test_static_context_exists_at_the_intake_post(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            context_path = Path(directory) / "context.json"
            state = FakeState(
                {
                    "project_id": "project-1",
                    "profile_id": "profile-1",
                    "project_base_commit": "base-1",
                    "project_config_hash": "c" * 64,
                    "model_dir": directory,
                }
            )
            api = FakeApi(context_path)
            context = SimpleNamespace(
                state=state,
                api=api,
                config=SimpleNamespace(run_id="run-1"),
                admit_scope=lambda scope: None,
            )
            compose_intake.autonomous_request(context)

            observed = api.observed_context
            self.assertEqual(observed["projectId"], "project-1")
            self.assertEqual(observed["objective"], state.get("intake_objective"))
            # Dynamic ids are not known yet: the fixture resolves them from the
            # real prompt and the harness rewrites them below.
            self.assertEqual(observed["requestId"], "")
            self.assertEqual(observed["actionId"], "")

            final = json.loads(context_path.read_text(encoding="utf-8"))
            self.assertEqual(final["requestId"], "request-1")
            self.assertEqual(final["actionId"], "action-1")
            self.assertEqual(final["intakeRequestId"], "intake-1")
            self.assertEqual(state.get("model_context"), str(context_path))


if __name__ == "__main__":
    unittest.main()
