#!/usr/bin/env bash
# Architecture lint: detect JavaScript/TypeScript test functions with only
# tautological assertions (assertions that always pass).
#
# A test function whose only assertion is `expect(container).toBeTruthy()`
# (or equivalent) after a `render()` call is tautological: `render()` always
# returns a container object, so `container` is always truthy. The test
# proves only "the component didn't throw during render" — it provides zero
# coverage of the component's visual or interactive behavior.
#
# Detection signals:
#   1. An `it(` or `test(` block contains `expect(container).toBeTruthy()`
#      (or .toBeDefined() or .not.toBeNull()) as the ONLY expect call.
#   2. An `it(` or `test(` block where every `expect()` call targets the
#      `container` variable with a tautological matcher.
#
# Exemptions:
#   - Tests with `// tautological:ok` comment in the body
#   - Files listed in scripts/tautological-assertion-js-exemptions.txt
#
# See: specs/reviews/task-053.md F3
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_DIR="web/src"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/tautological-assertion-js-exemptions.txt"

if [ ! -d "$WEB_DIR" ]; then
    echo "Skipping tautological assertion check: $WEB_DIR not found"
    exit 0
fi

# Load exemption list
EXEMPTED_FILES=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_FILES=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

echo "Checking for tautological assertions in JS/TS test files..."

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

    # Detect it() blocks where every expect() is tautological
    # Tautological patterns:
    #   expect(container).toBeTruthy()
    #   expect(container).toBeDefined()
    #   expect(container).not.toBeNull()
    #   expect(EXPR || EXPR).toBeTruthy()  where EXPR uses container.querySelector
    awk -v file="$file" '
    # Track it() / test() blocks
    /^\s*(it|test)\s*\(/ {
        # Extract test name
        match($0, /(it|test)\s*\(\s*['"'"'"](.*)['"'"'"]/, m)
        test_name = m[2]
        if (test_name == "") test_name = "(unnamed)"
        in_test = 1
        total_expects = 0
        tautological_expects = 0
        has_exempt = 0
        test_line = NR
        paren_depth = 0
        # Count parens on this line to track nesting
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "(") paren_depth++
            if (c == ")") paren_depth--
        }
        next
    }
    in_test {
        if ($0 ~ /tautological:ok/) has_exempt = 1

        # Count expect() calls
        if ($0 ~ /expect\s*\(/) {
            total_expects++
            # Check if this is a tautological assertion on container
            if ($0 ~ /expect\s*\(\s*container\s*\)\s*\.\s*toBeTruthy\s*\(/) {
                tautological_expects++
            } else if ($0 ~ /expect\s*\(\s*container\s*\)\s*\.\s*toBeDefined\s*\(/) {
                tautological_expects++
            } else if ($0 ~ /expect\s*\(\s*container\s*\)\s*\.\s*not\s*\.\s*toBeNull\s*\(/) {
                tautological_expects++
            }
            # Also catch: expect(container.querySelector(...) || container.querySelector(...)).toBeTruthy()
            # This pattern queries elements but asserts only existence of the container-or fallback
            else if ($0 ~ /expect\s*\(.*\|\|.*\)\s*\.\s*toBeTruthy\s*\(/) {
                tautological_expects++
            }
            # Also catch: expect(hasValidRoot).toBeTruthy() where hasValidRoot is derived from querySelector || querySelector
            # But this is harder to detect statically, so we catch the direct patterns above
        }

        # Track nesting depth
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "(") paren_depth++
            if (c == ")") paren_depth--
        }

        # Detect end of test block (closing paren+semicolon at low depth)
        # The it() block ends when we reach depth 0 or see `});`
        if ($0 ~ /^\s*\}\s*\)\s*;?\s*$/ || paren_depth <= 0) {
            if (total_expects > 0 && total_expects == tautological_expects && !has_exempt) {
                printf "%s:%d: test \"%s\" has %d assertion(s), all tautological (expect(container).toBeTruthy() or equivalent)\n", file, test_line, test_name, total_expects
            }
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "TAUTOLOGICAL ASSERTIONS (JS/TS) — test functions with only always-true assertions:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A test whose only assertion is expect(container).toBeTruthy()"
    echo "  after render() is tautological: render() always returns a"
    echo "  container, so .toBeTruthy() always passes. The test provides"
    echo "  zero coverage of the component's visual or interactive behavior."
    echo ""
    echo "  Fix: Replace tautological assertions with behavioral ones:"
    echo "    - Query specific DOM elements and assert on their content"
    echo "    - Fire events (click, input) and assert on DOM changes"
    echo "    - Verify CSS classes, attributes, or text content"
    echo "    - Assert that interactive elements produce visible effects"
    echo ""
    echo "  If the test is intentionally a smoke test (render-only),"
    echo "  add: // tautological:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-053.md F3"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "JS/TS tautological assertion check passed."
exit 0
