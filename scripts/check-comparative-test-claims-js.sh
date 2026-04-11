#!/usr/bin/env bash
# Architecture lint: detect JavaScript/TypeScript test functions whose names
# make comparative claims but whose bodies lack meaningful comparisons.
#
# A test named "at low zoom, fillText calls are reduced" claims a comparative
# relationship: some metric is LOWER in one condition than another. To verify
# this claim, the test body must:
#   1. Take at least one measurement
#   2. Assert that measurement against a non-trivial baseline (not literal 0)
#
# A test whose only assertion is `.toBeGreaterThan(0)` proves existence, not
# reduction. A test with `.toBeLessThan(50000)` proves a ceiling, which is
# at least a non-trivial bound. But `.toBeGreaterThan(0)` is always true for
# any positive count — it provides zero evidence for a comparative claim.
#
# Detection signals:
#   1. Test name (the string argument to `it()` or `test()`) contains a
#      comparative/contrastive word: "reduced", "fewer", "less", "more",
#      "increases", "decreases", "individually", "not bundled", "not grouped"
#   2. Every `toBeLessThan()` / `toBeGreaterThan()` call in the test body
#      uses literal 0 as its argument — or no such calls exist at all
#
# Exemptions:
#   - Tests with `// comparative-claim:ok — <reason>` comment in the body
#   - Files listed in scripts/comparative-test-claim-js-exemptions.txt
#
# See: specs/reviews/task-053.md R2 F6, F8
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_DIR="web/src"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/comparative-test-claim-js-exemptions.txt"

if [ ! -d "$WEB_DIR" ]; then
    echo "Skipping comparative test claim check: $WEB_DIR not found"
    exit 0
fi

# Load exemption list
EXEMPTED_FILES=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_FILES=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

echo "Checking for comparative test claims without meaningful comparisons in JS/TS test files..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Comparative words regex for test names (case-insensitive matching done in awk)
# These words imply a comparison between two states/measurements.
COMPARATIVE_WORDS="reduced|fewer|less than|more than|increases|decreases|increased|decreased|individually|not bundled|not grouped"

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

    awk -v file="$file" -v comp_words="$COMPARATIVE_WORDS" '
    # Track it() / test() blocks
    /^\s*(it|test)\s*\(/ {
        # Extract test name (handle both single and double quotes)
        test_name = ""
        if (match($0, /(it|test)\s*\(\s*'"'"'([^'"'"']*)'"'"'/, m)) {
            test_name = m[2]
        } else if (match($0, /(it|test)\s*\(\s*"([^"]*)"/, m)) {
            test_name = m[2]
        } else if (match($0, /(it|test)\s*\(\s*`([^`]*)`/, m)) {
            test_name = m[2]
        }
        if (test_name == "") next

        # Check if test name contains a comparative word (case-insensitive)
        lower_name = tolower(test_name)
        has_comparative = 0
        n_words = split(comp_words, words, "|")
        for (i = 1; i <= n_words; i++) {
            if (index(lower_name, words[i]) > 0) {
                has_comparative = 1
                matched_word = words[i]
                break
            }
        }
        if (!has_comparative) next

        in_test = 1
        has_meaningful_comparison = 0
        has_exempt = 0
        test_line = NR
        saved_test_name = test_name
        saved_matched_word = matched_word
        brace_depth = 0
        # Count braces on this line
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }
        next
    }

    in_test {
        if ($0 ~ /comparative-claim:ok/) has_exempt = 1

        # Check for toBeLessThan(X) or toBeGreaterThan(X) where X is not literal 0
        # Also check toBeGreaterThanOrEqual and toBeLessThanOrEqual
        if ($0 ~ /\.toBeLessThan\s*\(/ || $0 ~ /\.toBeGreaterThan\s*\(/ || \
            $0 ~ /\.toBeLessThanOrEqual\s*\(/ || $0 ~ /\.toBeGreaterThanOrEqual\s*\(/) {
            # Extract the argument to the comparison matcher
            # Match: .toBeLessThan( ARG ) or .toBeGreaterThan( ARG )
            if (match($0, /\.(toBeLessThan|toBeGreaterThan|toBeLessThanOrEqual|toBeGreaterThanOrEqual)\s*\(\s*([^)]+)\s*\)/, m)) {
                arg = m[2]
                # Trim whitespace
                gsub(/^\s+|\s+$/, "", arg)
                # If argument is NOT literal 0, this is a meaningful comparison
                if (arg != "0" && arg != "0.0") {
                    has_meaningful_comparison = 1
                }
            }
        }

        # Track brace depth
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }

        # End of test block
        if ($0 ~ /^\s*\}\s*\)\s*;?\s*$/ || brace_depth <= 0) {
            if (!has_meaningful_comparison && !has_exempt) {
                printf "%s:%d: test \"%s\" — name contains comparative claim \"%s\" but body has no meaningful comparison (only > 0 or no comparison at all)\n", file, test_line, saved_test_name, saved_matched_word
            }
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "COMPARATIVE TEST CLAIMS WITHOUT COMPARISON (JS/TS):"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A test whose name contains a comparative word (\"reduced\", \"fewer\","
    echo "  \"individually\", \"not bundled\") claims a comparative relationship:"
    echo "  metric X is lower/higher/different in condition A than condition B."
    echo ""
    echo "  To verify this claim, the test must compare two measurements:"
    echo "    const baseline = mockCtx.op.mock.calls.length;  // condition A"
    echo "    // ... change condition ..."
    echo "    const actual = mockCtx.op.mock.calls.length;    // condition B"
    echo "    expect(actual).toBeLessThan(baseline);          // comparison"
    echo ""
    echo "  An assertion like expect(count).toBeGreaterThan(0) proves existence,"
    echo "  not reduction. It always passes for any positive count."
    echo ""
    echo "  Fix: Either add a second measurement and compare, or rename the"
    echo "  test to match what it actually verifies (e.g., \"produces draw calls\""
    echo "  instead of \"reduced draw calls\")."
    echo ""
    echo "  Exempt with: // comparative-claim:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-053.md R2 F6, F8"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "JS/TS comparative test claim check passed."
exit 0
