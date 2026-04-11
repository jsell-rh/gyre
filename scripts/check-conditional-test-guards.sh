#!/usr/bin/env bash
# Architecture lint: detect conditional guards around test assertions in
# frontend test files.
#
# When a test wraps assertions inside an `if (element)` guard:
#
#   if (ackBtn) { await fireEvent.click(ackBtn); expect(...) }
#
# the assertions are silently skipped if the element fails to render.
# The test passes with zero assertions for that code path, masking
# rendering bugs. The correct pattern is:
#
#   expect(ackBtn).toBeTruthy();   // assert element exists FIRST
#   await fireEvent.click(ackBtn);
#   expect(...);
#
# This script detects `if (var) { ... expect ... }` patterns in test files,
# both single-line and multi-line (within a 10-line window).
#
# Exempt with: // conditional-guard:ok — <reason>
#
# See: specs/reviews/task-052.md F5 (acknowledge test with conditional guard)
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/conditional-test-guard-exemptions.txt"

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping conditional test guard check: $WEB_SRC not found"
    exit 0
fi

# Load exemption list (file:line patterns, one per line, # comments allowed)
EXEMPTED_LINES=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_LINES=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

echo "Checking for conditional guards around test assertions..."

HITS_FILE=$(mktemp)
FILTERED_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE" "$FILTERED_FILE"' EXIT

# Find test files (.test.js, .test.ts, .spec.js, .spec.ts)
for file in $(find "$WEB_SRC" -type f \( -name '*.test.js' -o -name '*.test.ts' -o -name '*.spec.js' -o -name '*.spec.ts' \) \
    ! -path '*/node_modules/*' \
    | sort); do
    [ -f "$file" ] || continue

    # Use awk to detect conditional guards around assertions.
    #
    # Pattern 1 (single-line):
    #   if (someVar) { ... expect( ... }
    #
    # Pattern 2 (multi-line):
    #   if (someVar) {
    #     ... (within next 10 lines)
    #     expect(
    #
    # NOT flagged:
    #   - Lines with conditional-guard:ok exemption

    awk -v file="$file" '
    # Skip lines with exemption
    /conditional-guard:ok/ { next }

    # Pattern 1: single-line if-guarded assertion
    # Match: if (varName) { ... expect( ... }
    /^[[:space:]]*if[[:space:]]*\([a-zA-Z_$][a-zA-Z0-9_$.]*\)[[:space:]]*[{].*expect\(/ {
        printf "%s:%d: assertion inside conditional guard (silently skippable)\n  %s\n", file, NR, $0
        next
    }

    # Pattern 2: multi-line if-guard start
    # Match: if (varName) {   (with optional whitespace, end of line)
    /^[[:space:]]*if[[:space:]]*\([a-zA-Z_$][a-zA-Z0-9_$.]*\)[[:space:]]*[{]?[[:space:]]*$/ {
        in_if_guard = 1
        if_guard_line = NR
        if_guard_text = $0
        has_expect = 0
        next
    }

    in_if_guard {
        # Check for expect inside the guarded block
        if ($0 ~ /expect\(/) {
            has_expect = 1
        }

        # End of block (closing brace on its own line or at similar indent)
        if ($0 ~ /^[[:space:]]*[}]/) {
            if (has_expect) {
                printf "%s:%d: assertion inside conditional guard (silently skippable)\n  %s\n", file, if_guard_line, if_guard_text
            }
            in_if_guard = 0
            next
        }

        # Safety: stop tracking after 10 lines
        if (NR - if_guard_line > 10) {
            if (has_expect) {
                printf "%s:%d: assertion inside conditional guard (silently skippable)\n  %s\n", file, if_guard_line, if_guard_text
            }
            in_if_guard = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

# Filter out exempted lines
if [ -s "$HITS_FILE" ] && [ -n "$EXEMPTED_LINES" ]; then
    while IFS= read -r line; do
        is_exempt=0
        for exempt in $EXEMPTED_LINES; do
            if echo "$line" | grep -qF "$exempt"; then
                is_exempt=1
                break
            fi
        done
        [ "$is_exempt" -eq 0 ] && echo "$line" >> "$FILTERED_FILE"
    done < <(grep "^[^ ]" "$HITS_FILE")
    # Also copy indented context lines for non-exempted hits
    if [ -s "$FILTERED_FILE" ]; then
        cp "$FILTERED_FILE" "$HITS_FILE"
    else
        : > "$HITS_FILE"
    fi
fi

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "CONDITIONAL TEST GUARDS — assertions inside if-guards that can be silently skipped:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  An assertion inside an if (element) guard is silently skipped when"
    echo "  the element fails to render. The test passes with zero assertions"
    echo "  for that code path, masking rendering bugs."
    echo ""
    echo "  Fix: Assert the element exists FIRST, then interact with it:"
    echo "    const ackBtn = container.querySelector('.acknowledge-btn');"
    echo "    expect(ackBtn).toBeTruthy();   // fails loudly if missing"
    echo "    await fireEvent.click(ackBtn);"
    echo "    expect(mockFn).toHaveBeenCalled();"
    echo ""
    echo "  Do NOT wrap assertions in if (element) guards — this converts"
    echo "  a test failure into a silent pass."
    echo ""
    echo "  If genuinely intentional, add '// conditional-guard:ok — <reason>'"
    echo "  on the if line."
    echo ""
    echo "  See: specs/reviews/task-052.md F5"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Conditional test guard check passed."
exit 0
