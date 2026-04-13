#!/usr/bin/env bash
# Architecture lint: detect Rust test functions with zero assertion macros.
#
# A test function with no assert!, assert_eq!, assert_ne!, or assert_matches!
# macros is tautological — it always passes regardless of code behavior.
# The only thing it proves is "no panic," which is meaningless for functions
# designed to never panic (graceful degradation, error returns, Result types).
#
# This is a broader check than check-test-coverage-depth.sh (which only targets
# evaluate_push_constraints / evaluate_merge_constraints). This script catches
# assertionless tests for ANY function.
#
# Exemptions:
#   - #[should_panic] tests (assertion is the panic itself)
#   - Tests with "// assertionless:ok" comment in the function body
#   - Tests listed in scripts/assertionless-test-exemptions.txt (legacy baseline)
#
# See: specs/reviews/task-019.md F4
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/assertionless-test-exemptions.txt"

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping assertionless test check: $CRATE_SRC not found"
    exit 0
fi

# Load exemption list (test function names, one per line, # comments allowed)
EXEMPTED_TESTS=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_TESTS=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' | tr '\n' '|')
    EXEMPTED_TESTS="${EXEMPTED_TESTS%|}"  # strip trailing |
fi

echo "Checking for assertionless test functions..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" -v exempted="$EXEMPTED_TESTS" '
    # Detect test attribute — match #[test], #[tokio::test], #[tokio::test(...)]
    /^\s*#\[tokio::test|^\s*#\[test\]/ {
        in_test_attr = 1
        should_panic = 0
        next
    }
    # Check for should_panic between test attr and fn
    in_test_attr && /^\s*#\[should_panic/ { should_panic = 1; next }
    # Skip other attributes between test attr and fn
    in_test_attr && /^\s*#\[/ { next }
    # Skip blank lines between attributes
    in_test_attr && /^\s*$/ { next }
    # Match fn declaration after test attribute
    in_test_attr && /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        in_test_attr = 0
        if (should_panic) { should_panic = 0; next }

        match($0, /fn ([a-zA-Z_0-9]+)/, m)
        test_name = m[1]

        # Check exemption list
        if (exempted != "" && test_name ~ "^(" exempted ")$") { next }

        in_test = 1
        has_assert = 0
        has_exempt_comment = 0
        test_start = NR
        next
    }
    # If we see a non-attribute, non-fn, non-blank line after test attr, reset
    in_test_attr { in_test_attr = 0 }

    # Inside test body — scan for assertions and exemption comments
    in_test {
        if ($0 ~ /assert!|assert_eq!|assert_ne!|assert_matches!/) has_assert = 1
        if ($0 ~ /assertionless:ok/) has_exempt_comment = 1

        # Detect end of test function (closing brace at 4-space indent)
        if ($0 ~ /^    \}$/) {
            if (!has_assert && !has_exempt_comment) {
                printf "%s:%d: test \"%s\" has zero assertion macros\n", file, test_start, test_name
            }
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "ASSERTIONLESS TESTS — test functions with zero assertion macros:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A test with no assert!, assert_eq!, or assert_ne! macros is"
    echo "  tautological — it always passes regardless of correctness."
    echo "  The test proves only 'no panic,' which is meaningless for"
    echo "  functions that return Results instead of panicking."
    echo ""
    echo "  Fix: Add assertions on observable side effects:"
    echo "    - assert_eq!(entry.status, ...) — verify state after operation"
    echo "    - assert!(error_msg.contains(...)) — verify error content"
    echo "    - assert!(result.is_ok()) — explicit success assertion"
    echo "    - Retrieve domain objects and assert on their fields"
    echo ""
    echo "  If a test is intentionally assertionless (pure smoke test),"
    echo "  add comment: // assertionless:ok"
    echo "  Or add the test name to scripts/assertionless-test-exemptions.txt"
    echo ""
    echo "  See: specs/reviews/task-019.md F4"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Assertionless test check passed."
exit 0
