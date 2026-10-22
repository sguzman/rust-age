#!/usr/bin/env python3
"""Generate an auditable roadmap from the task progress ledger."""
import argparse
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

def pct(xs):
    return (sum(t["progress"] for t in xs) * 2 // len(xs) + 1) // 2 if xs else 0

def bar(n):
    filled = (n + 5) // 10
    return "█" * filled + "░" * (10 - filled) + f" {n}%"

def render(d):
    lines = ["# Rust Age roadmap", "",
             "Generated from [`progress.json`](progress.json) with `python3 tools/render_roadmap.py`.",
             "Progress is the arithmetic mean of atomic task percentages. Completed tasks are retained in",
             "the ledger but omitted from this display. Partial scaffold ≠ playable game.",
             "Fallout 2 (M7) is gated on the accepted Fallout 1 Linux release (M6).", ""]
    for m in d["milestones"]:
        all_tasks = [t for p in m["phases"] for t in p["tasks"]]
        lines += [f"## {m['emoji']} {m['id']} — {m['name']} — `{bar(pct(all_tasks))}`",
                  f"**Acceptance gate:** {m['gate']}", ""]
        for p in m["phases"]:
            lines += [f"### {p['emoji']} {p['id']} — {p['name']} — `{bar(pct(p['tasks']))}`", ""]
            for t in p["tasks"]:
                if t["progress"] != 100:
                    lines.append(f"- {t['emoji']} {t['id']} — {t['name']} — `{bar(t['progress'])}`")
            lines.append("")
    lines += ["## Progress accounting", "",
              "- Every active milestone, phase and task touched by a progress report gets a percent bar.",
              "- Completed tasks (100%) disappear from progress reports and this view; historical evidence remains in `progress.json`.",
              "- Do not assign 100% without evidence; differentiate code written, CI-tested and game-tested.",
              "- Do not commit original game data, private locations, accounts or personal conversations.", ""]
    return "\n".join(lines)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = json.loads((ROOT / "docs/progress.json").read_text())
    for m in data["milestones"]:
        for p in m["phases"]:
            for t in p["tasks"]:
                assert 0 <= t["progress"] <= 100
                if t["progress"] == 100:
                    assert t.get("evidence"), t["id"]
    output = render(data)
    path = ROOT / "docs/ROADMAP.md"
    if args.check:
        assert path.read_text() == output, "Generated roadmap differs; rerun without --check"
    else:
        path.write_text(output)

if __name__ == "__main__":
    main()
