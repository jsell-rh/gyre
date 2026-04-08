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

# ── Check 2: Explicit priority overrides via _with_priority calls ────────
#
# The default_priority() check (above) catches wrong values in the enum.
# But code can bypass the default by calling a _with_priority variant
# with an explicit integer argument.  This was the exact mechanism for
# TASK-008 F2 (a repeat of TASK-007 R1 F2): the code called
# create_violation_notifications_with_priority(state, ..., 3) instead of
# using the default-priority function.
#
# This check scans for explicit priority arguments at call sites of
# _with_priority functions and verifies them against the spec table.
#
# See: specs/reviews/task-008.md F2 (repeat of TASK-007 R1 F2)

echo ""
echo "=== Check 2: Explicit priority overrides ==="

# Build a lookup of notification type -> spec priority from the table above
declare -A PRIORITY_MAP
for entry in "${SPEC_PRIORITIES[@]}"; do
    TYPE=$(echo "$entry" | cut -d: -f1)
    EXPECTED=$(echo "$entry" | cut -d: -f2)
    PRIORITY_MAP["$TYPE"]="$EXPECTED"
done

# Find all calls to functions ending in _with_priority that pass an explicit
# integer as the last argument.  Pattern: _with_priority(\n...\n  N,\n) or
# inline _with_priority(..., N).
#
# We scan all non-test Rust source files under crates/gyre-server/src/.
for file in $(find crates/gyre-server/src -name '*.rs' -type f 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Use awk to detect _with_priority calls with explicit numeric priority
    # and correlate them with the notification type (ConstraintViolation etc.)
    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test = 1 }
    in_test { next }

    # Track calls to _with_priority functions
    /_with_priority\(/ {
        in_priority_call = 1
        priority_call_line = NR
        priority_call_text = $0
        paren_depth = 0
        # Count opening parens on this line
        n = split($0, chars, "")
        for (i = 1; i <= n; i++) {
            if (chars[i] == "(") paren_depth++
            if (chars[i] == ")") paren_depth--
        }
        # Check for inline priority arg: ..., N) on same line
        if (paren_depth <= 0) {
            # Extract last numeric arg before closing paren
            if (match($0, /,\s*([0-9]+)\s*,?\s*\)/, m)) {
                printf "EXPLICIT_PRIORITY:%s:%d:%s\n", file, NR, m[1]
            }
            in_priority_call = 0
        }
        next
    }

    # Continue tracking multi-line _with_priority call
    in_priority_call {
        n = split($0, chars, "")
        for (i = 1; i <= n; i++) {
            if (chars[i] == "(") paren_depth++
            if (chars[i] == ")") paren_depth--
        }
        # Look for the priority argument (bare integer, usually last before closing paren)
        if (paren_depth <= 0) {
            # This line likely has the closing ); check previous lines stored
            # Actually, look for a bare integer on a line by itself (common Rust formatting)
            if (match($0, /^\s*([0-9]+)\s*,/, m)) {
                printf "EXPLICIT_PRIORITY:%s:%d:%s\n", file, NR, m[1]
            }
            in_priority_call = 0
        } else if (match($0, /^\s*([0-9]+)\s*,\s*(\/\/.*)?$/, m)) {
            # Bare integer on its own line with possible comment
            printf "EXPLICIT_PRIORITY:%s:%d:%s\n", file, NR, m[1]
        }
    }
    ' "$file"
done > /tmp/check-notif-priority-$$

if [ -f /tmp/check-notif-priority-$$ ]; then
    while IFS=: read -r tag efile eline epriority; do
        [ "$tag" = "EXPLICIT_PRIORITY" ] || continue

        # For ConstraintViolation notifications, the spec says priority 2.
        # Check if this explicit override matches.
        EXPECTED_PRIORITY="${PRIORITY_MAP[ConstraintViolation]:-}"

        if [ -n "$EXPECTED_PRIORITY" ] && [ "$epriority" != "$EXPECTED_PRIORITY" ]; then
            echo ""
            echo "EXPLICIT PRIORITY OVERRIDE MISMATCH: $efile:$eline"
            echo "  Call to _with_priority passes explicit priority: $epriority"
            echo "  Spec requires: priority $EXPECTED_PRIORITY (authorization-provenance.md §7.5)"
            echo "  The spec makes no distinction between push-time and merge-time priority --"
            echo "  all constraint violation notifications are priority $EXPECTED_PRIORITY."
            echo ""
            echo "  This is a repeat error class (TASK-007 R1 F2 -> TASK-008 R1 F2)."
            echo "  Use create_violation_notifications (which uses the correct default)"
            echo "  instead of the _with_priority variant, OR pass $EXPECTED_PRIORITY."
            echo ""
            # Mark exemption comment
            if grep -q 'priority-override:ok' "$efile" 2>/dev/null; then
                echo "  (exempted by // priority-override:ok comment)"
            else
                VIOLATIONS=$((VIOLATIONS + 1))
                FAIL=1
            fi
        fi
    done < /tmp/check-notif-priority-$$
    rm -f /tmp/check-notif-priority-$$
fi

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Notification priority lint passed: ${#SPEC_PRIORITIES[@]} type(s) checked."
    exit 0
else
    echo "Fix: Update the priority value in default_priority() to match the spec."
    echo "     Also update the doc comment on the variant to match."
    echo "     Remove explicit _with_priority overrides or fix them to match the spec."
    echo "     The spec is always the source of truth -- verify against the spec"
    echo "     section, not the task file (task files may have transcription errors)."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
