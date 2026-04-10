#!/usr/bin/env bash
# Architecture lint: detect tests whose names claim behavior their bodies admit
# they don't exercise (aspirational test names / false coverage claims).
#
# Detection signals:
#   1. Test body contains self-admitting phrases like "this test verifies the
#      persistence path", "true X requires", "which is an integration test
#      concern", "cannot be tested here", "does not test what".
#   2. Test name contains a recovery/revert verb but final assertion asserts
#      a degraded state (Stale, Failed, Error, etc.) — proving persistence,
#      not recovery.
#
# Exemptions:
#   - Tests with "// aspirational-name:ok — <reason>" comment in the body
#   - Tests listed in scripts/aspirational-test-exemptions.txt
#
# See: specs/reviews/task-021.md R2 F1
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/aspirational-test-exemptions.txt"

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping aspirational test name check: $CRATE_SRC not found"
    exit 0
fi

# Load exemption list (test function names, one per line, # comments allowed)
EXEMPTED_TESTS=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_TESTS=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' | tr '\n' '|' || true)
    EXEMPTED_TESTS="${EXEMPTED_TESTS%|}"  # strip trailing |
fi

echo "Checking for aspirational test names (false coverage claims)..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# --- Check 1: Self-admitting comments in test bodies ---
# These phrases signal the test explicitly acknowledges it doesn't exercise
# the behavior its name claims.
SELF_ADMITTING_PATTERNS=(
    "this test verifies the .* path"
    "true .* requires"
    "which is an integration test concern"
    "cannot be tested here"
    "does not test what"
    "actually tests"
    "does not exercise"
    "cannot control .* in tests"
    "can.t control .* in tests"
    "we can.t .* in (unit |)tests"
)

for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" -v exempted="$EXEMPTED_TESTS" '
    # Detect test attribute
    /^[[:space:]]*(#\[test\]|#\[tokio::test)/ {
        in_test_attr = 1
        next
    }

    # Detect function declaration after test attribute
    in_test_attr && /^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)/ {
        in_test_attr = 0
        match($0, /fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        test_name = m[1]
        in_test = 1
        brace_depth = 0
        has_exemption = 0
        has_self_admit = 0
        self_admit_line = 0
        self_admit_text = ""
        delete body_lines
        body_count = 0
        # Count braces on the fn declaration line (opening { is typically here)
        for (i = 1; i <= length($0); i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }
        next
    }

    # Not a function after test attribute — reset
    in_test_attr && /^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn/ {
        in_test_attr = 0
    }

    in_test_attr && !/^[[:space:]]*$/ && !/^[[:space:]]*\/\// {
        in_test_attr = 0
    }

    # Track brace depth inside test function
    in_test {
        for (i = 1; i <= length($0); i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }
        body_count++
        body_lines[body_count] = $0

        # Check for exemption marker
        if ($0 ~ /aspirational-name:ok/) {
            has_exemption = 1
        }

        # Check for self-admitting phrases (in comments)
        # These phrases signal the test does NOT exercise the named behavior.
        # "this test verifies the X path" is only a finding when followed by
        # a disclaimer ("true Y requires...", "which is an integration test
        # concern") — alone, it describes what the test DOES verify.
        if ($0 ~ /\/\//) {
            lower = tolower($0)
            if (lower ~ /which is an integration test concern/ ||
                lower ~ /cannot be tested here/ ||
                lower ~ /does not test what/ ||
                lower ~ /does not exercise/ ||
                lower ~ /cannot control .* in tests/ ||
                lower ~ /can.t control .* in tests/ ||
                lower ~ /we can.t .* in (unit )?tests/ ||
                lower ~ /true .* requires .* (git repo|real|production|integration)/) {
                has_self_admit = 1
                self_admit_line = NR
                self_admit_text = $0
            }
        }

        # End of test function
        if (brace_depth == 0 && body_count > 1) {
            # Check exemption list
            if (exempted != "" && test_name ~ ("^(" exempted ")$")) {
                in_test = 0
                next
            }

            if (has_exemption) {
                in_test = 0
                next
            }

            if (has_self_admit) {
                printf "VIOLATION [Check 1]: %s:%d — test `%s` contains self-admitting comment:\n  %s\n  The test name claims coverage for behavior the comment admits it does not exercise.\n  Fix: rename the test to match actual behavior, or make the claimed behavior testable.\n  Exempt with: // aspirational-name:ok — <reason>\n\n", file, self_admit_line, test_name, self_admit_text
            }

            in_test = 0
        }
    }
    ' "$file" >> "$HITS_FILE" 2>/dev/null || true
done

