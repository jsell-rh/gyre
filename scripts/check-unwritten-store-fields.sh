#!/usr/bin/env bash
# check-unwritten-store-fields.sh — Detect in-memory adapter fields that
# are READ by a getter/lookup method but never WRITTEN by any non-test
# code, producing a per-adapter silent no-op (always None/404/empty).
#
# Procedural background (specs/reviews/task-087.md F2): MemTraceRepository
# declared `payloads: Arc<Mutex<HashMap<(String, String), SpanPayload>>>`.
# `get_span_payload` read `self.payloads`, but `store()` only wrote
# `self.store` — nothing in the codebase ever inserted into `self.payloads`.
# In pure in-memory mode every span-payload lookup returned 404. The
# compiler cannot catch this: the field IS "used" (the getter reads it),
# and `#[derive(Default)]` initializes it to an empty map. The sibling
# SQLite adapter DID populate its payload blob during store() — the two
# adapters silently diverged because no contract test ran the same
# store/get round-trip against the mem adapter.
#
# The fix-class rule this script enforces:
#
#   For every in-memory adapter struct — a struct whose field type is a
#   collection (`HashMap`/`Vec`/`HashSet`/`BTreeMap`, directly or inside
#   `Arc<Mutex<...>>`) and whose name contains `Mem` or which implements
#   a `*Repository`/`*Store` port trait — every field READ as
#   `self.FIELD` must also be WRITTEN somewhere in non-test code:
#     - an assignment `self.FIELD = ...`, or
#     - a collection mutation `self.FIELD.lock()...insert/push/extend`,
#       `self.FIELD.insert/push/extend`, or
#     - initialization in an explicit constructor (`Self { FIELD, ... }`
#       or `Self { FIELD: ..., ... }` — a derived Default alone does NOT
#       count).
#
# The `Mem` name filter plus the collection-type filter keeps the check
# scoped to the F2 flaw class (in-memory adapter divergence) and off the
# many legitimate read-only accessor patterns elsewhere.
#
# NOT flagged: code inside #[cfg(test)]/mod tests regions; lines carrying
# `// unwritten-field:ok`.
#
# Exempt a line with: `// unwritten-field:ok — <reason>`
#
# Usage: bash scripts/check-unwritten-store-fields.sh [paths...]
#        (default: crates/)
#
# Run by pre-commit and CI.

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

TEST_START = re.compile(r'^\s*(mod\s+tests\b|#\[cfg\(test\)\]\s*(mod|fn)|mod\s+\w+\s*\{\s*//\s*test)')
STRUCT_RE = re.compile(r'^\s*(pub\s+)?struct\s+(\w+)\s*\{')
FIELD_RE = re.compile(r'^\s*(pub\s+)?(pub\(crate\)\s+)?(\w+)\s*:')
COLLECTION_TYPE = re.compile(r'HashMap|HashSet|Vec<|BTreeMap|VecDeque')

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
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'unwritten-store-fields-exemptions.txt'
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
        test_starts = [i for i, l in enumerate(lines) if TEST_START.match(l)]
        limit = test_starts[0] if test_starts else len(lines)
        code = lines[:limit]

        # Collect candidate adapter structs: collection-typed fields, and
        # either a Mem-ish name or a repository/store impl on the struct.
        structs = {}  # struct name -> set of collection-typed field names
        i = 0
        while i < limit:
            m = STRUCT_RE.match(code[i])
            if m:
                name = m.group(2)
                fields = {}
                j = i + 1
                while j < limit and '}' not in code[j]:
                    fm = FIELD_RE.match(code[j])
                    if fm:
                        fields[fm.group(3)] = code[j]
                    j += 1
                if fields:
                    structs[name] = fields
                i = j
            i += 1

        adapter_fields = {}  # (struct_name, field) -> declaration line
        for name, fields in structs.items():
            is_adapter = 'Mem' in name or re.search(
                r'impl\s+\w*(Repository|Store)\s+for\s+' + re.escape(name), text
            ) is not None
            if not is_adapter:
                continue
            for fname, decl in fields.items():
                if COLLECTION_TYPE.search(decl):
                    adapter_fields[(name, fname)] = decl

        if not adapter_fields:
            continue

        # A write for field F, anywhere in non-test code:
        #   self.F = ...                          (assignment)
        #   self.F.insert/push/extend/remove/retain(...)
        #   self.F.lock() ... .insert/.push/...   (guard + mutation, incl.
        #   let mut g = self.F.lock(); g.insert()  multi-line chains)
        #   self.F.write().unwrap().push(...)      (RwLock)
        #   Self { F, ... } / Type { F: ..., ... } (constructor init)
        # Statement-level scan: collapse each statement (up to ';') onto one
        # line so multi-line lock chains parse as single units.
        stmts = re.split(r';', '\n'.join(code))
        collapsed = [re.sub(r'\s+', ' ', s) for s in stmts]

        def has_write(fname):
            esc = re.escape(fname)
            assign = re.compile(r'self\.' + esc + r'\s*=(?!=)')
            mutate = re.compile(
                r'self\.' + esc
                + r'\s*(\.\s*(lock|write)\s*\(\s*\)[^;]*(\.\s*(insert|push|extend|retain)|\bmut\b[^;]*;)|'
                + r'\.\s*(insert|push|extend|retain)\s*\()'
            )
            guard_mut = re.compile(r'let\s+mut\s+\w+\s*=\s*self\.' + esc + r'\s*\.\s*(lock|write)\s*\(')
            ctor = re.compile(r'\bSelf\s*\{[^}]*\b' + esc + r'\s*(:|,)')
            ctor_named = re.compile(
                r'(?<!struct\s)(?<!enum\s)\b[A-Z]\w*\s*\{[^}]*\b' + esc + r'\s*:\s*[^=]'
            )
            for s in collapsed:
                if (assign.search(s) or mutate.search(s) or guard_mut.search(s)
                        or ctor.search(s) or ctor_named.search(s)):
                    return True
            return False

        # Reads: `self.FIELD` in any non-test line (a read position is any
        # occurrence that is not itself a write).
        for (sname, fname), decl in sorted(adapter_fields.items()):
            if has_write(fname):
                continue
            read_re = re.compile(r'self\.' + re.escape(fname) + r'\b')
            for idx, line in enumerate(code):
                if '// unwritten-field:ok' in line:
                    continue
                if read_re.search(line):
                    key = f"{path}:{idx + 1}"
                    if key in exempt:
                        continue
                    print(f"ERROR: adapter field read but never written: `{sname}.{fname}` at {key}")
                    print(f"  {line.strip()}")
                    print(f"  Field declared as: {decl.strip()}")
                    print(f"  This collection field is read via `self.{fname}` but no non-test code")
                    print(f"  ever assigns it, inserts into it, or initializes it in a constructor")
                    print(f"  (a derived Default only makes it empty). Every read returns")
                    print(f"  None/empty — a per-adapter silent no-op. This is the")
                    print(f"  specs/reviews/task-087.md F2 flaw class (MemTraceRepository.payloads")
                    print(f"  never populated; in-memory get_span_payload always 404ed).")
                    print(f"  Populate the field in the store/save path (mirror the sibling")
                    print(f"  adapter), or exempt with: // unwritten-field:ok — <reason>")
                    print()
                    errors += 1

    if errors:
        print(f"check-unwritten-store-fields: FAILED — {errors} unwritten adapter field read(s) found")
        sys.exit(1)
    print("check-unwritten-store-fields: OK")

main()
PYEOF
