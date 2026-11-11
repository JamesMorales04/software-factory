#!/usr/bin/env python3
"""Summarize CycloneDX and sbomnix CSV SBOMs for this spike.

Usage: ./summarize-sbom.py node.cdx.json node.csv rust.cdx.json rust.csv

The two formats answer different questions:

- CycloneDX `components[].licenses` says whether a license entry exists at all.
- The sbomnix CSV `meta_license_spdxid` says whether nixpkgs metadata resolved
  to an SPDX identifier. A nixpkgs short name (for example `mit`) can exist
  without an SPDX identifier.
"""

import collections
import csv
import json
import sys


def summarize_cdx(path: str) -> None:
    with open(path, encoding="utf-8") as handle:
        document = json.load(handle)

    components = document.get("components", [])
    licenses = collections.Counter()
    no_entry = 0
    no_spdx_id = 0
    for component in components:
        entries = component.get("licenses") or []
        if not entries:
            no_entry += 1
            continue
        has_spdx_id = False
        for entry in entries:
            license_data = entry.get("license", {})
            if license_data.get("id"):
                has_spdx_id = True
                name = license_data["id"]
            else:
                name = license_data.get("name") or entry.get("expression") or "unknown"
            licenses[name] += 1
        if not has_spdx_id:
            no_spdx_id += 1

    print(f"{path}: {len(components)} components")
    print(f"  no license entry:        {no_entry}")
    print(f"  entry without SPDX id:   {no_spdx_id}")
    for name, count in licenses.most_common():
        print(f"  {count:4d}  {name}")


def summarize_csv(path: str) -> None:
    with open(path, newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))

    no_short = 0
    no_spdx = 0
    short_but_no_spdx = 0
    for row in rows:
        short = (row.get("meta_license_short") or "").strip()
        spdx = (row.get("meta_license_spdxid") or "").strip()
        if not short:
            no_short += 1
        if not spdx:
            no_spdx += 1
        elif not short:
            # An SPDX id without a short name is unusual but not a missing SPDX.
            pass
        if short and not spdx:
            short_but_no_spdx += 1

    print(f"{path}: {len(rows)} rows")
    print(f"  no nixpkgs license short name: {no_short}")
    print(f"  no SPDX identifier:            {no_spdx}")
    print(f"  short name but no SPDX id:     {short_but_no_spdx}")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    for path in sys.argv[1:]:
        if path.endswith(".csv"):
            summarize_csv(path)
        else:
            summarize_cdx(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
