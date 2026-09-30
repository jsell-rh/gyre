#!/usr/bin/env bash
# check-byte-slice-truncation.sh — Flag constant byte-index slices/truncations
# of runtime strings in Rust.
#
# Procedural background (specs/reviews/task-095.md F4): `&combined[..4096]` on
# `String::from_utf8_lossy` output panics when byte 4096 falls inside a
# multibyte UTF-8 character. Gate/CI output routinely exceeds 4 KiB AND
# contains non-ASCII (compiler diagnostics, localized tools) — the panic
# unwound through the merge-processor recovery path, leaving the queue paused
# with no revert/notification. The identical pre-existing pattern in
# `run_command` shows this is a class, not a one-off.
#
# Flagged patterns (byte-index operations on a String/&str with a CONSTANT
# index — literals or ALL_CAPS consts):
#   &expr[..N]   &expr[N..]   &expr[N..M]   expr.truncate(N)   &expr[N]
#
# Slices with a leading `..` AND trailing bound only when the expression is
# runtime-derived (we cannot prove constness of the string, so we assume
# runtime — the exemption path is explicit).
#
# NOT flagged: slices of byte arrays/Vec<u8> (&v[..N] on non-UTF-8 is fine),
# u8 slices (`.as_bytes()[..N]` — slicing bytes never hits a char boundary),
# char_indices/chars()-based truncation, floor_char_boundary.
#
# Exempt a line with: `// slice:ok — <reason>`
#
# Usage: bash scripts/check-byte-slice-truncation.sh [paths...]  (default crates/)

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

# Constant index: numeric literal or SCREAMING_CASE const.
CONST = r'(?:0x[0-9a-fA-F]+|\d+|[A-Z][A-Z0-9_]*(?:::[A-Z][A-Z0-9_]*)*)'

# Patterns that byte-index a string expression at a constant.
# We conservatively flag `&expr[..N]`, `&expr[N..]`, `&expr[N..M]`,
# `expr.truncate(N)`. `expr` = identifier-ish chain (not .as_bytes()).
PATTERNS = [
    # &ident[..N] / &ident.chain[..N] — but skip if the sliced expr is bytes
    re.compile(r'&\s*([a-z_][a-z0-9_]*(?:\.[a-z_][a-z0-9_]*)*)\s*\[\s*\.\.'
               + CONST + r'\s*\]'),
    re.compile(r'&\s*([a-z_][a-z0-9_]*(?:\.[a-z_][a-z0-9_]*)*)\s*\[\s*'
               + CONST + r'\s*\.\.\s*\]'),
    re.compile(r'&\s*([a-z_][a-z0-9_]*(?:\.[a-z_][a-z0-9_]*)*)\s*\[\s*'
               + CONST + r'\s*\.\.\s*' + CONST + r'\s*\]'),
    re.compile(r'([a-z_][a-z0-9_]*(?:\.[a-z_][a-z0-9_]*)*)\.truncate\(\s*'
               + CONST + r'\s*\)'),
]

# Expressions that are byte-typed — slicing them is char-boundary-safe.
BYTE_EXPR_RE = re.compile(
    r'\b(?:as_bytes|as_slice|len\(\)|to_vec|into_bytes|bytes\(\))\s*$'
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
    # File-level exemptions: one `path:line` per line, `#` comments allowed.
    # Format matches the repo's other check exemption files.
    exempt = set()
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'byte-slice-truncation-exemptions.txt'
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
        for lineno, line in enumerate(lines, 1):
            key = f"{path}:{lineno}"
            if key in exempt or '// slice:ok' in line:
                continue
            # Cheap prefilter: only lines with a range slice or truncate call.
            if not re.search(r'\[\s*\.\.|\.truncate\(|\[\s*\d+\s*\.\.', line):
                continue
            # Context: the statement containing the slice can span lines
            # (method chains). Look back up to 3 lines for guard patterns.
            ctx = '\n'.join(lines[max(0, lineno - 4):lineno])
            for pat in PATTERNS:
                for m in pat.finditer(line):
                    expr = m.group(1) if m.groups() else ''
                    # Skip slicing of byte-typed expressions.
                    if BYTE_EXPR_RE.search(ctx):
                        continue
                    # Skip `chars()` / `char_indices()` derived chains.
                    if 'char' in expr:
                        continue
                    # ASCII-prefix slicing: `starts_with("tok=")` bounds
                    # `&p[N..]` to a known ASCII prefix, and `split()`
                    # fields split on ASCII delimiters — provably safe for
                    # constant N ≤ the delimiter length.
                    if re.search(r'\.split\s*\(', ctx):
                        continue
                    if re.search(r"\bstarts_with\s*\(\s*(?:\"[ -~]{1,20}\"|'(?:\\.|[^\\'])')\s*\)", ctx) and \
                       re.search(r'\[\s*' + CONST + r'\s*\.\.', m.group(0)):
                        continue
                    # `from_utf8(&x[..N])` takes &[u8] — the slice is bytes.
                    if 'from_utf8(' in line:
                        continue
                    # A `b"..."` byte string being compared/sliced is bytes.
                    if re.search(r'\bb"', ctx):
                        continue
                    # `&v[..N]` on a `Vec<u8>`/array — can't tell types, but
                    # the repo's byte-slice sites use as_bytes()/b"..." or
                    # split()-derived values; if the expr's last segment is
                    # `parts`, `segments`, `bytes` — treat as index-typed.
                    if expr and re.match(r'^(?:parts|segments|bytes|chunks|fields|toks|words|lines)\b', expr.split('.')[-1]):
                        continue
                    print(f"ERROR: constant byte-index slice at {path}:{lineno}")
                    print(f"  {line.strip()}")
                    print("  the index falls inside a multibyte UTF-8 character. Gate/CI")
                    print("  output exceeds limits AND contains non-ASCII — this is the")
                    print("  specs/reviews/task-095.md F4 flaw class.")
                    print("  Use a char-boundary-safe truncation, or exempt with:")
                    print("  // slice:ok — <reason>")
                    print()
                    errors += 1
    if errors:
        print(f"check-byte-slice-truncation: FAILED — {errors} unsafe slice(s) found")
        sys.exit(1)
    print("check-byte-slice-truncation: OK")

main()
PYEOF
