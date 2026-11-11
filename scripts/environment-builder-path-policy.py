#!/usr/bin/env python3
"""Canonical path policy for the rootless builder wrapper.

The wrapper must never mount `/`, `$HOME`, the repository root, a container
engine store or a credential directory. Writable cache/store/run paths must
stay under an explicit cache base. All checks run after ``realpath`` and reject
``..`` components, so a symlink cannot escape the policy.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

PROTECTED_COMPONENTS = {".ssh", ".gnupg", ".config", ".git"}
PROTECTED_SEQUENCES = (".local/share/containers", "podman.sock", "docker.sock")
EXACT_FORBIDDEN = {"/", "/root", "/home", "/etc", "/proc", "/sys", "/dev", "/run", "/var/run"}


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(1)


def canonical(path: str) -> str:
    if not path:
        fail("an empty path is not allowed")
    if any(part == ".." for part in Path(path).parts):
        fail(f"path {path!r} contains a '..' component")
    return os.path.realpath(path)


def is_under(child: str, parent: str) -> bool:
    if parent == os.sep:
        return True
    parent = parent.rstrip(os.sep)
    return child == parent or child.startswith(parent + os.sep)


def reject_protected(real: str) -> None:
    parts = Path(real).parts
    if any(part in PROTECTED_COMPONENTS for part in parts):
        fail(f"path {real} contains a protected component")
    if any(sequence in real for sequence in PROTECTED_SEQUENCES):
        fail(f"path {real} exposes a protected container/credential location")


def validate_cache_base(path: str, home: str, root: str) -> str:
    real = canonical(path)
    if real in EXACT_FORBIDDEN or real == os.path.realpath(home):
        fail(f"cache base {real} is a protected host path")
    if is_under(real, os.path.realpath(root)):
        fail(f"cache base {real} is inside the repository root")
    reject_protected(real)
    return real


def validate_cache(path: str, base: str, home: str, root: str) -> str:
    real = canonical(path)
    if not is_under(real, base):
        fail(f"cache {real} is not under the allowed cache base {base}")
    if is_under(real, os.path.realpath(root)):
        fail(f"cache {real} is inside the repository root")
    reject_protected(real)
    return real


def validate_child(path: str, cache: str, home: str, root: str) -> str:
    real = canonical(path)
    if not is_under(real, cache):
        fail(f"path {real} is not under the builder cache {cache}")
    if is_under(real, os.path.realpath(root)):
        fail(f"path {real} is inside the repository root")
    reject_protected(real)
    return real


def validate_workspace(path: str, home: str, root: str) -> str:
    real = canonical(path)
    home_real = os.path.realpath(home)
    root_real = os.path.realpath(root)
    if real in EXACT_FORBIDDEN or real in {home_real, root_real}:
        fail(f"workspace {real} is a protected host path")
    if is_under(real, root_real):
        fail(f"workspace {real} is inside the repository root")
    reject_protected(real)
    if not os.path.isdir(real):
        fail(f"workspace {real} is not a directory")
    return real


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("cache-base", "cache", "child", "workspace"))
    parser.add_argument("--path", required=True)
    parser.add_argument("--base", default="")
    parser.add_argument("--home", required=True)
    parser.add_argument("--root", required=True)
    args = parser.parse_args()
    if args.kind == "cache-base":
        print(validate_cache_base(args.path, args.home, args.root))
    elif args.kind == "cache":
        print(validate_cache(args.path, args.base, args.home, args.root))
    elif args.kind == "child":
        print(validate_child(args.path, args.base, args.home, args.root))
    else:
        print(validate_workspace(args.path, args.home, args.root))
    return 0


if __name__ == "__main__":
    sys.exit(main())