# --- Check 2: Recovery-verb test names with degraded-state assertions ---
# A test named *_recovery_* or *_reverts_* or *_restores_* that ends by
# asserting the entity is still in its degraded state (Stale, Failed, Error)
# is testing persistence, not recovery.
for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" -v exempted="$EXEMPTED_TESTS" '
    /^[[:space:]]*(#\[test\]|#\[tokio::test)/ {
        in_test_attr = 1
        next
    }

    in_test_attr && /^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)/ {
        in_test_attr = 0
        match($0, /fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        test_name = m[1]

        # Only check tests with recovery-implying names
        if (test_name ~ /(recover|revert|restore|heal|resolve|return_to_active|back_to_active)/) {
            in_test = 1
            brace_depth = 0
            has_exemption = 0
            last_assert_line = 0
            last_assert_text = ""
            asserts_degraded = 0
            body_count = 0
            # Count braces on the fn declaration line
            for (i = 1; i <= length($0); i++) {
                c = substr($0, i, 1)
                if (c == "{") brace_depth++
                if (c == "}") brace_depth--
            }
        }
        next
    }

    in_test_attr && !/^[[:space:]]*$/ && !/^[[:space:]]*\/\// {
        in_test_attr = 0
    }

    in_test {
        for (i = 1; i <= length($0); i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }
        body_count++

        if ($0 ~ /aspirational-name:ok/) {
            has_exemption = 1
        }

        # Track last assert_eq! that references a status-like value
        if ($0 ~ /assert(_eq|_ne)?!/) {
            last_assert_line = NR
            last_assert_text = $0
            # Check if the assertion asserts a degraded state
            if ($0 ~ /(Stale|Failed|Error|Broken|Degraded|Inactive|Disabled|Rejected)/) {
                asserts_degraded = 1
            } else {
                asserts_degraded = 0
            }
        }

        if (brace_depth == 0 && body_count > 1) {
            if (exempted != "" && test_name ~ ("^(" exempted ")$")) {
                in_test = 0
                next
            }

            if (has_exemption) {
                in_test = 0
                next
            }

            if (asserts_degraded && last_assert_line > 0) {
                printf "VIOLATION [Check 2]: %s:%d — test `%s` has a recovery-implying name but its final assertion asserts a degraded state:\n  %s\n  A recovery test should assert the entity returns to a healthy state (e.g., Active).\n  If this tests persistence (not recovery), rename it accordingly.\n  Exempt with: // aspirational-name:ok — <reason>\n\n", file, last_assert_line, test_name, last_assert_text
            }

            in_test = 0
        }
    }
    ' "$file" >> "$HITS_FILE" 2>/dev/null || true
done

# --- Check 3: Negative-outcome test names with positive-outcome assertions ---
# A test named *_is_skipped, *_rejected, *_blocked, *_denied, *_excluded,
# *_filtered, *_drops_*, *_ignored, *_prevents_*, *_not_processed that ends
# with an assertion of a positive outcome (Clean, Success, Ok, Active,
# Approved, Merged, Processed, true, Passed) contradicts its own name.
# The test claims the item is NOT processed but proves it IS.
#
# See: specs/reviews/task-028.md R2 F7
for file in $(find "$CRATE_SRC" -name '*.rs' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" -v exempted="$EXEMPTED_TESTS" '
    /^[[:space:]]*(#\[test\]|#\[tokio::test)/ {
        in_test_attr = 1
        next
    }

    in_test_attr && /^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)/ {
        in_test_attr = 0
        match($0, /fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        test_name = m[1]

        # Only check tests with negative-outcome-implying names
        if (test_name ~ /(is_skipped|_skipped$|gets_skipped|is_rejected|_rejected$|gets_rejected|is_blocked|_blocked$|gets_blocked|is_denied|_denied$|is_excluded|_excluded$|is_filtered|_filtered$|_drops_|is_dropped|_ignored$|is_ignored|prevents_|not_processed|not_allowed|is_disallowed|does_not_process)/) {
            in_test = 1
            brace_depth = 0
            has_exemption = 0
            last_assert_line = 0
            last_assert_text = ""
            asserts_positive = 0
            body_count = 0
            for (i = 1; i <= length($0); i++) {
                c = substr($0, i, 1)
                if (c == "{") brace_depth++
                if (c == "}") brace_depth--
            }
        }
        next
    }

    in_test_attr && !/^[[:space:]]*$/ && !/^[[:space:]]*\/\// {
        in_test_attr = 0
    }

    in_test {
        for (i = 1; i <= length($0); i++) {
            c = substr($0, i, 1)
            if (c == "{") brace_depth++
            if (c == "}") brace_depth--
        }
        body_count++

        if ($0 ~ /aspirational-name:ok/) {
            has_exemption = 1
        }

        # Track last assert_eq! that references a status-like value
        if ($0 ~ /assert(_eq|_ne)?!/) {
            last_assert_line = NR
            last_assert_text = $0
            # Check if the assertion asserts a positive/success state
            # — contradicting the negative-outcome test name
            if ($0 ~ /(Clean|Success|Active|Approved|Merged|Processed|Passed|Completed|Ready|Valid|Resolved|Some\()/) {
                asserts_positive = 1
            } else if ($0 ~ /assert_eq!\(.*,\s*true\)/ || $0 ~ /assert_eq!\(\s*true\s*,/) {
                asserts_positive = 1
            } else {
                asserts_positive = 0
            }
        }

        if (brace_depth == 0 && body_count > 1) {
            if (exempted != "" && test_name ~ ("^(" exempted ")$")) {
                in_test = 0
                next
            }

            if (has_exemption) {
                in_test = 0
                next
            }

            if (asserts_positive && last_assert_line > 0) {
                printf "VIOLATION [Check 3]: %s:%d — test `%s` has a negative-outcome name (skipped/rejected/blocked/etc.) but its final assertion asserts a positive outcome:\n  %s\n  A test named \"is_skipped\" should assert the item was actually skipped (e.g., status == Skipped, result is None).\n  If the test verifies that dependencies ARE satisfied and processing succeeds, rename it to match (e.g., `deps_resolved_across_waves`).\n  Exempt with: // aspirational-name:ok — <reason>\n\n", file, last_assert_line, test_name, last_assert_text
            }

            in_test = 0
        }
    }
    ' "$file" >> "$HITS_FILE" 2>/dev/null || true
done

if [ -s "$HITS_FILE" ]; then
    cat "$HITS_FILE"
    VIOLATIONS=$(grep -c "^VIOLATION" "$HITS_FILE" || true)
    echo ""
    echo "Found $VIOLATIONS aspirational test name violation(s)."
    echo "See implementation prompt item 75 for guidance."
    exit 1
else
    echo "No aspirational test name violations found."
    exit 0
fi
