#!/usr/bin/env python3
"""Summarize CycloneDX SBOMs produced by sbomnix for this spike.

Usage: ./summarize-sbom.py node.cdx.json [rust.cdx.json ...]
"""

import collections
import json
import sys


def summarize(path: str) -> None:
    with open(path, encoding="utf-8") as handle:
        document = json.load(handle)

    components = document.get("components", [])
    licenses = collections.Counter()
    without_license = 0
    for component in components:
        entries = component.get("licenses") or []
        if not entries:
            without_license += 1
            continue
        for entry in entries:
            license_data = entry.get("license", {})
            name = (
                license_data.get("id")
                or license_data.get("name")
                or entry.get("expression")
                or "unknown"
            )
            licenses[name] += 1

    print(f"{path}: {len(components)} components, {without_license} without license")
    for name, count in licenses.most_common():
        print(f"  {count:4d}  {name}")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    for path in sys.argv[1:]:
        summarize(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
