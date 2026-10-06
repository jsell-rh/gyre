#!/usr/bin/env python3
"""Fail Clippy warnings on changed Rust lines and all compiler errors.

The repository has existing Clippy warnings. This keeps the gate strict for
new code without requiring unrelated cleanup in every feature branch.
"""

import json
import os
import re
import subprocess
import sys


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


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check-clippy-diff.py BASE_COMMIT")
    base = sys.argv[1]
    changed = {
        path: changed_lines(base, path)
        for path in git("diff", "--name-only", "--diff-filter=ACMRT", base, "HEAD", "--", "*.rs").splitlines()
    }
    env = os.environ.copy()
    env["SKIP_WEB_BUILD"] = "1"
    result = subprocess.run(
        ["cargo", "clippy", "--all-targets", "--all-features", "--message-format=json", "--", "-W", "clippy::all"],
        capture_output=True, text=True, env=env,
    )
    failures = []
    ignored = 0
    for line in result.stdout.splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if item.get("reason") != "compiler-message":
            continue
        message = item["message"]
        level = message.get("level")
        if level not in ("warning", "error"):
            continue
        spans = [span for span in message.get("spans", []) if span.get("is_primary")]
        new_line = any(
            span.get("line_start", 0) <= number <= span.get("line_end", 0)
            for span in spans
            for number in changed.get(span.get("file_name", ""), ())
        )
        if level == "error" or new_line:
            code = (message.get("code") or {}).get("code") or level
            where = spans[0] if spans else {}
            failures.append(f"{where.get('file_name', '?')}:{where.get('line_start', '?')}: {code}: {message.get('message', '')}")
        else:
            ignored += 1
    for failure in failures:
        print(failure, file=sys.stderr)
    if result.returncode or failures:
        print(result.stderr[-4000:], file=sys.stderr)
        raise SystemExit(1)
    print(f"clippy: changed lines clean ({len(changed)} Rust files, {ignored} existing warnings outside changes)")


if __name__ == "__main__":
    main()
