#!/usr/bin/env bash
# Architecture lint: detect non-atomic entity creation with dependent records.
#
# When a handler creates a parent entity (e.g., workspace) and then separately
# creates dependent records (e.g., trust policies, bindings) via individual
# repository calls, a failure mid-sequence leaves a partially-initialized entity.
# The parent exists without all its required dependencies — violating domain
# invariants silently.
#
# This script detects handler functions that contain multiple distinct
# `state.<repo>.create(` calls without using a transactional wrapper.  When
# entity A's integrity depends on records B1..Bn, all creations must happen
# atomically — either through a single domain service method that uses a
# transaction, or by wrapping the calls in a transaction block.
#
# The pattern detected:
#   1. A function has 2+ distinct `state.<repo>.create(` calls where <repo>
#      names differ (e.g., `state.workspaces.create` + `state.policies.create`).
#   2. There is no transaction wrapper (`transaction`, `begin_transaction`,
#      `apply_trust_transition`, or similar atomic domain method) enclosing both.
#
# Legitimate patterns (non-findings):
#   - A function that creates multiple records of the SAME type in a loop
#     (e.g., seeding initial data) where each record is independent.
#   - Functions that use a transactional domain method for the dependent creation.
#
# Exempt a line with: // non-atomic-create:ok — <reason>
# Pre-existing sites are baselined in scripts/non-atomic-creation-exemptions.txt
# (`path:line` of the reported function, one per line); never add entries.
#
# See: specs/reviews/task-077.md F2 (workspace creation policy seeding)
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ ! -d "$SERVER_SRC" ]; then
    echo "Skipping non-atomic creation check: $SERVER_SRC not found"
    exit 0
fi

echo "Checking for non-atomic entity creation with dependent records..."

# Strategy: For each non-test .rs file, find functions that contain
# `state.<repo1>.create(` AND `state.<repo2>.create(` where repo1 != repo2,
# without a transaction wrapper between them.
#
# Implemented in python3 (like the sibling checks) so behavior is identical
# on gawk and mawk hosts: the previous embedded awk used gawk-only 3-arg
# match(), which made this check abort on mawk — a red gate on any host
# without gawk, regardless of the code.

GYRE_SCRIPT_DIR="$SCRIPT_DIR" python3 - "$SERVER_SRC" <<'PYEOF'
import os
import re
import sys
from pathlib import Path
EXEMPT_FILE = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'non-atomic-creation-exemptions.txt'
EXEMPTED = set()
if EXEMPT_FILE.exists():
    for _raw in EXEMPT_FILE.read_text().splitlines():
        _raw = _raw.split('#', 1)[0].strip()
        if _raw:
            EXEMPTED.add(_raw)

FN_RE = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)')
CREATE_RE = re.compile(r'state\.([a-z_]+)\.create\(')
TX_RE = re.compile(r'transaction|begin_transaction|apply_trust_transition|\.atomic\(')
EXEMPT = 'non-atomic-create:ok'
TEST_MOD_RE = re.compile(r'^\s*(#\[[^\]]*\]\s*)?(pub )?mod tests\b')

def fix_options():
    return (
        "\n"
        "  Fix options:\n"
        "    1. Use a transactional domain service method for the entire creation\n"
        "    2. Wrap all creations in a single database transaction\n"
        "    3. If intentional: add '// non-atomic-create:ok — <reason>' on each create line\n"
        "\n"
    )

def check_file(path):
    try:
        lines = path.read_text(encoding='utf-8', errors='replace').splitlines()
    except OSError:
        return 0
    violations = 0
    fn_name = ''
    fn_start = 0
    has_exempt = False
    has_transaction = False
    repos_seen = {}

    def flush(out):
        nonlocal violations
        if f"{path}:{fn_start}" in EXEMPTED:
            return
        if fn_name and not has_exempt and len(repos_seen) > 1 and not has_transaction:
            out.append(f"NON-ATOMIC CREATION: {fn_name} in {path}:{fn_start}")
            out.append(f"  Creates entities via {len(repos_seen)} different repositories without a transaction:")
            for repo, line_no in repos_seen.items():
                out.append(f"    - state.{repo}.create() at line {line_no}")
            out.append("")
            out.append("  If any creation fails mid-sequence, the parent entity exists without")
            out.append("  all its required dependent records — silently violating domain invariants.")
            out.append(fix_options())
            violations += 1

    out = []
    for idx, line in enumerate(lines, 1):
        # Test modules are outside the check's scope (mirrors the awk
        # pre-filter that dropped #[cfg(test)] create calls).
        if TEST_MOD_RE.match(line):
            flush(out)
            fn_name = ''
            break
        m = FN_RE.match(line)
        if m:
            flush(out)
            fn_name = m.group(1)
            fn_start = idx
            has_exempt = False
            has_transaction = False
            repos_seen = {}
            if fn_name.startswith('test_'):
                fn_name = ''
            continue
        if fn_name:
            if EXEMPT in line:
                has_exempt = True
            if TX_RE.search(line):
                has_transaction = True
            cm = CREATE_RE.search(line)
            if cm and cm.group(1) not in repos_seen:
                repos_seen[cm.group(1)] = idx
    flush(out)
    for line in out:
        print(line)
    return violations

def main():
    root = Path(sys.argv[1])
    total = 0
    for path in sorted(root.rglob('*.rs')):
        sp = str(path)
        if '/tests/' in sp or sp.endswith('_test.rs'):
            continue
        total += check_file(path)
    if total == 0:
        print("")
        print("Non-atomic creation check passed.")
        print("No handlers found with multi-repository creation without transaction wrapping.")
        return 0
    print("")
    print("Fix: Wrap related entity creations in a single transaction or use a")
    print("     transactional domain service method.")
    print("     Exempt with: // non-atomic-create:ok — <reason>")
    print("     Pre-existing sites: scripts/non-atomic-creation-exemptions.txt (frozen baseline)")
    print("See: specs/reviews/task-077.md F2 (non-atomic workspace+policy creation)")
    print(f"{total} violation(s) found.")
    return 1

sys.exit(main())
PYEOF
