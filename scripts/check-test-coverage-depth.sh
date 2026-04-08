#!/usr/bin/env bash
# Architecture lint: detect integration tests that structurally cannot exercise
# the logic they claim to test.
#
# Two failure patterns:
#   1. Tests that pass invalid fixtures (e.g., "/nonexistent/path" as a repo
#      path) to multi-stage functions, causing an early return before the core
#      logic executes.  The test name claims to "run constraints" but constraint
#      evaluation is never reached.
#
#   2. Tests whose only assertion is "no panic = success" for functions that
#      are designed to never panic (graceful degradation).  The assertion is
#      tautological — it always passes regardless of correctness.
#
# See: specs/reviews/task-007.md F9
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
FAIL=0

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping test coverage depth check: $CRATE_SRC not found"
    exit 0
fi

echo "Checking for early-exit test illusions..."

# ── Check 1: Tests using /nonexistent paths in evaluation functions ──────
#
# Integration tests for evaluate_push_constraints or evaluate_merge_constraints
# that pass "/nonexistent" as a repo_path will always exit before constraint
# evaluation because compute_push_diff / compute_commit_diff will fail.
#
# Pattern: a #[test] function that calls evaluate_push_constraints or
# evaluate_merge_constraints with a string containing "/nonexistent".

NONEXISTENT_PATH_TESTS=""

# Find Rust test files containing evaluation function calls with /nonexistent
for file in $(grep -rl 'evaluate_push_constraints\|evaluate_merge_constraints' "$CRATE_SRC" --include='*.rs' 2>/dev/null || true); do
    # Only check test modules (after #[cfg(test)] or in tests/ directory)
    # Extract test function blocks that call evaluation functions with /nonexistent
    HITS=$(awk '
        /^\s*#\[tokio::test\]|^\s*#\[test\]/ { in_test = 1; test_line = NR; test_name = ""; next }
        in_test && /async fn |fn / {
            match($0, /fn ([a-zA-Z_0-9]+)/, m)
            test_name = m[1]
        }
        in_test && /evaluate_(push|merge)_constraints/ { has_eval = 1 }
        in_test && /\/nonexistent/ { has_nonexistent = 1 }
        in_test && /^    \}$/ && has_eval && has_nonexistent {
            print FILENAME ":" test_line ": test \"" test_name "\" calls evaluation function with /nonexistent path"
            in_test = 0; has_eval = 0; has_nonexistent = 0; test_name = ""
        }
        in_test && /^    \}$/ { in_test = 0; has_eval = 0; has_nonexistent = 0; test_name = "" }
    ' "$file" 2>/dev/null)

    if [ -n "$HITS" ]; then
        NONEXISTENT_PATH_TESTS="$NONEXISTENT_PATH_TESTS$HITS
"
    fi
done

if [ -n "$NONEXISTENT_PATH_TESTS" ]; then
    echo ""
    echo "EARLY-EXIT TEST ILLUSION — invalid repo path in evaluation tests:"
    while IFS= read -r line; do
        [ -n "$line" ] && echo "  $line"
    done <<< "$NONEXISTENT_PATH_TESTS"
    echo ""
    echo "  These tests call evaluate_push_constraints or evaluate_merge_constraints"
    echo "  with \"/nonexistent\" as the repo path.  The function will fail at"
    echo "  compute_push_diff / compute_commit_diff (before constraint evaluation)"
    echo "  and return early.  The test never exercises the constraint logic it"
    echo "  claims to test."
    echo ""
    echo "  Fix: Use tempfile::tempdir() to create a real git repository with"
    echo "  known files.  Then assert on observable side effects: events emitted"
    echo "  to broadcast channels, notifications created, or specific constraint"
    echo "  evaluation results."
    echo ""
    echo "  See: specs/reviews/task-007.md F9"
    echo ""
    FAIL=1
fi

# ── Check 2: Tautological "no panic = success" tests ──────────────────
#
# Tests that call a multi-stage evaluation function but contain no assert!,
# assert_eq!, or assert_ne! after the function call.  If the function is
# designed to never panic (only log warnings and return), "no panic" is not
# a meaningful assertion.
#
# Heuristic: a test function calls an evaluate_* function, and the only
# assertion-like content is a comment containing "no panic" or "No panic".

NO_ASSERT_TESTS=""

for file in $(grep -rl 'evaluate_push_constraints\|evaluate_merge_constraints' "$CRATE_SRC" --include='*.rs' 2>/dev/null || true); do
    HITS=$(awk '
        /^\s*#\[tokio::test\]|^\s*#\[test\]/ { in_test = 1; test_line = NR; test_name = ""; has_eval = 0; past_eval = 0; has_assert = 0; has_no_panic = 0; next }
        in_test && /async fn |fn / {
            match($0, /fn ([a-zA-Z_0-9]+)/, m)
            test_name = m[1]
        }
        in_test && /evaluate_(push|merge)_constraints/ { has_eval = 1; past_eval = 1 }
        in_test && past_eval && /assert!|assert_eq!|assert_ne!/ { has_assert = 1 }
        in_test && /[Nn]o panic.*success|success.*[Nn]o panic/ { has_no_panic = 1 }
        in_test && /^    \}$/ {
            if (has_eval && !has_assert && has_no_panic) {
                print FILENAME ":" test_line ": test \"" test_name "\" uses tautological \"no panic = success\" assertion"
            }
            in_test = 0; has_eval = 0; past_eval = 0; has_assert = 0; has_no_panic = 0
        }
    ' "$file" 2>/dev/null)

    if [ -n "$HITS" ]; then
        NO_ASSERT_TESTS="$NO_ASSERT_TESTS$HITS
"
    fi
done

if [ -n "$NO_ASSERT_TESTS" ]; then
    echo ""
    echo "TAUTOLOGICAL ASSERTION — 'no panic = success' on gracefully-degrading function:"
    while IFS= read -r line; do
        [ -n "$line" ] && echo "  $line"
    done <<< "$NO_ASSERT_TESTS"
    echo ""
    echo "  These tests call evaluation functions but contain no real assertions"
    echo "  (assert!, assert_eq!, assert_ne!).  The only check is a comment saying"
    echo "  \"No panic = success.\"  Since these functions never panic (they log"
    echo "  warnings and return), this assertion always passes regardless of"
    echo "  whether the test exercises any meaningful logic."
    echo ""
    echo "  Fix: Assert on observable side effects:"
    echo "    - Events emitted to state.message_broadcast_tx (subscribe before call)"
    echo "    - Notifications created in state.notifications"
    echo "    - Specific VerificationResult fields (valid/invalid, constraint pass/fail)"
    echo ""
    echo "  See: specs/reviews/task-007.md F9"
    echo ""
    FAIL=1
fi

# ── Result ──────────────────────────────────────────────────────────────

if [ "$FAIL" -eq 0 ]; then
    echo "Test coverage depth check passed."
    exit 0
else
    echo "Fix: Replace /nonexistent paths with real temp repos and add real assertions."
    exit 1
fi
