#!/usr/bin/env bash
# check-relative-path-defaults.sh — Flag relative ("./...") string-literal
# path defaults, and dynamic relative path defaults, in non-test Rust.
#
# Procedural background (specs/reviews/task-106.md R2 F6):
# the server's repos_root defaults to the RELATIVE "./repos"
# (lib.rs, main.rs), repo.path stores that prefix verbatim in the DB, and
# spawn.rs passes it un-canonicalized to `jj git init --git-repo` /
# `jj workspace add`. jj resolves paths against the command's cwd, not the
# server's intended root, so provisioning fails in default deployments —
# and every failure is swallowed at warn. The fix-class rule: a path (or
# path-derived value) that a child process will resolve must be absolute
# at rest or canonicalized at the call site. `jj_ops.rs:82`'s own comment
# ("jj requires absolute paths here") proves the class was known — the
# fix authors canonicalized one call site and missed its siblings.
#
# Flagged (Check 1): string literals starting "./<letter>" — the direct
# signature of a relative-path default or relative path value — in
# non-test code.
#
# Flagged (Check 2, added by the task-099 process revision, F6): a
# `PathBuf::from(...)` / `Path::new(...)` (optionally `std::path::`-
# qualified) used as an `unwrap_or` / `unwrap_or_else` fallback —
# `.unwrap_or_else(|| std::path::PathBuf::from(&repo_name))` defaults the
# path to a bare name resolved against the process cwd. This is the
# dynamic sibling of the literal class: same failure (child processes and
# filesystem consumers resolve it against the wrong root in default
# deployments), but it evades the literal pattern because the relative
# component comes from a variable, not a string literal.
#
# NOT flagged: constructions inside #[cfg(test)]/mod tests regions;
# starts_with("./") prefix CHECKS (a comparison, not a path value — the
# literal is exactly "./" with nothing following); lines carrying the
# inline exemption marker.
#
# Exempt a line with: `// path:ok — <reason>`
#
# Usage: bash scripts/check-relative-path-defaults.sh [paths...]  (default crates/)

set -uo pipefail

if [ $# -eq 0 ]; then
    set -- crates/
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

GYRE_SCRIPT_DIR="$SCRIPT_DIR" python3 - "$@" <<'PYEOF'
import os
import re
import sys
from pathlib import Path

TEST_START = re.compile(r'\s*(#\[[^\]]*\]\s*)?(mod tests|#\[cfg\(test\)\])')
# "./" followed by a path segment — excludes the bare "./" of starts_with checks.
REL_PATH = re.compile(r'"\.\/[A-Za-z]')
# unwrap_or/unwrap_or_else fallback that constructs a path from a dynamic
# value — the default is a bare name resolved against the process cwd.
DYN_REL_PATH = re.compile(
    r'\.unwrap_or(?:_else)?\(\s*(?:\|\|\s*)?'
    r'(?:std::path::)?(?:PathBuf|Path)::(?:from|new)\('
)

def find_files(paths):
    for p in paths:
        path = Path(p)
        if path.is_dir():
            yield from sorted(path.rglob('*.rs'))
        elif path.suffix == '.rs':
            yield path

def main():
    errors = 0
    exempt = set()
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'relative-path-defaults-exemptions.txt'
    if exempt_path.exists():
        for raw in exempt_path.read_text().splitlines():
            raw = raw.split('#', 1)[0].strip()
            if raw:
                exempt.add(raw)
    for path in find_files(sys.argv[1:]):
        try:
            text = path.read_text(encoding='utf-8', errors='replace')
        except OSError:
            continue
        lines = text.splitlines()
        test_start = None
        for i, line in enumerate(lines):
            if TEST_START.match(line):
                test_start = i
                break
        for lineno, line in enumerate(lines, 1):
            key = f"{path}:{lineno}"
            if key in exempt or '// path:ok' in line:
                continue
            if test_start is not None and lineno - 1 >= test_start:
                continue
            if REL_PATH.search(line):
                print(f"ERROR: relative path literal at {path}:{lineno}")
                print(f"  {line.strip()}")
                print("  A \"./...\" path stored or passed to a child process is resolved")
                print("  against the COMMAND's cwd, not the server's intended root —")
                print("  subprocess provisioning (jj/git/docker) silently breaks in default")
                print("  deployments. Canonicalize at rest or at the call site.")
                print("  This is the specs/reviews/task-106.md R2-F6 flaw class.")
                print("  Use an absolute default, or exempt with:")
                print("  // path:ok — <reason>")
                print()
                errors += 1
            if DYN_REL_PATH.search(line):
                print(f"ERROR: dynamic relative path default at {path}:{lineno}")
                print(f"  {line.strip()}")
                print("  An unwrap_or/unwrap_or_else fallback constructing a path from a")
                print("  bare name (PathBuf::from(name)) defaults to a RELATIVE path —")
                print("  resolved against the process cwd, not the caller's intended root.")
                print("  In default deployments (flag/argument omitted) every child process")
                print("  or filesystem consumer resolves it from an arbitrary directory.")
                print("  This is the specs/reviews/task-099.md F6 flaw class (dynamic")
                print("  sibling of the task-106 R2-F6 literal class).")
                print("  Canonicalize at rest or at the call site, or exempt with:")
                print("  // path:ok — <reason>")
                print()
                errors += 1
    if errors:
        print(f"check-relative-path-defaults: FAILED — {errors} relative path literal(s) found")
        sys.exit(1)
    print("check-relative-path-defaults: OK")

main()
PYEOF
