#!/usr/bin/env bash
# check-scope-literal-defaults.sh — Flag struct-literal scope fields
# fabricating a "default" tenant/workspace identity at construction time.
#
# Procedural background (specs/reviews/task-099.md F1): the admin bootstrap
# created a user via `User::new` (which leaves `tenant_id: None`), and the
# API-key auth extractor later fabricates `tenant_id: "default"` for any
# authenticated user lacking one — so an entity the platform-model spec
# requires to belong to exactly one tenant ("Every user belongs to exactly
# one tenant") rides a fabricated scope identity that no operator assigned.
# The existing check-fabricated-scope-defaults.sh catches the LOOKUP
# fallback shape (`unwrap_or("default")` on a failed store fetch); this
# check catches the CONSTRUCTION shape: a struct literal or field write
# that hardcodes `"default"` into a tenant_id/workspace_id/ws_id field.
# The two shapes compound: an entity created without its mandatory scope
# silently inherits whatever "default" identity a downstream consumer
# fabricates.
#
# Flagged: struct-literal fields and field assignments of the form
# `tenant_id|workspace_id|ws_id: "default"` / `Id::new("default")` /
# `"default".to_string()` in non-test Rust.
#
# NOT flagged: lines inside #[cfg(test)]/mod tests regions; comment lines;
# lines carrying the inline exemption marker.
#
# Exempt a line with: `// scope-literal:ok — <reason>`
#
# The exemption file (scripts/scope-literal-defaults-exemptions.txt) is
# seeded with the pre-existing sites identified when this check was
# introduced (task-099 process revision) and frozen at that count. Fix a
# flagged site by resolving the real scope (or refusing to create the
# entity without it); delete the exemption line when fixed and lower
# FROZEN_EXEMPTION_COUNT — never add entries, never raise it.
FROZEN_EXEMPTION_COUNT=24
#
# Run by pre-commit and CI.

set -uo pipefail

# No-arg default: scan crates/ — mirrors every sibling check. Without this,
# the argv-iterating Python body scans zero files and every invocation path
# (pre-commit pass_filenames:false, CI bare invocation, dev-check.sh) runs
# the check vacuously, silently un-pinning exemption lines whenever edits
# shift line numbers.
if [ $# -eq 0 ]; then
    set -- crates/
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

GYRE_SCRIPT_DIR="$SCRIPT_DIR" python3 - "$@" <<'PYEOF'
import os
import re
import sys
from pathlib import Path

FROZEN_EXEMPTION_COUNT = 24

TEST_START = re.compile(r'\s*(#\[[^\]]*\]\s*)?(mod tests|#\[cfg\(test\)\])')

# Struct-literal field or field assignment fabricating a scope identity:
#   tenant_id: "default".to_string(),
#   workspace_id: Id::new("default"),
#   tenant_id: "default",
SCOPE = re.compile(
    r'\b(tenant_id|workspace_id|ws_id)\s*:\s*'
    r'(?:Id::new\(\s*"default"\s*\)|"default"\.to_string\(\)|"default")'
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
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'scope-literal-defaults-exemptions.txt'
    if exempt_path.exists():
        for raw in exempt_path.read_text().splitlines():
            raw = raw.split('#', 1)[0].strip()
            if raw:
                exempt.add(raw)
    if len(exempt) > FROZEN_EXEMPTION_COUNT:
        print(f"check-scope-literal-defaults: FAILED — exemption file grew to "
              f"{len(exempt)} entries (baseline: {FROZEN_EXEMPTION_COUNT})")
        print("  The exemption file is legacy debt (seeded at check creation,"
              " task-099 process revision), not an approval mechanism. Fix a")
        print("  flagged site by resolving the real scope, then delete the line"
              " and lower FROZEN_EXEMPTION_COUNT; never add entries, never raise it.")
        sys.exit(1)
    for path in find_files(sys.argv[1:] or ['crates/']):
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
            if key in exempt or '// scope-literal:ok' in line:
                continue
            if test_start is not None and lineno - 1 >= test_start:
                continue
            stripped = line.strip()
            # Skip pure comments (doc lines quoting the pattern, etc.).
            if stripped.startswith('//') or stripped.startswith('*') or stripped.startswith('/*'):
                continue
            if not SCOPE.search(line):
                continue
            print(f"ERROR: fabricated scope identity at construction at {path}:{lineno}")
            print(f"  {stripped}")
            print("  A struct-literal/field 'default' scope fabricates an identity no")
            print("  operator assigned. The platform model requires every user/repo/")
            print("  task to belong to exactly one tenant/workspace; an entity created")
            print("  without its mandatory scope silently rides a fabricated 'default'")
            print("  identity downstream (cross-tenant data exposure if a real tenant")
            print("  is literally named \"default\").")
            print("  This is the specs/reviews/task-099.md F1 flaw class (construction-time")
            print("  sibling of the task-097 F3 lookup-fallback class).")
            print("  Resolve the real scope or refuse to create the entity without it;")
            print("  or exempt with:")
            print("  // scope-literal:ok — <reason>")
            print()
            errors += 1
    if errors:
        print(f"check-scope-literal-defaults: FAILED — {errors} fabricated scope literal(s) found")
        sys.exit(1)
    print("check-scope-literal-defaults: OK")

main()
PYEOF
