#!/usr/bin/env bash
# Architecture lint: detect tests that mirror production logic.
#
# A mirrored-logic test duplicates a production function's algorithm
# (match expressions, format strings) inside the test body and asserts
# on the reimplementation — without ever calling the production function.
# The test passes regardless of what the production code does.
#
# Detection signal:
#   Test function body contains a `match` expression where at least one
#   arm produces a string literal value (e.g., `"code" => "blue"`).
#   Value-producing match expressions in test code are almost always
#   logic duplication — tests should use known constants for expected
#   values or call the production function for actual values, not
#   recompute values via the same branching logic.
#
# Exemptions:
#   - Tests with "// mirrored-logic:ok" comment in the body
#   - Tests listed in scripts/mirrored-logic-test-exemptions.txt
#   - Match arms on Result/Option types (Ok/Err/Some/None) are excluded
#
# See: specs/reviews/task-024.md F1, F2
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/mirrored-logic-test-exemptions.txt"

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping mirrored-logic test check: $CRATE_SRC not found"
    exit 0
fi

# Load exemption list (test function names, one per line, # comments allowed)
EXEMPTED_TESTS=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_TESTS=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' | tr '\n' '|' || true)
    EXEMPTED_TESTS="${EXEMPTED_TESTS%|}"  # strip trailing |
fi

echo "Checking for mirrored-logic test functions..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" -v exempted="$EXEMPTED_TESTS" '
    # Detect test attribute — match #[test], #[tokio::test], #[tokio::test(...)]
    /^\s*#\[tokio::test|^\s*#\[test\]/ {
        in_test_attr = 1
        next
    }
    # Skip other attributes between test attr and fn
    in_test_attr && /^\s*#\[/ { next }
    # Skip blank lines between attributes
    in_test_attr && /^\s*$/ { next }
    # Match fn declaration after test attribute
    in_test_attr && /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        in_test_attr = 0

        match($0, /fn ([a-zA-Z_0-9]+)/, m)
        test_name = m[1]

        # Check exemption list
        if (exempted != "" && test_name ~ "^(" exempted ")$") { next }

        in_test = 1
        has_value_match = 0
        has_exempt_comment = 0
        brace_depth = 0
        test_start = NR
        next
    }
    # If we see a non-attribute, non-fn, non-blank line after test attr, reset
    in_test_attr { in_test_attr = 0 }

    # Inside test body — scan for value-producing match arms
    in_test {
        if ($0 ~ /mirrored-logic:ok/) has_exempt_comment = 1

        # Detect match arms that produce string literal values.
        # Pattern: `=> "string"` where the string is a bare value
        # (not inside Ok(), Err(), Some(), panic!(), assert!, format!,
        #  println!, eprintln!, or .to_string() chains starting from a string)
        #
        # We look for lines matching:  ... => "...",  or  ... => "...",
        # but NOT lines that are Result/Option destructuring or error handling.
        #
        # Positive examples (flagged):
        #   "code" => "blue",
        #   _ => "gray",
        #   "schema" => "purple",
        #
        # Negative examples (NOT flagged):
        #   Ok(value) => "...",    — Result destructuring
        #   Err(e) => "...",       — Error handling
        #   Some(x) => "...",      — Option destructuring
        #   None => panic!("..."), — Option handling
        #   ... => format!("..."), — format macro (produces String, not &str)

        if ($0 ~ /=>\s*"[^"]*"\s*[,}]?\s*$/) {
            # Exclude Result/Option/control-flow patterns
            if ($0 !~ /Ok\s*\(/ && $0 !~ /Err\s*\(/ && $0 !~ /Some\s*\(/ && $0 !~ /None\s*=>/) {
                has_value_match = 1
            }
        }

        # Detect end of test function (closing brace at 4-space indent)
        if ($0 ~ /^    \}$/) {
            if (has_value_match && !has_exempt_comment) {
                printf "%s:%d: test \"%s\" contains match arms producing string literals — likely mirrors production logic\n", file, test_start, test_name
            }
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "MIRRORED-LOGIC TESTS — tests that duplicate production code's branching logic:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A mirrored-logic test contains match expressions that map values"
    echo "  to string literals (e.g., \"code\" => \"blue\") — duplicating the"
    echo "  production function's algorithm. The test asserts on values"
    echo "  computed by the test's own logic, not by the production code."
    echo "  If the production function changes, this test still passes."
    echo ""
    echo "  Fix: Call the production function and assert on its output:"
    echo "    - Refactor the production function to return a String (or"
    echo "      accept a Write sink) so its output is capturable in tests"
    echo "    - Assert on the captured output's values"
    echo "    - Use known constants for expected values, not match expressions"
    echo ""
    echo "  If the match is intentional (e.g., parameterized test helper),"
    echo "  add comment: // mirrored-logic:ok — <reason>"
    echo "  Or add the test name to scripts/mirrored-logic-test-exemptions.txt"
    echo ""
    echo "  See: specs/reviews/task-024.md F1, F2"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Mirrored-logic test check passed."
exit 0
