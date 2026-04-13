#!/usr/bin/env bash
# Architecture lint: detect nested iterations in time-scoped functions where
# the inner loop filters on status/type but NOT on a timestamp field.
#
# When a briefing or time-scoped query filters parent entities by `since`
# (e.g., `mr.updated_at >= since`) and then iterates child entities
# (e.g., gate results), the inner loop must also filter children by their
# own timestamp (e.g., `gr.finished_at >= since`). Without this, old child
# records leak into the briefing through recently-updated parents.
#
# This script detects the pattern:
#   for parent in ....filter(|x| x.updated_at >= since) {
#       ...child_results...
#       for child in results.iter().filter(|gr| gr.status == ...) {
#           // ^^^ filters on status but not on a timestamp field
#
# Legitimate cases can be exempted with:
#   // time-scope:ok — <reason>
#
# See: specs/reviews/task-013.md F3
#
# Run by pre-commit and CI.

set -euo pipefail

RUST_SRC="crates"
FAIL=0

if [ ! -d "$RUST_SRC" ]; then
    echo "Skipping nested time-scope check: $RUST_SRC not found"
    exit 0
fi

echo "Checking for missing child-entity timestamp filters in time-scoped queries..."

# ── Check 1: Inner filter on status without timestamp in time-scoped functions ──
#
# Strategy: Find functions that contain `>= since` (time-scoped) and also
# contain an inner `.filter(...)` that checks `.status ==` but does NOT
# check a timestamp field (finished_at, created_at, updated_at) in the
# same filter closure.
#
# We search for `.filter(` closures containing `.status ==` and then
# check whether the same filter also references a timestamp comparison.

# Find Rust files containing `>= since` (time-scoped functions)
TIME_SCOPED_FILES=$(grep -rl '>= since\|>=since' "$RUST_SRC" \
    --include='*.rs' \
    | grep -v '/tests/\|_test\.rs\|/target/' \
    || true)

for file in $TIME_SCOPED_FILES; do
    # Extract line numbers of inner .filter() closures that check .status ==
    # but don't check a timestamp field within the same logical block.
    #
    # We use a multi-line approach: find .filter(|..| ...status == ...) blocks
    # and check if they also contain finished_at/created_at/updated_at >= since

    # Get all filter closures checking status
    STATUS_FILTER_LINES=$(grep -n '\.filter(|.*status ==' "$file" \
        | grep -v '// time-scope:ok' \
        || true)

    if [ -z "$STATUS_FILTER_LINES" ]; then
        continue
    fi

    while IFS= read -r match; do
        LINE_NUM=$(echo "$match" | cut -d: -f1)
        LINE_CONTENT=$(echo "$match" | cut -d: -f2-)

        # Check if this filter closure also contains a timestamp comparison.
        # Look at the filter line and the next 5 lines for timestamp fields.
        CONTEXT=$(sed -n "${LINE_NUM},$((LINE_NUM + 5))p" "$file")

        if echo "$CONTEXT" | grep -qE '(finished_at|created_at|updated_at|timestamp).*>= *since|>= *since.*(finished_at|created_at|updated_at|timestamp)'; then
            # Has a timestamp filter — OK
            continue
        fi

        # Check if this is inside a time-scoped function (has >= since elsewhere)
        # by looking for >= since in the surrounding function
        FUNC_CONTEXT=$(sed -n "$((LINE_NUM > 50 ? LINE_NUM - 50 : 1)),${LINE_NUM}p" "$file")
        if echo "$FUNC_CONTEXT" | grep -qE '>= *since'; then
            echo "  $file:$LINE_NUM: inner filter checks status but not timestamp in time-scoped context"
            echo "    $LINE_CONTENT"
            FAIL=1
        fi
    done <<< "$STATUS_FILTER_LINES"
done

# ── Check 2: Test timestamp uniformity in time-scoped test functions ──
#
# Detect tests for time-scoped functions where parent and child entities
# use identical timestamps, making it impossible to detect missing child
# timestamp filters.
#
# Pattern: test functions containing both `updated_at` and `finished_at`
# assignments with the same literal value and a `since` parameter.

TEST_FILES=$(grep -rl 'assemble_briefing\|fn briefing_' "$RUST_SRC" \
    --include='*.rs' \
    | grep -E 'test|_test\.rs' \
    || true)

# Also check test modules in non-test files
TEST_FILES_INLINE=$(grep -rl '#\[cfg(test)\]' "$RUST_SRC" \
    --include='*.rs' \
    || true)

ALL_TEST_FILES=$(echo -e "${TEST_FILES}\n${TEST_FILES_INLINE}" | sort -u | grep -v '^$' || true)

for file in $ALL_TEST_FILES; do
    # Find test functions that call assemble_briefing
    TEST_FUNCS=$(grep -n 'async fn briefing_\|fn test_briefing' "$file" || true)

    if [ -z "$TEST_FUNCS" ]; then
        continue
    fi

    # For each test function, check if it creates both parent (updated_at)
    # and child (finished_at) with the same timestamp value
    while IFS= read -r func_match; do
        FUNC_LINE=$(echo "$func_match" | cut -d: -f1)
        FUNC_NAME=$(echo "$func_match" | grep -oP 'fn \K\w+' || echo "unknown")

        # Get function body (next 80 lines should be enough for most test functions)
        FUNC_BODY=$(sed -n "${FUNC_LINE},$((FUNC_LINE + 80))p" "$file")

        # Extract updated_at and finished_at literal values
        UPDATED_AT_VALS=$(echo "$FUNC_BODY" | grep -oP 'updated_at[: =]+\K\d+' || true)
        FINISHED_AT_VALS=$(echo "$FUNC_BODY" | grep -oP 'finished_at[: =]+Some\(\K\d+' || true)
        SINCE_VAL=$(echo "$FUNC_BODY" | grep -oP 'assemble_briefing.*,\s*\K\d+' || true)

        if [ -n "$UPDATED_AT_VALS" ] && [ -n "$FINISHED_AT_VALS" ] && [ -n "$SINCE_VAL" ]; then
            # Check if any updated_at == finished_at (same value, both after since)
            for ua in $UPDATED_AT_VALS; do
                for fa in $FINISHED_AT_VALS; do
                    if [ "$ua" = "$fa" ] && [ "$ua" -ge "$SINCE_VAL" ] 2>/dev/null; then
                        echo "  WARNING: $file:$FUNC_LINE ($FUNC_NAME): parent updated_at=$ua and child finished_at=$fa are identical (since=$SINCE_VAL)"
                        echo "    This test cannot detect a missing child-level timestamp filter."
                        echo "    Add a test case with finished_at < since to verify child filtering."
                        # This is a warning, not a hard failure — existing tests may need revision
                    fi
                done
            done
        fi
    done <<< "$TEST_FUNCS"
done

if [ "$FAIL" -eq 0 ]; then
    echo "Nested time-scope check passed."
    exit 0
else
    echo ""
    echo "NESTED TIME-SCOPE VIOLATIONS found."
    echo ""
    echo "  When a time-scoped query filters parent entities by 'since' and then"
    echo "  iterates child entities, the inner loop must also filter children by"
    echo "  their own timestamp field (e.g., gr.finished_at >= since)."
    echo ""
    echo "  Without this, old child records leak into the briefing through"
    echo "  recently-updated parents."
    echo ""
    echo "  Fix: Add a timestamp check to the inner filter:"
    echo "    .filter(|gr| gr.status == Failed && gr.finished_at.map_or(false, |t| t >= since))"
    echo ""
    echo "  Exempt with: // time-scope:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-013.md F3"
    exit 1
fi
