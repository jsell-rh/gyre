#!/usr/bin/env bash
# check-inert-enforcement.sh — Flag enforcement/evaluation calls whose result
# is discarded (statement position) in non-test Rust.
#
# Procedural background (specs/reviews/task-077.md F5): the merge processor
# called evaluate_attestation_abac(...) twice and discarded both results —
# documented "audit-only — logged but not enforced (merge proceeds
# regardless)". The merge processor also never constructed the
# subject.type: "system" / subject.id: "merge-processor" ABAC identity the
# spec mandates, so the Supervised trust policy was inert data. The
# fix-class rule: an evaluation exists to gate an action; a call whose
# return value is not bound and not branched on gates nothing.
#
# Flagged: evaluate_*/enforce_*/verify_*/validate_* calls in statement
# position (line starts with the call, not `let`/`if`/`return`/`match`),
# in non-test code. Both free-function form (evaluate_x(...) at statement
# start) and method form (expr.evaluate_x(...) as the whole statement).
#
# validate_* (added by the task-099 process revision, F2: orchestrator.rs
# called validate_persona(...) and discarded the () result — and it read
# state.meta_specs, a store the bootstrap never populates for personas)
# excludes consumption shapes on the same line: `?` propagation,
# `.map_err`/`.await`/`.map(`/`.ok_` chains, and `&mut` arguments (the
# errors-accumulator pattern where the validator's effect flows through
# the mutable argument, not the return value); it also skips a call whose
# next line starts with `.` (a rustfmt continuation of a consumed chain).
#
# NOT flagged: calls in #[cfg(test)]/mod tests regions; calls bound to a
# variable, branched on, or returned; lines carrying `// x:ok`.
#
# Exempt a line with: `// enforcement:ok — <reason>`
#
# Usage: bash scripts/check-inert-enforcement.sh [paths...]  (default crates/)

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

# Statement-position free-function call: line begins (after whitespace) with
# evaluate_*/enforce_*/verify_*/validate_* followed by '('.
FN_CALL = re.compile(r'^\s*(evaluate_|enforce_|verify_|validate_)\w*\s*\(')

# Statement-position method call: a receiver chain ending in .evaluate_x(...)
# as the whole statement. The line must not start with let/if/return/match.
METH_CALL = re.compile(r'^\s*[\w.\[\]()]+\.((evaluate_|enforce_|verify_|validate_)\w*)\s*\(')

# Things that make a call non-inert on the same line.
# validate_* consumption shapes on the same line: `?` anywhere, a method
# chain on the result, or an errors-accumulator `&mut` argument. NOTE:
# `.await` alone is NOT consumption — an awaited-but-discarded result is
# still inert (task-099 F2: validate_persona(...).await;).
VALIDATE_CONSUMED = re.compile(r'\?|\.map_err\(|\.map\(|\.ok_|&mut')

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
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'inert-enforcement-exemptions.txt'
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
            if key in exempt or '// enforcement:ok' in line or '// x:ok' in line:
                continue
            if test_start is not None and lineno - 1 >= test_start:
                continue
            hit = FN_CALL.match(line) or (METH_CALL.match(line) and not BOUND.search(line))
            if not hit:
                continue
            if line.lstrip().startswith('validate_'):
                # validate_* consumption shapes: `?`/chain on the same line,
                # or a rustfmt continuation whose next line starts with '.'
                # (the result feeds a consumed method chain).
                if VALIDATE_CONSUMED.search(line):
                    continue
                nxt = lines[lineno] if lineno < len(lines) else ''
                if nxt.lstrip().startswith('.'):
                    continue
            print(f"ERROR: discarded evaluation result at {path}:{lineno}")
            print(f"  {line.strip()}")
            print("  this evaluate_/enforce_/verify_/validate_ call is in statement position —")
            print("  result is not bound, branched on, or returned, so it gates nothing.")
            print("  This is the specs/reviews/task-077.md F5 flaw class (audit-only")
            print("  enforcement: merge proceeds regardless of the policy decision).")
            print("  Bind the result and branch on it, or exempt with:")
            print("  // enforcement:ok — <reason>")
            print()
            errors += 1
    if errors:
        print(f"check-inert-enforcement: FAILED — {errors} inert evaluation site(s) found")
        sys.exit(1)
    print("check-inert-enforcement: OK")

main()
PYEOF
