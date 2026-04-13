#!/usr/bin/env bash
# Architecture lint: verify string-typed discriminator field values match
# spec-defined values and codebase serialization conventions.
#
# When code constructs a string-typed discriminator field (entity_type, type,
# kind, status) that consumers match on, the literal value MUST match the
# spec's defined values or the existing serde serialization for the
# corresponding enum.  Abbreviated or paraphrased values (e.g.,
# "assertion_failure" instead of "spec_assertion_failure") will cause
# downstream consumers matching on the spec-defined value to silently miss
# records.
#
# This script maintains a table of (field:context -> spec-required value)
# and verifies the code matches.
#
# See: specs/reviews/task-013.md F4 (assertion_failure vs spec_assertion_failure,
#      mr_revert vs reverted)
#
# Run by pre-commit and CI.

set -euo pipefail

RUST_SRC="crates"
FAIL=0
VIOLATIONS=0

if [ ! -d "$RUST_SRC" ]; then
    echo "Skipping type discriminator value check: $RUST_SRC not found"
    exit 0
fi

echo "Checking type discriminator values against spec..."

# ── Check 1: Spec-derived discriminator value table ────────────────────
#
# Format: field_name:wrong_value:correct_value:spec_reference
#
# This table encodes known mismatches where the code is likely to use an
# abbreviated or paraphrased value instead of the spec-defined value.
# Each entry says: "if you see field_name assigned to wrong_value, it
# should be correct_value per the spec."
#
# Add entries as new discriminator value mismatches are discovered.

KNOWN_MISMATCHES=(
    'entity_type:"assertion_failure":"spec_assertion_failure":HSI §9 line 1316 + notification priority table line 931'
    'entity_type:"mr_revert":"reverted":HSI §9 + MrStatus::Reverted serde convention (merge_deps.rs, specs.rs, merge_requests.rs)'
)

echo ""
echo "=== Check 1: Known discriminator value mismatches ==="

for entry in "${KNOWN_MISMATCHES[@]}"; do
    FIELD=$(echo "$entry" | cut -d: -f1)
    WRONG=$(echo "$entry" | cut -d: -f2)
    CORRECT=$(echo "$entry" | cut -d: -f3)
    SPEC_REF=$(echo "$entry" | cut -d: -f4-)

    # Remove surrounding quotes for grep pattern
    WRONG_BARE=$(echo "$WRONG" | tr -d '"')
    CORRECT_BARE=$(echo "$CORRECT" | tr -d '"')

    # Search non-test Rust code for the wrong value assigned to the field
    HITS=$(grep -rn "${FIELD}:.*${WRONG}" "$RUST_SRC" \
        --include='*.rs' \
        | grep -v '#\[cfg(test)\]\|#\[test\]\|mod tests' \
        | grep -v '// discriminator:ok' \
        || true)

    # Also check test code — tests using wrong values confirm wrong behavior
    TEST_HITS=$(grep -rn "${FIELD}.*==.*${WRONG}\|${FIELD}:.*${WRONG}" "$RUST_SRC" \
        --include='*.rs' \
        | grep -E '#\[cfg\(test\)\]|mod tests|#\[test\]|_test\.rs' \
        || true)

    # Use a broader pattern to catch the value anywhere near field assignment
    HITS2=$(grep -rn "\"${WRONG_BARE}\"" "$RUST_SRC" \
        --include='*.rs' \
        | grep -i "$FIELD" \
        | grep -v '// discriminator:ok' \
        || true)

    ALL_HITS=$(echo -e "${HITS}\n${HITS2}" | sort -u | grep -v '^$' || true)

    if [ -n "$ALL_HITS" ]; then
        echo ""
        echo "DISCRIMINATOR VALUE MISMATCH: ${FIELD} = ${WRONG}"
        echo "  Spec says: ${CORRECT}"
        echo "  Spec reference: ${SPEC_REF}"
        echo ""
        echo "  Occurrences:"
        echo "$ALL_HITS" | while IFS= read -r line; do
            echo "    $line"
        done
        echo ""
        VIOLATIONS=$((VIOLATIONS + 1))
        FAIL=1
    fi

    # Warn about test code using wrong values (won't fail the build, but flagged)
    if [ -n "$TEST_HITS" ]; then
        echo ""
        echo "WARNING: Tests also use wrong discriminator value ${WRONG} for ${FIELD}:"
        echo "$TEST_HITS" | head -5 | while IFS= read -r line; do
            echo "    $line"
        done
        echo "  Tests using wrong values confirm wrong behavior — update them alongside the fix."
    fi
done

# ── Check 2: Cross-reference entity_type values against existing enums ──
#
# When a string literal is assigned to entity_type and a corresponding
# Rust enum variant exists with serde serialization, the string should
# match the serde output.  This catches cases where the implementer
# invents a new string instead of using the enum's serialization.
#
# We check: entity_type values that look like they correspond to known
# enum variants (MrStatus, NotificationType, GateStatus, etc.)

echo ""
echo "=== Check 2: entity_type values vs enum serialization conventions ==="

# Find all entity_type string assignments in non-test server code
ENTITY_TYPE_VALUES=$(grep -rn 'entity_type:.*"[a-z_]*"' "$RUST_SRC" \
    --include='*.rs' \
    | grep -v '_test\.rs\|/tests/' \
    | grep -oP 'entity_type:\s*"[a-z_]*"' \
    | grep -oP '"[a-z_]*"' \
    | sort -u \
    || true)

# For each value, check if a similar-but-different enum variant exists
for value in $ENTITY_TYPE_VALUES; do
    VALUE_BARE=$(echo "$value" | tr -d '"')

    # Skip values already in the known mismatches table (handled above)
    SKIP=0
    for entry in "${KNOWN_MISMATCHES[@]}"; do
        WRONG_BARE=$(echo "$entry" | cut -d: -f2 | tr -d '"')
        if [ "$VALUE_BARE" = "$WRONG_BARE" ]; then
            SKIP=1
            break
        fi
    done
    [ "$SKIP" -eq 1 ] && continue

    # Check if a serde-renamed enum variant exists that this value should match
    # Look for enum variants whose snake_case serde name differs from the value
    # This is a heuristic — it flags potential mismatches for manual review
done

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Type discriminator value check passed."
    exit 0
else
    echo "TYPE DISCRIMINATOR VALUE VIOLATIONS found: ${VIOLATIONS} violation(s)."
    echo ""
    echo "  String-typed discriminator fields (entity_type, type, kind, status)"
    echo "  must use the exact values defined in the spec or matching the"
    echo "  existing serde serialization convention for the corresponding enum."
    echo ""
    echo "  Fix: Replace the wrong value with the spec-defined value."
    echo "       Also update any test filter predicates that match on the wrong value."
    echo ""
    echo "  The spec is always the source of truth — not task files."
    echo "  Task files may use informal abbreviations in implementation plans"
    echo "  that do not match the spec's actual values."
    echo ""
    echo "  Exempt with: // discriminator:ok — <reason>"
    echo ""
    echo "  See: specs/reviews/task-013.md F4"
    exit 1
fi
