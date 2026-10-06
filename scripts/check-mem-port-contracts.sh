#!/usr/bin/env bash
# check-mem-port-contracts.sh — a mem adapter's `create` must enforce a
# port-documented duplicate constraint when the port doc states one.
#
# Procedural background (specs/reviews/task-097.md F1):
# gyre-ports SecretRepository::create documents "Fails if a secret with
# the same `id`, or the same `(scope, scope_id, name)` within the
# tenant, already exists." SQLite enforces this via the migration's
# UNIQUE constraint; MemSecretRepository::create (mem.rs) pushed
# unconditionally, so duplicates could be inserted while get_value and
# rotate — both `.find(...)`-based — only ever saw the first entry.
# The divergence was invisible to all 17 adapter tests because they ran
# against SQLite only: a per-adapter test suite cannot catch a
# contract that only one adapter enforces. Any later consumer running
# in mem mode (store! wiring with no DB URL) inherits the divergence
# silently.
#
# Detection: for each port trait whose `create` doc comment contains a
# "Fails if ... already exists" (or equivalent duplicate-rejection)
# clause, the corresponding mem adapter's `create` body MUST contain a
# duplicate guard (an existence check: .any(, .find(, .position(,
# .contains(, .iter().any, exists(, bail! on duplicate) before the
# insert (.push( / .insert(). A create body that only inserts is a
# contract divergence.
#
# If a mem create is INTENTIONALLY unguarded because its port doc has
# no failure clause, it is not flagged (only doc'd constraints are
# enforced). If a flagged site must be exempted:
#   // mem-port-contract:ok — <reason>
# on the create body's first line.
#
# Exemptions are legacy debt in
# scripts/mem-port-contracts-exemptions.txt (path:line form,
# frozen count). Fix by adding the guard; never add entries.
FROZEN_EXEMPTION_COUNT=0
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/mem-port-contracts-exemptions.txt"
MEM_FILE="crates/gyre-server/src/mem.rs"
FAIL=0

declare -A EXEMPT
EXEMPT_TOTAL=0
if [ -f "$EXEMPTIONS_FILE" ]; then
    while IFS=: read -r file line rest; do
        case "$file" in ''|'#'*) continue ;; esac
        [ -n "$line" ] || continue
        EXEMPT["${file}:${line}"]=1
        EXEMPT_TOTAL=$((EXEMPT_TOTAL + 1))
    done < "$EXEMPTIONS_FILE"
fi

if [ "$EXEMPT_TOTAL" -gt "$FROZEN_EXEMPTION_COUNT" ]; then
    echo "FAIL: mem-port-contracts-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-097 F1), not an approval"
    echo "mechanism. Fix a flagged mem create by adding the duplicate guard"
    echo "the port doc states; never by exempting it. If you fixed a site,"
    echo "delete the line and lower FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

if [ ! -f "$MEM_FILE" ]; then
    echo "ERROR: Cannot find $MEM_FILE"
    exit 1
fi

echo "Checking mem adapter create() bodies for port-documented duplicate guards..."

# Single python pass over mem.rs: extract each `impl gyre_ports::<Trait>
# for <Type>` block, find `async fn create` bodies, and check for a
# duplicate guard. The port's create doc is read from gyre-ports to
# decide whether a guard is REQUIRED (only doc'd constraints count).
python3 - "$MEM_FILE" <<'PYEOF' > /tmp/.mem-port-contracts.$$
import re, sys, pathlib

mem_path = sys.argv[1]
src = pathlib.Path(mem_path).read_text()

