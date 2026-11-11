"""Unit tests for the builder host-path policy.

The wrapper uses the same standalone script; these tests cover the protected
descendants that an exact $HOME/$ROOT comparison would miss.
"""

from __future__ import annotations

import importlib.util
import os
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
POLICY = HERE.parents[2] / "scripts" / "environment-builder-path-policy.py"


def load_policy():
    spec = importlib.util.spec_from_file_location("builder_path_policy", POLICY)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


POLICY_MODULE = load_policy()


class PathPolicyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.home = str(Path.home())
        self.root = str(HERE.parents[3])

    def test_cache_base_accepts_default_and_rejects_ssh(self) -> None:
        base = os.path.join(self.home, ".cache", "software-factory")
        self.assertEqual(
            POLICY_MODULE.validate_cache_base(base, self.home, self.root),
            os.path.realpath(base),
        )
        with self.assertRaises(SystemExit):
            POLICY_MODULE.validate_cache_base(os.path.join(self.home, ".ssh"), self.home, self.root)
        with self.assertRaises(SystemExit):
            POLICY_MODULE.validate_cache_base(os.path.join(self.root, ".cache"), self.home, self.root)

    def test_cache_rejects_protected_descendant(self) -> None:
        with self.assertRaises(SystemExit):
            POLICY_MODULE.validate_cache(
                os.path.join(self.home, ".ssh", "cache"),
                os.path.join(self.home, ".cache", "software-factory"),
                self.home,
                self.root,
            )

    def test_workspace_rejects_ssh_and_repo_git(self) -> None:
        with self.assertRaises(SystemExit):
            POLICY_MODULE.validate_workspace(os.path.join(self.home, ".ssh"), self.home, self.root)
        with self.assertRaises(SystemExit):
            POLICY_MODULE.validate_workspace(os.path.join(self.root, ".git"), self.home, self.root)
        with self.assertRaises(SystemExit):
            POLICY_MODULE.validate_workspace(self.root, self.home, self.root)

    def test_workspace_accepts_temp_project(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(
                POLICY_MODULE.validate_workspace(directory, self.home, self.root),
                os.path.realpath(directory),
            )

    def test_child_must_stay_under_cache_and_reject_dotdot(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = os.path.join(directory, "cache")
            os.makedirs(cache)
            inside = os.path.join(cache, "runs", "one")
            os.makedirs(inside)
            self.assertEqual(
                POLICY_MODULE.validate_child(inside, cache, self.home, self.root),
                os.path.realpath(inside),
            )
            with self.assertRaises(SystemExit):
                POLICY_MODULE.validate_child(os.path.join(directory, "outside"), cache, self.home, self.root)
            with self.assertRaises(SystemExit):
                POLICY_MODULE.validate_child(cache + "/../outside", cache, self.home, self.root)

    def test_symlink_escape_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = os.path.join(directory, "cache")
            outside = os.path.join(directory, "outside")
            os.makedirs(cache)
            os.makedirs(outside)
            link = os.path.join(cache, "escape")
            os.symlink(outside, link)
            with self.assertRaises(SystemExit):
                POLICY_MODULE.validate_child(link, cache, self.home, self.root)


if __name__ == "__main__":
    unittest.main()
