#!/usr/bin/env python3
"""Reject rustfmt violations on changed Rust lines, without reformatting old debt.

Pass the base commit as the first argument. For a merge commit, that is HEAD^1.
"""

import difflib
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def git(*args):
    return subprocess.run(["git", *args], check=True, text=True, capture_output=True).stdout


def changed_lines(base, path):
    patch = git("diff", "--unified=0", base, "HEAD", "--", path)
    lines = set()
    for start, count in re.findall(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", patch, re.M):
        first = int(start)
        length = int(count) if count else 1
        lines.update(range(first, first + length))
    return lines


def format_violations(path):
    original = Path(path).read_text().splitlines(keepends=True)
    with tempfile.TemporaryDirectory() as directory:
        copy = Path(directory) / Path(path).name
        copy.write_text("".join(original))
        subprocess.run(["rustfmt", "--edition", "2021", "--config", "skip_children=true", str(copy)],
                       check=True, text=True, capture_output=True)
        formatted = copy.read_text().splitlines(keepends=True)
    bad = set()
    for tag, i1, i2, _, _ in difflib.SequenceMatcher(None, original, formatted, autojunk=False).get_opcodes():
        if tag == "equal":
            continue
        # A formatting insertion is anchored to the preceding source line.
        if i1 == i2:
            bad.add(max(1, i1))
        else:
            bad.update(range(i1 + 1, i2 + 1))
    return bad


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check-rustfmt-diff.py BASE_COMMIT")
    base = sys.argv[1]
    paths = git("diff", "--name-only", "--diff-filter=ACMRT", base, "HEAD", "--", "*.rs").splitlines()
    failed = False
    for path in paths:
        overlap = changed_lines(base, path) & format_violations(path)
        if overlap:
            failed = True
            print(f"rustfmt: {path}: changed lines need formatting: {', '.join(map(str, sorted(overlap)[:12]))}")
    if failed:
        raise SystemExit(1)
    print(f"rustfmt: changed lines clean ({len(paths)} Rust files checked)")


if __name__ == "__main__":
    main()