# Collect port traits whose `create` method doc documents duplicate rejection.
port_dir = pathlib.Path("crates/gyre-ports/src")
doc_fail_traits = set()
if port_dir.is_dir():
    for pf in sorted(port_dir.glob("*.rs")):
        text = pf.read_text()
        for m in re.finditer(r'async\s+fn\s+create\s*\(&self[^)]*\)[^{;]*[;{]', text):
            # doc comment sits above the fn; grab preceding /// lines
            head = text[:m.start()].rstrip().splitlines()
            doc_lines = []
            for line in reversed(head):
                if line.strip().startswith('///'):
                    doc_lines.append(line)
                else:
                    break
            doc = "\n".join(reversed(doc_lines)).lower()
            if 'fails if' in doc and ('already exists' in doc or 'duplicate' in doc):
                # trait name: nearest `trait <Name>` above
                tm = None
                for t in re.finditer(r'(?:pub\s+)?trait\s+([A-Za-z0-9_]+)', text[:m.start()]):
                    tm = t
                if tm:
                    doc_fail_traits.add(tm.group(1))

def body_of(text, fn_start):
    """Return (brace-balanced body, line_of_fn_start) for the fn starting at fn_start."""
    i = text.find('{', fn_start)
    if i < 0:
        return None, None
    depth, j = 0, i
    while j < len(text):
        if text[j] == '{':
            depth += 1
        elif text[j] == '}':
            depth -= 1
            if depth == 0:
                break
        j += 1
    return text[i:j], 1 + text[:fn_start].count('\n')

GUARD_PAT = re.compile(r'\.any\(|\.find\(|\.position\(|\.contains\(|\.iter\(\)\s*\.any|exists\(|already exists|bail!')
INSERT_PAT = re.compile(r'\.push\(|\.insert\(|\.extend\(|store\.lock')

results = []
for m in re.finditer(r'impl\s+gyre_ports::([A-Za-z0-9_]+)\s+for\s+([A-Za-z0-9_]+)', src):
    trait, typ = m.group(1), m.group(2)
    # impl block body (offset = chars of src before blk, for file-absolute lines)
    blk, _ = body_of(src, m.start())
    if blk is None:
        continue
    blk_offset = src.index(blk)
    for cm in re.finditer(r'async\s+fn\s+create\s*\(&self[^)]*\)', blk):
        body, _ = body_of(blk, cm.start())
        if body is None:
            continue
        # file-absolute line of the fn signature
        line = 1 + src[:blk_offset + cm.start()].count('\n')
        inserts = bool(INSERT_PAT.search(body))
        guard = bool(GUARD_PAT.search(body))
        exempt = 'mem-port-contract:ok' in body
        results.append((trait, typ, line, guard, inserts, exempt, trait in doc_fail_traits))

for trait, typ, line, guard, inserts, exempt, doc_requires in results:
    if doc_requires and inserts and not guard and not exempt:
        print(f"VIOLATION {trait} {typ} create at line {line}")
PYEOF

VIOLATIONS=0
while IFS= read -r line; do
    case "$line" in
        VIOLATION*)
            # line format: VIOLATION <Trait> <Type> create at line <N>
            lineno="$(echo "$line" | awk '{print $NF}')"
            typ="$(echo "$line" | awk '{print $3}')"
            loc="${MEM_FILE}:${lineno}"
            if [ -z "${EXEMPT["$loc"]:-}" ]; then
                echo "MEM PORT CONTRACT DIVERGENCE: $typ create() has no duplicate guard"
                echo "  at $loc"
                echo "  The $typ port trait's create doc documents a duplicate-rejection"
                echo "  constraint ('Fails if ... already exists'); the mem implementation"
                echo "  inserts without checking, so duplicates are accepted while lookups"
                echo "  (.find-based) only ever see the first entry. SQLite enforces via"
                echo "  its UNIQUE constraint — tests running against SQLite alone cannot"
                echo "  catch this divergence (task-097 F1)."
                echo "  Fix: guard the insert with an existence check and bail!/Err on"
                echo "  duplicate, mirroring the SQLite adapter's constraint."
                echo ""
                VIOLATIONS=$((VIOLATIONS + 1))
                FAIL=1
            fi
            ;;
    esac
done < /tmp/.mem-port-contracts.$$

rm -f /tmp/.mem-port-contracts.$$
if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: $VIOLATIONS mem adapter create() method(s) violate port-documented uniqueness contracts."
    exit 1
fi

echo "OK: all mem adapter create() methods enforce their port-documented duplicate constraints."
