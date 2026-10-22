#!/usr/bin/env python3
"""Generate the strict per-compilation-unit Fallout 1 CE port matrix."""
import argparse
import json
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LABELS = {
    "partial": "🟡 Partial",
    "unported": "⚪ Not ported",
    "complete": "✅ Complete",
}

def render(data):
    entries = data["files"]
    assert len({entry["source"] for entry in entries}) == len(entries), "Duplicate C++ path"
    assert all(entry["status"] in LABELS for entry in entries)
    groups = defaultdict(list)
    for entry in entries:
        groups["/".join(entry["source"].split("/")[:-1])].append(entry)
    partials = sum(entry["status"] == "partial" for entry in entries)
    completed = sum(entry["status"] == "complete" for entry in entries)
    lines = [
        "# 🧩 Community Edition C++ → Rust port matrix", "",
        "**Source:** [Fallout 1 Community Edition pinned baseline](https://github.com/alexbatalov/fallout1-ce/commit/0609bcfd0ec40ff0571d0f57fab2821eb461dc8b).",
        "Generated from [`port-matrix.json`](port-matrix.json); do not manually edit this document.",
        f"**Core compilation units:** {len(entries)}. **Complete:** {completed}. **Partial:** {partials}. **Not ported:** {len(entries)-partials-completed}.",
        "Partial means a narrow reader/function exists, not that the entire C++ implementation has been replaced.",
        "The archive is not yet playable; the source matrix is not a gameplay-progress percentage.", ""
    ]
    for directory, group in groups.items():
        lines += [f"## {directory} ({len(group)})", "",
                  "| C++ compilation unit | Rust counterpart | Status |",
                  "|---|---|---|"]
        for entry in group:
            rust = ", ".join("`" + name + "`" for name in entry["rust"]) if entry["rust"] else "—"
            lines.append(f"| `{entry['source']}` | {rust} | {LABELS[entry['status']]} |")
        lines.append("")
    lines += ["## Method", "",
        "The manifest covers C++ compilation units in the pinned CE root CMake `target_sources` list. Header-only code, third-party codecs, platform-specific glue, behavioral overlap and game-data verification require separate audits. File-level completion is intentionally stricter than task percentage accounting; do not count partial ports as complete.", ""]
    return "\n".join(lines)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = json.loads((ROOT / "docs/port-matrix.json").read_text())
    rendered = render(data)
    dest = ROOT / "docs/PORT_MATRIX.md"
    if args.check:
        assert dest.read_text() == rendered, "Run tools/render_port_matrix.py to regenerate"
    else:
        dest.write_text(rendered)

if __name__ == "__main__":
    main()
