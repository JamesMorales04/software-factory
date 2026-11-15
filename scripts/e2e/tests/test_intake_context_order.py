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


class OwnerWaitAndErrorScopeTests(unittest.TestCase):
    def test_auto_approved_preparation_is_not_an_owner_wait(self) -> None:
        projection = {
            "state": "AWAITING_APPROVAL",
            "planning": {
                "materialsReady": False,
                "reviewCurrent": True,
                "reviewStatus": "AUTO_APPROVED",
            },
        }
        self.assertFalse(compose_intake._owner_wait_is_real(projection))

    def test_current_pending_review_with_materials_is_a_real_owner_wait(self) -> None:
        projection = {
            "state": "AWAITING_APPROVAL",
            "planning": {
                "materialsReady": True,
                "reviewCurrent": True,
                "reviewStatus": "PENDING",
            },
        }
        self.assertTrue(compose_intake._owner_wait_is_real(projection))

    def test_only_new_errors_for_this_request_are_relevant(self) -> None:
        diagnostics = {
            "errors": {
                "old-request|assessment": "already seen",
                "other-request|assessment": "other run",
                "mine|assessment": "current failure",
                "|planner": "no identity",
            }
        }
        relevant = compose_intake._fixture_relevant_errors(
            {"old-request|assessment"}, diagnostics, "mine"
        )
        self.assertEqual(relevant, {"mine|assessment": "current failure"})


class PerRequestContextTests(unittest.TestCase):
    def load_fixture(self):
        import importlib.util
        from pathlib import Path

        repo = Path(__file__).resolve().parents[3]
        fixture_path = repo / "scripts" / "fixtures" / "compose-model-fixture.py"
        spec = importlib.util.spec_from_file_location("compose_model_fixture_r6", fixture_path)
        module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        spec.loader.exec_module(module)
        return module

    def context(self, request_id):
        return {
            "projectId": "project-1",
            "actionId": "action-1",
            "requestId": request_id,
            "objective": "Add the deterministic Compose fixture candidate.",
            "baseRevision": "base-1",
            "projectConfigHash": "b" * 64,
            "qualityProfile": "generic",
            "provider": "fixture",
            "model": "fixture-model",
            "allowedPaths": ["src"],
            "capabilities": ["repository-edit", "generic-toolchain"],
            "checks": [],
        }

    def prompt(self):
        return (
            "Objective: Add the deterministic Compose fixture candidate."
            ". Project: project-1. Action: action-1. Request: request-1."
        )

    def test_two_requests_keep_their_own_identity(self) -> None:
        import json
        import tempfile
        from pathlib import Path

        fixture = self.load_fixture()
        with tempfile.TemporaryDirectory() as directory:
            model_dir = Path(directory)
            (model_dir / "contexts").mkdir()
            fixture.CONTEXT_PATH = model_dir / "context.json"
            fixture.CONTEXTS_DIR = model_dir / "contexts"
            first = self.context("request-1")
            second = self.context("request-2")
            (model_dir / "contexts" / "request-1.json").write_text(json.dumps(first))
            (model_dir / "contexts" / "request-2.json").write_text(json.dumps(second))
            body = json.loads(
                fixture.assessment(self.prompt())["choices"][0]["message"]["content"]
            )
            self.assertEqual(body["requestId"], "request-1")
            self.assertEqual(fixture.context("request-2")["requestId"], "request-2")

    def test_barrier_reads_the_real_context_file(self) -> None:
        import json
        import tempfile
        import threading
        from pathlib import Path

        fixture = self.load_fixture()
        result = {}
        with tempfile.TemporaryDirectory() as directory:
            model_dir = Path(directory)
            fixture.CONTEXT_PATH = model_dir / "context.json"
            fixture.CONTEXTS_DIR = model_dir / "contexts"
            fixture.CONTEXT_PATH.write_text(json.dumps(self.context("")))
            barrier = threading.Barrier(2)

            def dispatch():
                barrier.wait()
                result["body"] = json.loads(
                    fixture.assessment(self.prompt())["choices"][0]["message"]["content"]
                )

            def finalize():
                barrier.wait()
                (model_dir / "contexts").mkdir()
                (model_dir / "contexts" / "request-1.json").write_text(
                    json.dumps(self.context("request-1"))
                )

            threads = [threading.Thread(target=dispatch), threading.Thread(target=finalize)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join()
        self.assertEqual(result["body"]["requestId"], "request-1")
