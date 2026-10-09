#!/usr/bin/env python3
"""Coverage parsing and stable code generations for bounded fidelity audits."""
import hashlib
import json
import re
from datetime import date
from pathlib import Path


def rows(body):
    result = []
    for line in body.splitlines():
        parts = [part.strip() for part in line.split("|")]
        if len(parts) >= 6 and parts[1].isdigit():
            result.append({"id": int(parts[1]), "section": parts[2], "status": parts[4],
                           "tasks": re.findall(r"task-\d+", parts[5])})
    return result


def code_generation(git, ref):
    objects = {}
    for path in ("crates", "web/src", "scripts", "specs/system", "specs/development", "specs/GOAL.md"):
        try:
            objects[path] = git("rev-parse", f"{ref}:{path}")
        except RuntimeError:
            objects[path] = None
    return hashlib.sha256(json.dumps(objects, sort_keys=True).encode()).hexdigest()


def summary(matrices, date):
    lines = ["# Spec Coverage Summary", "", f"**Last updated:** {date}", "",
             "| Spec | Total | n/a | Not Started | Assigned | Implemented | Verified | Coverage |",
             "|------|-------|-----|-------------|----------|-------------|----------|----------|"]
    total = [0] * 6
    categories = {"n/a": 1, "not-started": 2, "task-assigned": 3, "in-progress": 3,
                  "implemented": 4, "verified": 5}
    for path, body in sorted(matrices.items()):
        count = [0] * 6
        for row in rows(body):
            if row["status"] not in categories:
                raise ValueError(f"{path}: unrecognized coverage status {row['status']}")
            count[0] += 1; count[categories[row["status"]]] += 1
        total = [left + right for left, right in zip(total, count)]
        applicable = count[0] - count[1]
        percent = (count[4] + count[5]) * 100 // applicable if applicable else 0
        lines.append(f"| {path.rsplit('/', 1)[-1]} | " + " | ".join(map(str, count)) + f" | {percent}% |")
    applicable = total[0] - total[1]
    percent = (total[4] + total[5]) * 100 // applicable if applicable else 0
    lines.append("| **TOTAL** | " + " | ".join(f"**{value}**" for value in total) + f" | **{percent}%** |")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    directory = Path("specs/coverage")
    matrices = {str(path): path.read_text() for path in directory.rglob("*.md") if path.name != "SUMMARY.md"}
    (directory / "SUMMARY.md").write_text(summary(matrices, date.today().isoformat()))
