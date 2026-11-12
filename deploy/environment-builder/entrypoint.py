#!/usr/bin/env python3
"""Executable entrypoint for the Software Factory Nix/devenv builder image."""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from environment_builder.cli import main  # noqa: E402

if __name__ == "__main__":
    sys.exit(main())
