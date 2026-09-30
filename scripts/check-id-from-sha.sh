#!/usr/bin/env bash
# check-id-from-sha.sh — Flag Id::new(...) constructions whose argument
# references a sha variable in non-test Rust.
#
# Procedural background (specs/reviews/task-095.md R2-3):
# apply_revert_side_effects did `let revert_mr_id = Id::new(revert_sha.to_string())`
# — the domain field MergeRequest::revert_mr_id is named and documented as a
# reference to the revert MR, but was populated with a git commit SHA because
# no revert MR object exists. Any future consumer joining revert_mr_id
# against the merge_requests table gets a silent miss. The fix-class rule: a
# field named for one kind of identifier must hold that kind — a sha flowing
# into an Id constructor named for another entity is the direct signature.
#
# Flagged: `Id::new(x)` where x (or a prefix of x) contains `sha`
# (case-insensitive) — e.g. Id::new(revert_sha.to_string()),
# Id::new(head_sha.into()) — in non-test code.
#
# NOT flagged: constructions in #[cfg(test)]/mod tests regions; Id::new on
# arguments with no sha reference.
#
# Exempt a line with: `// id:ok — <reason>`
#
# Usage: bash scripts/check-id-from-sha.sh [paths...]  (default crates/)

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
ID_NEW = re.compile(r'Id::new\(\s*([A-Za-z_][A-Za-z0-9_]*)')

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
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'id-from-sha-exemptions.txt'
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
            if key in exempt or '// id:ok' in line:
                continue
            if test_start is not None and lineno - 1 >= test_start:
                continue
            m = ID_NEW.search(line)
            if not m:
                continue
            arg = m.group(1)
            if not re.search(r'sha', arg, re.I):
                continue
            print(f"ERROR: commit SHA flowing into Id field at {path}:{lineno}")
            print(f"  {line.strip()}")
            print("  Id::new is being fed a sha-named value — if the field being populated")
            print("  is named for another entity (e.g. revert_mr_id), this stores a commit")
            print("  SHA where an MR id belongs: joins against the real table silently miss.")
            print("  This is the specs/reviews/task-095.md R2-3 flaw class.")
            print("  Use the correct identifier source, or exempt with:")
            print("  // id:ok — <reason>")
            print()
            errors += 1
    if errors:
        print(f"check-id-from-sha: FAILED — {errors} sha-into-Id construction(s) found")
        sys.exit(1)
    print("check-id-from-sha: OK")

main()
PYEOF
