#!/usr/bin/env python3
"""Summarize measurement JSON files into a compact Markdown/JSON report."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def load(directory: Path):
    results = []
    for path in sorted(directory.glob("*.json")):
        try:
            data = json.loads(path.read_text())
        except json.JSONDecodeError:
            continue
        if isinstance(data, dict) and "label" in data:
            results.append(data)
    return results


def main() -> int:
    directory = Path(sys.argv[1])
    results = load(directory)
    summary = {"results": results}
    (directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    lines = [
        "| label | exit | wall ms | peak RSS KiB | processes | threads | sampled CPU ms |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for result in results:
        lines.append(
            "| {label} | {exit_code} | {wall_ms} | {peak_rss_kib} | {peak_processes} | "
            "{peak_threads} | {sampled_cpu_ms} |".format(**result)
        )
    (directory / "summary.md").write_text("\n".join(lines) + "\n")

    internal = []
    for report_path in sorted(directory.glob("prototype-*/report.json")):
        data = json.loads(report_path.read_text())
        internal.append(
            {
                "label": report_path.parent.name,
                "success": data.get("success"),
                "wall_ms": data.get("metrics", {}).get("wall_ms"),
                "first_child_ms": data.get("metrics", {}).get("first_child_ms"),
                "callback_queue_latency_ms": data.get("metrics", {}).get(
                    "callback_queue_latency_ms"
                ),
                "effect_to_callback_ms": data.get("metrics", {}).get(
                    "effect_to_callback_ms"
                ),
                "duplicate_external_effects": data.get("metrics", {}).get(
                    "duplicate_external_effects"
                ),
                "event_count": data.get("metrics", {}).get("event_count"),
                "candidate_sha256": (data.get("candidate") or {}).get("candidate_sha256"),
            }
        )
    if internal:
        summary["prototype_internal"] = internal
        (directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        lines.append("")
        lines.append(
            "| prototype | success | wall ms | first child ms | callback queue ms | "
            "effect→callback ms | duplicate effects | events |"
        )
        lines.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |")
        for item in internal:
            lines.append(
                "| {label} | {success} | {wall_ms} | {first_child_ms} | "
                "{callback_queue_latency_ms} | {effect_to_callback_ms} | "
                "{duplicate_external_effects} | {event_count} |".format(**item)
            )
        (directory / "summary.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
