#!/usr/bin/env bash
# check-unwired-port-methods.sh — adapter-implemented port traits must
# have production wiring or a production caller.
#
# Procedural background (specs/reviews/task-207.md F1):
# `ActivityRepository::delete_older_than` was added to the port and
# implemented in SQLite, Postgres, and mem with passing adapter tests —
# but the trait was never wired into `AppState`, so no production code
# could call it: `run_cleanup`'s `activity_events` arm computed the
# cutoff, logged, and did nothing. The spec's 90-day activity retention
# guarantee was absent while every adapter test stayed green. The gap is
# invisible to tests because adapters certify the capability and nothing
# certifies that any production surface reaches it.
#
# Detection: a `pub trait` defined in gyre-ports and implemented in
# gyre-adapters (`impl Trait for Type` / `impl Type for Trait` /
# inherent `impl Trait { ... }`) with zero references in non-test
# production code of gyre-server, gyre-cli, or gyre-domain. Production
# code means: before the first `#[cfg(test)]` marker in each file,
# excluding `//` and `//!` comment lines. gyre-server integration tests
# (tests/) also count as consumers.
#
# Remediation: wire the trait into `build_state` (the `store!` pattern)
# or call it from a handler/job, so a production surface actually
# consumes it. A port that is intentionally adapter-only (no consumer
# yet) should not exist — cut it from the port until it has a caller.
# Legacy sites are frozen in scripts/unwired-port-methods-exemptions.txt
# (one trait name per line, with a reason). Never add entries.
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/unwired-port-methods-exemptions.txt"

if [ ! -f "$EXEMPTIONS_FILE" ]; then
    echo "check-unwired-port-methods: missing $EXEMPTIONS_FILE" >&2
    exit 1
fi

/usr/bin/python3 - "$EXEMPTIONS_FILE" <<'PYEOF'
import re, glob, sys

EXEMPT_FILE = sys.argv[1]

ADAPTER_FILES = sorted(glob.glob('crates/gyre-adapters/src/**/*.rs', recursive=True))
PORT_FILES = sorted(glob.glob('crates/gyre-ports/src/*.rs'))

traits = set()
for f in PORT_FILES:
    for m in re.finditer(r'pub trait (\w+)', open(f).read()):
        traits.add(m.group(1))

implemented = []
for t in sorted(traits):
    # `impl <Trait> for <Type>`, generic/prefixed forms
    # (`impl<T: X> Foo<T> for Bar<T>`), and inherent blocks in adapters
    # (a struct named after the port, e.g. `impl LlmPortFactory {`).
    # The optional prefix is a single identifier-like token followed by
    # whitespace so it cannot bleed into the trait name itself.
    pat_for = re.compile(r'impl\s+(?:<[^>{}]*>\s+)?(?:[A-Za-z_][A-Za-z0-9_:<>]*\s+)?' + t + r'\s+for\s')
    pat_inh = re.compile(r'impl\s+' + t + r'\s*\{')
    for f in ADAPTER_FILES:
        src = open(f).read()
        # Strip comments so doc mentions of the trait do not count as impls.
        code = re.sub(r'//[^\n]*', '', src)
        code = re.sub(r'/\*.*?\*/', '', code, flags=re.S)
        if pat_for.search(code) or pat_inh.search(code):
            implemented.append(t)
            break

def prod_ref_lines(name):
    """Non-comment references in production (pre-#[cfg(test)]) code of server/cli/domain."""
    hits = []
    for f in (glob.glob('crates/gyre-server/src/**/*.rs', recursive=True)
              + glob.glob('crates/gyre-server/tests/**/*.rs', recursive=True)
              + glob.glob('crates/gyre-cli/src/**/*.rs', recursive=True)
              + glob.glob('crates/gyre-domain/src/**/*.rs', recursive=True)):
        lines = open(f).read().splitlines()
        try:
            cut = next(i for i, l in enumerate(lines) if '#[cfg(test)]' in l)
        except StopIteration:
            cut = len(lines)
        word = re.compile(r'\b' + name + r'\b')
        for i, l in enumerate(lines[:cut]):
            ls = l.strip()
            if ls.startswith('//') or ls.startswith('//!'):
                continue
            if word.search(l):
                hits.append(f'{f}:{i+1}')
    return hits

exempt = set()
for line in open(EXEMPT_FILE):
    line = line.strip()
    if not line or line.startswith('#'):
        continue
    exempt.add(line)

violations = []
for t in implemented:
    if t in exempt:
        continue
    if not prod_ref_lines(t):
        violations.append(t)

if violations:
    print('UNWIRED PORT METHODS: adapter-implemented port traits with zero production references:')
    for t in violations:
        print(f'  - {t}: implemented in gyre-adapters but never referenced by non-test')
        print(f'      gyre-server/gyre-cli/gyre-domain code — no AppState wiring, no caller.')
        print(f'      Wire it into build_state (store! pattern) or call it from production code,')
        print(f'      or add it to scripts/unwired-port-methods-exemptions.txt with a reason.')
    sys.exit(1)
print('OK: every adapter-implemented port trait has production wiring or a caller (or is exempted).')
PYEOF
