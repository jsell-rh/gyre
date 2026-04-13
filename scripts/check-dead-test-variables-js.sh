#!/usr/bin/env bash
# Architecture lint: detect dead array/collection variables in JS/TS test files.
#
# When a test function declares an array variable (`const X = []` or
# `let X = []`), populates it via `.push()`, but never consumes the
# variable (never passes it to `render()`, `expect()`, or any other
# function call), the variable is dead code. It was populated but never
# used — indicating either:
#   (a) An abandoned comparison that was never completed (the developer
#       intended to render both arrays and compare, but only completed one)
#   (b) A copy-paste remnant from another test
#
# Detection:
#   Within each `it()` or `test()` block:
#   1. Find `const X = []` or `let X = []` declarations
#   2. Track if X appears in `.push()` calls (write operations)
#   3. At block end, check if X appears in `render(`, `expect(`,
#      or any function call context beyond `.push()`
#   4. If X only appears in declaration + `.push()` → dead variable
#
# Exempt with: // dead-test-var:ok — <reason>
#
# See: specs/reviews/task-053.md R2 F7
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping dead test variable check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for dead array variables in JS/TS test files..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

for file in $(find "$WEB_SRC" -type f \( -name '*.test.js' -o -name '*.test.ts' -o -name '*.test.jsx' -o -name '*.test.tsx' -o -name '*.spec.js' -o -name '*.spec.ts' \) \
    ! -path '*/node_modules/*' \
    | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    BEGIN {
        in_test = 0
    }

    # Detect test block start: it( or test(
    !in_test && /^\s*(it|test)\s*\(/ {
        in_test = 1
        brace_depth = 0
        num_vars = 0
        has_exempt = 0
        test_start = NR
        delete var_names
        delete var_lines
        delete var_has_push
        delete var_has_consume

        # Count braces on this line
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "(") brace_depth++
            if (c == ")") brace_depth--
        }
        next
    }

    in_test {
        # Track brace depth (using parens for it()/test() nesting)
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "(") brace_depth++
            if (c == ")") brace_depth--
        }

        # Check for exemption
        if ($0 ~ /dead-test-var:ok/) {
            has_exempt = 1
        }

        # Detect array declarations: const X = [] or let X = []
        if (match($0, /(const|let)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*=\s*\[\s*\]/, m)) {
            num_vars++
            var_names[num_vars] = m[2]
            var_lines[num_vars] = NR
            var_has_push[num_vars] = 0
            var_has_consume[num_vars] = 0
        }

        # For each tracked variable, check usage
        for (i = 1; i <= num_vars; i++) {
            vn = var_names[i]
            if (NR == var_lines[i]) continue  # Skip declaration line

            # Check if this line references the variable
            if ($0 ~ ("\\<" vn "\\>")) {
                # Check if it is a .push() call (write operation)
                if ($0 ~ (vn "\\.push\\s*\\(")) {
                    var_has_push[i] = 1
                }
                # Check if it is consumed: render(), expect(), function arg,
                # assignment to another variable, return, property access
                # other than .push()
                if ($0 ~ ("render\\s*\\(.*" vn) || \
                    $0 ~ ("expect\\s*\\(.*" vn) || \
                    $0 ~ ("nodes:\\s*" vn) || \
                    $0 ~ ("edges:\\s*" vn) || \
                    $0 ~ ("props:.*" vn) || \
                    $0 ~ ("return\\s+" vn) || \
                    $0 ~ (vn "\\.length") || \
                    $0 ~ (vn "\\.map\\s*\\(") || \
                    $0 ~ (vn "\\.filter\\s*\\(") || \
                    $0 ~ (vn "\\.forEach\\s*\\(") || \
                    $0 ~ (vn "\\.reduce\\s*\\(") || \
                    $0 ~ (vn "\\.find\\s*\\(") || \
                    $0 ~ (vn "\\.some\\s*\\(") || \
                    $0 ~ (vn "\\.every\\s*\\(") || \
                    $0 ~ (vn "\\.join\\s*\\(") || \
                    $0 ~ (vn "\\.slice\\s*\\(") || \
                    $0 ~ (vn "\\[")) {
                    var_has_consume[i] = 1
                }
                # Also check: if the variable appears in a context that is
                # NOT a .push() call, it might be consumed
                if (!($0 ~ (vn "\\.push\\s*\\("))) {
                    # Variable referenced in a non-.push() context
                    var_has_consume[i] = 1
                }
            }
        }

        # End of test block
        if ($0 ~ /^\s*\}\s*\)\s*;?\s*$/ || brace_depth <= 0) {
            if (!has_exempt) {
                for (i = 1; i <= num_vars; i++) {
                    if (var_has_push[i] && !var_has_consume[i]) {
                        printf "%s:%d: array \"%s\" is populated via .push() but never consumed (not in render(), expect(), or any read operation)\n", file, var_lines[i], var_names[i]
                    }
                }
            }
            in_test = 0
            num_vars = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "DEAD ARRAY VARIABLES in test files:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  An array variable declared in a test block that is populated"
    echo "  via .push() but never consumed (not passed to render(), expect(),"
    echo "  or used in any read operation) is dead code."
    echo ""
    echo "  This typically indicates an abandoned comparison: the developer"
    echo "  intended to render/assert on two arrays but only completed one,"
    echo "  leaving the other as dead code that confuses readers."
    echo ""
    echo "  Fix: Either use the array (render it, assert on it, compare it)"
    echo "  or remove it entirely. If the test name claims a comparison"
    echo "  (e.g., \"concentrated vs spread-out\"), implement the comparison."
    echo ""
    echo "  Exempt with: // dead-test-var:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-053.md R2 F7"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Dead test variable check passed."
exit 0
