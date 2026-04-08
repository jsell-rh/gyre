#!/usr/bin/env bash
# Architecture lint: verify that event payload constructions include all
# spec-required fields for their MessageKind.
#
# When code emits an event (e.g., ConstraintViolation), the payload must
# include every field the spec defines for that event kind. A payload that
# substitutes task_id for attestation_id (or omits context_snapshot) compiles
# and runs, but violates the spec schema and breaks downstream consumers
# that expect the spec-defined fields.
#
# This script maintains a table of (MessageKind -> required fields) derived
# from the specs and verifies each field appears in the emit function.
#
# See: specs/reviews/task-007.md F1 (missing attestation_id and context_snapshot)
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking event payload spec-field completeness..."

# ── Spec-derived payload schemas ─────────────────────────────────────
#
# Each entry: MessageKind | required_field_1,required_field_2,...
# Source: authorization-provenance.md section 7.5
#
# Extend this table as new event kinds with spec-defined schemas are added.

SPEC_PAYLOADS=(
    "ConstraintViolation|attestation_id,constraint_name,expression,context_snapshot,action,agent_id,repo_id,timestamp"
)

for entry in "${SPEC_PAYLOADS[@]}"; do
    KIND="${entry%%|*}"
    FIELDS_CSV="${entry##*|}"
    IFS=',' read -ra REQUIRED_FIELDS <<< "$FIELDS_CSV"

    # Find non-test source files that emit this event kind.
    EMIT_FILES=$(grep -rln "MessageKind::${KIND}" "$SERVER_SRC" \
        --include='*.rs' || true)

    if [ -z "$EMIT_FILES" ]; then
        # No files emit this event kind yet — nothing to check.
        continue
    fi

    for file in $EMIT_FILES; do
        # Skip test files and test modules.
        # We check non-test production code only.
        basename_file=$(basename "$file")

        # Use awk to find functions that reference MessageKind::$KIND
        # (outside of test modules) and check for required fields.
        # Strategy: extract the function body that contains the MessageKind
        # reference and verify each required field appears as a string literal.

        # First, check if the MessageKind reference is only in test code.
        NON_TEST_HITS=$(awk '
            /^\s*#\[cfg\(test\)\]/ { in_test = 1; next }
            /^\s*mod\s+tests\s*\{/ { if (in_test) { test_depth = 1; next } }
            in_test && /\{/ { test_depth++ }
            in_test && /\}/ { test_depth--; if (test_depth <= 0) in_test = 0 }
            !in_test && /fn test_/ { in_test_fn = 1; next }
            in_test_fn && /^    \}/ { in_test_fn = 0; next }
            !in_test && !in_test_fn && /MessageKind::'"${KIND}"'/ { print NR": "$0 }
        ' "$file")

        if [ -z "$NON_TEST_HITS" ]; then
            # All references are in test code — skip.
            continue
        fi

        # Find the function(s) that emit this event kind (non-test).
        # Extract the function body and check for required fields.
        # We use a simpler heuristic: check if the required field string
        # appears anywhere in non-test code in the same file.

        for field in "${REQUIRED_FIELDS[@]}"; do
            # Check if the field name appears as a JSON key in non-test code.
            # Pattern: "field_name" (as a serde_json key) or field_name: (as a struct field)
            FIELD_FOUND=$(awk '
                /^\s*#\[cfg\(test\)\]/ { in_test = 1; next }
                /^\s*mod\s+tests\s*\{/ { if (in_test) { test_depth = 1; next } }
                in_test && /\{/ { test_depth++ }
                in_test && /\}/ { test_depth--; if (test_depth <= 0) in_test = 0 }
                !in_test && /fn test_/ { in_test_fn = 1; next }
                in_test_fn && /^    \}/ { in_test_fn = 0; next }
                !in_test && !in_test_fn && /["\x27]'"${field}"'["\x27]/ { found = 1 }
                END { if (found) print "yes" }
            ' "$file")

            if [ -z "$FIELD_FOUND" ]; then
                # Check for exemption comment on the MessageKind line.
                EXEMPT=$(grep "MessageKind::${KIND}" "$file" | grep -c '// event-payload:ok' || true)
                if [ "$EXEMPT" -gt 0 ]; then
                    continue
                fi

                echo ""
                echo "MISSING EVENT PAYLOAD FIELD: ${file}"
                echo "  Event kind: MessageKind::${KIND}"
                echo "  Missing spec-required field: \"${field}\""
                echo "  The spec defines this field as required in the ${KIND} payload."
                echo "  A payload without this field violates the spec schema."
                echo "  See: authorization-provenance.md section 7.5"
                echo "  See: specs/reviews/task-007.md F1"
                echo ""
                VIOLATIONS=$((VIOLATIONS + 1))
                FAIL=1
            fi
        done
    done
done

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Event payload spec-field lint passed: ${#SPEC_PAYLOADS[@]} event kind(s) checked."
    exit 0
else
    echo "Fix: Add the missing field(s) to the event payload construction."
    echo "     Reference the spec section that defines the event kind's schema."
    echo "     If a field is intentionally omitted, add '// event-payload:ok' on the"
    echo "     MessageKind::$KIND line (requires spec justification in commit message)."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
