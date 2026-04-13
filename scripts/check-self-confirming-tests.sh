#!/usr/bin/env bash
# Architecture lint: detect self-confirming test functions.
#
# A self-confirming test manually constructs the expected outcome
# (e.g., calls state.tasks.create(&task)) and then asserts that outcome
# exists (e.g., state.tasks.list_by_repo(...) returns the task), but
# never calls the production function that should produce that outcome.
#
# Detection signals:
#   1. Test body contains a .create( call with an adjacent comment
#      containing "like", "simulate", "would", or "as if" — indicating
#      the test is manually constructing what production code should do.
#   2. Test name implies automatic behavior (auto_create, detects,
#      triggers, generates, propagates) but only uses direct CRUD calls.
#
# Exemptions:
#   - Tests with "// self-confirming:ok" comment in the body
#   - Tests listed in scripts/self-confirming-test-exemptions.txt
#
# See: specs/reviews/task-020.md F3
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/self-confirming-test-exemptions.txt"

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping self-confirming test check: $CRATE_SRC not found"
    exit 0
fi

# Load exemption list (test function names, one per line, # comments allowed)
EXEMPTED_TESTS=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTED_TESTS=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' | tr '\n' '|' || true)
    EXEMPTED_TESTS="${EXEMPTED_TESTS%|}"  # strip trailing |
fi

echo "Checking for self-confirming test functions..."

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
        has_simulate_create = 0
        has_exempt_comment = 0
        test_start = NR
        # Buffer to check for simulation comments near .create( calls
        prev_line = ""
        next
    }
    # If we see a non-attribute, non-fn, non-blank line after test attr, reset
    in_test_attr { in_test_attr = 0 }

    # Inside test body — scan for simulation patterns
    in_test {
        if ($0 ~ /self-confirming:ok/) has_exempt_comment = 1

        # Detect comments that explicitly claim the following .create() calls
        # are simulating production code behavior. These are standalone comments
        # (not parenthetical asides) that indicate the test is manually
        # constructing what a production function should produce.
        #
        # True positive pattern (dependencies.rs):
        #   // Simulate: the push detection created a breaking change and a task.
        #   state.breaking_changes.create(&bc).await.unwrap();
        #   ...
        #   // Create task like detect_breaking_changes_on_push would.
        #   state.tasks.create(&task).await.unwrap();
        #
        # False positive pattern (NOT flagged — parenthetical usage):
        #   // Create task without task_type (simulates push-hook pre-approval task).
        #   create_task_with_type(app, "Pre-approval push-hook task", None).await;

        # Pattern 1: "Create X like <function_name> would" — standalone comment
        if ($0 ~ /^\s*\/\/.*[Cc]reate.*like.*would/) {
            has_simulate_create = 1
        }
        # Pattern 2: "// Simulate: ..." at start of comment (not parenthetical)
        # followed within 5 lines by a state.*.create( call
        if ($0 ~ /^\s*\/\/\s*[Ss]imulate:/) {
            simulate_window = 5
        }
        if (simulate_window > 0) {
            simulate_window--
            if ($0 ~ /state\.[a-z_]+\.create\(/) {
                has_simulate_create = 1
            }
        }

        prev_line = $0

        # Detect end of test function (closing brace at 4-space indent)
        if ($0 ~ /^    \}$/) {
            if (has_simulate_create && !has_exempt_comment) {
                printf "%s:%d: test \"%s\" has .create() call with simulation comment — likely self-confirming\n", file, test_start, test_name
            }
            in_test = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "SELF-CONFIRMING TESTS — tests that manually construct expected outcomes:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A self-confirming test manually creates the expected state"
    echo "  (e.g., state.tasks.create(&task)) and then asserts that state"
    echo "  exists — without calling the production function that should"
    echo "  create it. The test proves the repository round-trips data,"
    echo "  not that the feature works."
    echo ""
    echo "  Fix: Call the production function instead of manually"
    echo "  constructing its expected side effects:"
    echo "    - detect_breaking_changes_on_push(&state, ...) — then assert tasks"
    echo "    - If the function is hard to test, extract the side-effect"
    echo "      logic into a testable unit"
    echo ""
    echo "  If the test is intentionally a simulation (e.g., testing a"
    echo "  downstream consumer that requires pre-created data),"
    echo "  add comment: // self-confirming:ok — <reason>"
    echo "  Or add the test name to scripts/self-confirming-test-exemptions.txt"
    echo ""
    echo "  See: specs/reviews/task-020.md F3"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Self-confirming test check passed."
exit 0
