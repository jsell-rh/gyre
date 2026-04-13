#!/usr/bin/env bash
# Architecture lint: detect JavaScript/TypeScript test files that mirror
# production logic in locally-defined functions.
#
# A mirrored-logic test in JS defines a standalone function inside the test
# file that replicates conditional/comparison logic from a Svelte component
# or module, then asserts on the local function's return values instead of
# testing the component's actual rendering behavior. The test validates its
# own reimplementation — the component is never exercised.
#
# Detection signal:
#   A .test.js/.test.ts file defines `function name(params) { ... }` where:
#   1. The function body contains comparison operators (>=, <=, ===, !==,
#      >, <) or conditional keywords (if, ? :, includes, return ... &&)
#   2. The function name appears in an `expect(name(` call in the same file
#   3. The function is NOT a render/setup helper (doesn't call render())
#
# Exemptions:
#   - Functions with `// mirrored-logic:ok` comment in the body
#   - Files listed in scripts/mirrored-logic-test-js-exemptions.txt
#
# See: specs/reviews/task-053.md F1, F2
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_DIR="web/src"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/mirrored-logic-test-js-exemptions.txt"

if [ ! -d "$WEB_DIR" ]; then
    echo "Skipping JS mirrored-logic test check: $WEB_DIR not found"
    exit 0
fi

# Load exemption list (file paths, one per line, # comments allowed)
EXEMPTED_FILES=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_FILES=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

echo "Checking for mirrored-logic test functions in JS/TS test files..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

for file in $(find "$WEB_DIR" -name '*.test.js' -o -name '*.test.ts' 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Check exemption list
    if [ -n "$EXEMPTED_FILES" ]; then
        skip=false
        while IFS= read -r exempt; do
            [ -z "$exempt" ] && continue
            if [[ "$file" == *"$exempt"* ]]; then
                skip=true
                break
            fi
        done <<< "$EXEMPTED_FILES"
        $skip && continue
    fi

    # Phase 1: Collect function names defined in this file that contain logic
    # (comparison operators, conditionals) and are NOT render helpers
    logic_fns=$(awk '
    /^\s*function\s+[a-zA-Z_][a-zA-Z0-9_]*\s*\(/ {
        # Extract function name
        match($0, /function\s+([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        in_fn = 1
        has_logic = 0
        has_render = 0
        has_exempt = 0
        brace_depth = 0
        for (i = 1; i <= NF; i++) {
            if ($i ~ /\{/) brace_depth++
            if ($i ~ /\}/) brace_depth--
        }
        # Count braces on this line
        n = split($0, chars, "")
        brace_depth = 0
        for (i = 1; i <= n; i++) {
            if (substr($0, i, 1) == "{") brace_depth++
            if (substr($0, i, 1) == "}") brace_depth--
        }
        next
    }
    in_fn {
        if ($0 ~ /mirrored-logic:ok/) has_exempt = 1
        if ($0 ~ />=|<=|===|!==|[^=]>[^=]|[^=<]<[^=]/) has_logic = 1
        if ($0 ~ /\.includes\(/) has_logic = 1
        if ($0 ~ /\? .* :/) has_logic = 1
        if ($0 ~ /if\s*\(/) has_logic = 1
        if ($0 ~ /return.*&&/) has_logic = 1
        if ($0 ~ /render\(/) has_render = 1

        n = split($0, chars, "")
        for (i = 1; i <= n; i++) {
            if (substr($0, i, 1) == "{") brace_depth++
            if (substr($0, i, 1) == "}") brace_depth--
        }
        if (brace_depth <= 0) {
            if (has_logic && !has_render && !has_exempt) {
                print fn_name
            }
            in_fn = 0
        }
    }
    ' "$file" 2>/dev/null)

    [ -z "$logic_fns" ] && continue

    # Phase 2: Check if any of these functions are used in expect() calls
    while IFS= read -r fn_name; do
        [ -z "$fn_name" ] && continue
        # Check for expect(fn_name( pattern
        if grep -qE "expect\(\s*${fn_name}\s*\(" "$file" 2>/dev/null; then
            line=$(grep -nE "^\s*function\s+${fn_name}\s*\(" "$file" 2>/dev/null | head -1 | cut -d: -f1)
            echo "$file:${line:-0}: function \"$fn_name\" is defined locally with logic operators and tested via expect($fn_name(...)) — likely mirrors component logic" >> "$HITS_FILE"
        fi
    done <<< "$logic_fns"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "MIRRORED-LOGIC TESTS (JS/TS) — test files that define local logic functions and test them:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A mirrored-logic test defines a function in the test file that"
    echo "  replicates conditional/comparison logic from a Svelte component,"
    echo "  then asserts on the local function — never testing the component."
    echo "  If the component's logic changes, this test still passes."
    echo ""
    echo "  Fix: Test the component's actual behavior:"
    echo "    - Render the component with specific props (zoom, viewport, etc.)"
    echo "    - Assert on observable effects (draw calls, DOM elements, CSS)"
    echo "    - If testing an algorithm in isolation, extract the function"
    echo "      from the component and import it in both the component and test"
    echo ""
    echo "  If the local function is intentional (e.g., documenting expected"
    echo "  behavior), add: // mirrored-logic:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-053.md F1, F2"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "JS/TS mirrored-logic test check passed."
exit 0
