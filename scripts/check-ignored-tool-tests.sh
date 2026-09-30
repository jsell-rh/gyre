#!/usr/bin/env bash
# check-ignored-tool-tests.sh — Flag adapter functions that shell out to an
# external tool where the ONLY same-file tests exercising them are #[ignore]d.
#
# Procedural background (specs/reviews/task-106.md F1, F2): run_jj returned
# output.stdout only, but jj 0.39.0 writes rebase status (Rebased N commits,
# New conflicts appeared) to STDERR — stdout is empty. Conflict detection
# parsed stdout and was dead code against real jj. jj git init --colocate
# inside a git worktree is refused by jj ("Cannot create a colocated jj repo
# inside a Git worktree"), and the swallowed init failure left worktrees with
# no jj repo on disk. Every same-file test exercising the jj adapter fns was
# #[ignore]d, so CI never ran them against the real contract. The fix-class
# rule: external-tool contracts need (1) empirical verification against the
# real binary, and (2) recorded-fixture tests that run in CI.
#
# Flagged: in a file under gyre-adapters, a non-test function that shells out
# (Command::new directly, or via a same-file helper that does) where every
# same-file test calling that function is #[ignore]d — i.e., no runnable
# test pins the function's observable contract.
#
# NOT flagged: functions with zero same-file test callers are left to
# review (too noisy tree-wide); test-only files (all-test files) are
# naturally excluded because they define no non-test functions.
#
# Exempt a line with: `// tool-test:ok — <reason>`
#
# Usage: bash scripts/check-ignored-tool-tests.sh [paths...]  (default crates/gyre-adapters/src)

set -uo pipefail

if [ $# -eq 0 ]; then
    set -- crates/gyre-adapters/src
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

GYRE_SCRIPT_DIR="$SCRIPT_DIR" python3 - "$@" <<'PYEOF'
import os
import re
import sys
from pathlib import Path

TEST_START = re.compile(r'\s*(#\[[^\]]*\]\s*)?(mod tests|#\[cfg\(test\)\])')
FN_DEF = re.compile(r'\s*(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+(\w+)')

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
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'ignored-tool-tests-exemptions.txt'
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
        nontest = lines[:test_start] if test_start is not None else lines
        test = lines[test_start:] if test_start is not None else []
        if not nontest:
            continue

        # Non-test fn spans.
        starts = [(i, FN_DEF.match(l)) for i, l in enumerate(nontest) if FN_DEF.match(l)]
        if not starts:
            continue
        fns = []
        for idx, (i, m) in enumerate(starts):
            end = starts[idx + 1][0] if idx + 1 < len(starts) else len(nontest)
            fns.append((m.group(4), i + 1, '\n'.join(nontest[i:end])))

        # Command helpers and tool fns (direct Command::new or via a helper).
        cmd_fns = {n for n, _, body in fns if 'Command::new' in body}
        tool_fns = []
        for n, ln, body in fns:
            direct = 'Command::new' in body
            via_helper = any(re.search(r'\b' + re.escape(h) + r'\s*\(', body) for h in cmd_fns if h != n)
            if direct or via_helper:
                tool_fns.append((n, ln))

        if not tool_fns:
            continue

        # Test chunks: from each #[test]/#[tokio::test] to the next.
        tstarts = [i for i, l in enumerate(test) if re.search(r'#\[(tokio::)?test', l)]
        tstarts.append(len(test))
        tests = []
        for a, b in zip(tstarts, tstarts[1:]):
            chunk = '\n'.join(test[a:b])
            tests.append({'ignore': '#[ignore' in chunk,
                          'calls': set(re.findall(r'[.\s(](\w+)\(', chunk))})

        for fn_name, def_lineno in tool_fns:
            key = f"{path}:{def_lineno}"
            if key in exempt or '// tool-test:ok' in lines[def_lineno - 1]:
                continue
            callers = [t for t in tests if fn_name in t['calls']]
            # Only flag when ALL test callers are ignored AND there is at
            # least one — zero-caller functions are a review concern, not a
            # mechanically decidable one (too noisy tree-wide).
            if callers and all(t['ignore'] for t in callers):
                print(f"ERROR: {fn_name} at {path}:{def_lineno} — only #[ignore]d test coverage")
                print("  this function shells out to an external tool, and every same-file")
                print("  test that calls it is #[ignore]d — CI never pins its contract.")
                print("  This is the specs/reviews/task-106.md F1/F2 flaw class (jj writes")
                print("  status to stderr; colocated init refused in worktrees; the ignored")
                print("  tests never caught either). Add a recorded-fixture test that runs")
                print("  in CI, or exempt with:")
                print("  // tool-test:ok — <reason>")
                print()
                errors += 1
    if errors:
        print(f"check-ignored-tool-tests: FAILED — {errors} tool fn(s) with ignored-only coverage")
        sys.exit(1)
    print("check-ignored-tool-tests: OK")

main()
PYEOF
