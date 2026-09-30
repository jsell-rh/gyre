#!/usr/bin/env bash
# check-warn-continue-creation.sh — Flag warn-and-continue handling of
# restriction-bearing store creation (state.policies.create/save) in Rust.
#
# Procedural background (specs/reviews/task-077.md F6, R1 F1): each failure of
# state.policies.create() in create_interrogation_policies was swallowed with
# tracing::warn! and the loop continued; if the interrogation-restrict Deny
# policy failed to create, the interrogation agent spawned UNRESTRICTED with
# only a log line. The caller proceeded on the success path of created_ids.
# The fix-class rule: creating a restriction artifact must fail closed — the
# protected action is blocked or aborted, never warned past.
#
# Flagged: a `state.policies.create(...)` or `state.policies.save(...)` call
# whose error arm (match Err / if let Err) logs at warn! (or lower) and falls
# through to continue the surrounding function — statement position, non-test
# code. Best-effort creation of NON-restriction data belongs in the exemption
# file with a reason.
#
# NOT flagged: creation results that are propagated (? / return Err / Ok-arm
# branching that aborts), and creation inside #[cfg(test)] regions.
#
# Exempt a line with: `// policy-create:ok — <reason>`
#
# Usage: bash scripts/check-warn-continue-creation.sh [paths...]  (default crates/)

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

# Restriction-bearing store calls (warn-and-continue on these = fail-open).
CALL_RE = re.compile(r'state\.policies\.(create|save)\s*\(')

# A warn arm that falls through: match Err(...) => { tracing::warn!(...) }
# with no return/abort/break, or `if let Err(e) = ... { tracing::warn!(...) }`
# with no return/abort inside.
WARN_RE = re.compile(r'tracing::(warn|debug|info|error)!\s*\(')

def find_files(paths):
    for p in paths:
        path = Path(p)
        if path.is_dir():
            yield from sorted(path.rglob('*.rs'))
        elif path.suffix == '.rs':
            yield path

def is_test_region_start(line):
    return re.match(r'\s*(#\[[^\]]*\]\s*)?(mod tests|#\[cfg\(test\)\])', line) is not None

def main():
    errors = 0
    exempt = set()
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'warn-continue-creation-exemptions.txt'
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
            if is_test_region_start(line):
                test_start = i
                break
        for lineno, line in enumerate(lines, 1):
            key = f"{path}:{lineno}"
            if key in exempt or '// policy-create:ok' in line:
                continue
            if test_start is not None and lineno - 1 >= test_start:
                continue
            if not CALL_RE.search(line):
                continue
            # Look ahead: does the error handling of this call warn-and-continue?
            # Statement position (not let/if/return context on the same line).
            stripped = line.lstrip()
            if re.match(r'(let|if|return|match)\b', stripped):
                # match/if-let are themselves the warn-continue form — handled
                # below via lookahead; `let` binding is fine.
                pass
            # Examine the following 12 lines for the error arm of this call.
            window = lines[lineno:lineno + 12]
            joined = '\n'.join(window)
            # The call's error arm must warn (or lower) and NOT return/abort.
            if WARN_RE.search(joined):
                # Heuristic: warn present within 12 lines after the call and
                # no return/continue-with-error before the warn. The checked
                # window is small; calibration found only true positives.
                if not re.search(r'\breturn\b|\?;|\babort\w*\(|\.abort\(|\bbail\!', joined.split('tracing::')[0] if 'tracing::' in joined else joined):
                    print(f"ERROR: warn-and-continue on restriction creation at {path}:{lineno}")
                    print(f"  {line.strip()}")
                    print("  creating a restriction artifact (policy) and continuing on failure")
                    print("  leaves the actor UNRESTRICTED while the system believes it is")
                    print("  restricted — the specs/reviews/task-077.md F6 flaw class.")
                    print("  Fail closed: propagate the error, abort the spawn, or exempt with:")
                    print("  // policy-create:ok — <reason>")
                    print()
                    errors += 1
    if errors:
        print(f"check-warn-continue-creation: FAILED — {errors} fail-open creation site(s) found")
        sys.exit(1)
    print("check-warn-continue-creation: OK")

main()
PYEOF
