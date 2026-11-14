"""Deterministic tests for the driver -> Playwright cookie handoff.

The handoff uses the driver's real LWP cookie jar and exports Playwright
storageState JSON. No test parses a second cookie format; the journey must never
receive the legacy Netscape file.
"""

from __future__ import annotations

import http.cookiejar
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from common import Api, Config  # noqa: E402


def _config(root: Path) -> Config:
    run_dir = root / "run"
    return Config(
        root=root,
        run_id="factory-e2e-cookie-test",
        run_dir=run_dir,
        artifacts_dir=run_dir / "artifacts",
        project="software-factory-e2e-cookie-test",
        port=8894,
        provider="fixture",
        github="fake",
        cassette=None,
        browser="none",
        from_stage="enroll",
        to_stage="enroll",
        keep=True,
        snapshot=False,
        restore=False,
        snapshot_dir=run_dir / "snapshots",
        no_build=True,
        timeout=60,
        scenario="autonomous-intake",
    )


def _cookie(**overrides) -> http.cookiejar.Cookie:
    values = {
        "version": 0,
        "name": "factory_session",
        "value": "session-value",
        "port": None,
        "port_specified": False,
        "domain": "127.0.0.1",
        "domain_specified": False,
        "domain_initial_dot": False,
        "path": "/",
        "path_specified": True,
        "secure": False,
        "expires": 1893456000,
        "discard": False,
        "comment": None,
        "comment_url": None,
        "rest": {"HttpOnly": None},
        "rfc2109": False,
    }
    values.update(overrides)
    return http.cookiejar.Cookie(**values)


class StorageStateTests(unittest.TestCase):
    def test_exports_the_real_lwp_jar_as_playwright_storage_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            api = Api(_config(Path(directory)))
            api.jar.set_cookie(_cookie(name="factory_session"))
            api.jar.set_cookie(
                _cookie(
                    name="factory_csrf",
                    value="csrf-value",
                    secure=True,
                    rest={"SameSite": "Lax"},
                )
            )
            api._save()

            state = json.loads(api.config.storage_state.read_text(encoding="utf-8"))
            self.assertEqual(state["origins"], [])
            by_name = {cookie["name"]: cookie for cookie in state["cookies"]}
            self.assertEqual(set(by_name), {"factory_session", "factory_csrf"})
            session = by_name["factory_session"]
            self.assertEqual(session["domain"], "127.0.0.1")
            self.assertEqual(session["path"], "/")
            self.assertEqual(session["expires"], 1893456000)
            self.assertTrue(session["httpOnly"])
            self.assertFalse(session["secure"])
            csrf = by_name["factory_csrf"]
            self.assertTrue(csrf["secure"])
            self.assertEqual(csrf["sameSite"], "Lax")

    def test_session_cookie_uses_minus_one_expiry(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            api = Api(_config(Path(directory)))
            api.jar.set_cookie(_cookie(expires=None, discard=True))
            api._save()
            state = json.loads(api.config.storage_state.read_text(encoding="utf-8"))
            self.assertEqual(state["cookies"][0]["expires"], -1)

    def test_storage_state_file_is_owner_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            api = Api(_config(Path(directory)))
            api.jar.set_cookie(_cookie())
            api._save()
            mode = api.config.storage_state.stat().st_mode & 0o777
            self.assertEqual(mode, 0o600)

    def test_legacy_netscape_path_is_not_the_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            config = _config(Path(directory))
            self.assertFalse(hasattr(config, "netscape_cookies"))
            self.assertEqual(config.storage_state.name, "cookies.storage.json")


if __name__ == "__main__":
    unittest.main()
