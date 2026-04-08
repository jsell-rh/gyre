#!/usr/bin/env bash
# Architecture lint: verify notification priority values match spec-defined
# values.
#
# When a spec defines a notification priority (e.g., "priority 2 -- high but
# not critical"), the NotificationType::default_priority() implementation
# must return that exact value. A transcription error in a task file or
# implementation mistake can introduce a wrong priority that compiles and
# tests pass, but violates the spec.
#
# This script maintains a table of (NotificationType -> spec-required
# priority) and verifies the code matches.
#
# See: specs/reviews/task-007.md F2 (priority 3 vs spec-required 2)
#
# Run by pre-commit and CI.

set -euo pipefail

NOTIFICATION_RS="crates/gyre-common/src/notification.rs"
FAIL=0
VIOLATIONS=0

if [ ! -f "$NOTIFICATION_RS" ]; then
    echo "ERROR: Cannot find $NOTIFICATION_RS"
    exit 1
fi

echo "Checking notification priority values against spec..."

# ── Spec-derived priority table ──────────────────────────────────────
#
# Format: NotificationType:expected_priority:spec_reference
#
# Add entries here as specs define notification priorities.
# Only include types where the spec gives an EXPLICIT priority value.
# Types without a spec-defined priority are not checked.

SPEC_PRIORITIES=(
    "ConstraintViolation:2:authorization-provenance.md section 7.5"
)

for entry in "${SPEC_PRIORITIES[@]}"; do
    TYPE=$(echo "$entry" | cut -d: -f1)
    EXPECTED=$(echo "$entry" | cut -d: -f2)
    SPEC_REF=$(echo "$entry" | cut -d: -f3-)

    # Find the priority value in the default_priority() match arm.
    # Pattern: Self::$TYPE => $N
    ACTUAL=$(grep -A0 "Self::${TYPE} =>" "$NOTIFICATION_RS" \
        | grep -oP '=> \K[0-9]+' \
        | head -1 || true)

    if [ -z "$ACTUAL" ]; then
        echo ""
        echo "WARNING: Could not find priority for NotificationType::${TYPE}"
        echo "  Expected to find 'Self::${TYPE} => N' in $NOTIFICATION_RS"
        continue
    fi

    if [ "$ACTUAL" != "$EXPECTED" ]; then
        echo ""
        echo "PRIORITY MISMATCH: NotificationType::${TYPE}"
        echo "  Code says: priority ${ACTUAL}"
        echo "  Spec says: priority ${EXPECTED}"
        echo "  Spec reference: ${SPEC_REF}"
        echo "  The spec is the source of truth -- not task files or code comments."
        echo ""
        VIOLATIONS=$((VIOLATIONS + 1))
        FAIL=1
    fi

    # Also check the doc comment on the variant for consistency.
    DOC_PRIORITY=$(grep -B1 "^\s*${TYPE}," "$NOTIFICATION_RS" \
        | grep '///' \
        | grep -oP 'Priority \K[0-9]+' \
        || true)

    if [ -n "$DOC_PRIORITY" ] && [ "$DOC_PRIORITY" != "$EXPECTED" ]; then
        echo ""
        echo "DOC COMMENT MISMATCH: NotificationType::${TYPE}"
        echo "  Doc comment says: Priority ${DOC_PRIORITY}"
        echo "  Spec says: priority ${EXPECTED}"
        echo "  Update the doc comment to match the spec."
        echo ""
        VIOLATIONS=$((VIOLATIONS + 1))
        FAIL=1
    fi
done

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Notification priority lint passed: ${#SPEC_PRIORITIES[@]} type(s) checked."
    exit 0
else
    echo "Fix: Update the priority value in default_priority() to match the spec."
    echo "     Also update the doc comment on the variant to match."
    echo "     The spec is always the source of truth -- verify against the spec"
    echo "     section, not the task file (task files may have transcription errors)."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
