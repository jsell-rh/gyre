#!/usr/bin/env bash
# Architecture lint: verify that irretractable side effects (notifications,
# analytics events) are not emitted inside rollback-capable transactional
# loops.
#
# Flaw class: Premature side-effect emission in transactional operations
# (see TASK-027 R1 F1)
#
# When a multi-step operation can be rolled back (e.g., atomic group merge),
# notifications and analytics events emitted inside the loop cannot be
# retracted after rollback. This produces contradictory user-facing artifacts
# (e.g., "MR merged" followed by "group rolled back").
#
# Check 1: Notification calls inside functions with rollback paths
#   Detects functions that have BOTH:
#   (a) A rollback indicator (rollback_, reset_branch, requeue, revert)
#   (b) notify_ or analytics.record calls inside loop bodies (for/while)
#   This suggests side effects may fire before the transactional operation
#   commits, and cannot be retracted on rollback.
#
# Check 2: Analytics recording inside functions with rollback paths
#   Same as Check 1 but specifically for analytics.record() calls.
#
# The fix is to collect side-effect data during the loop and emit them
# in a post-loop success block.
#
# Exempt a line with: // transactional-side-effect:ok — <reason>
#
# Scope: all non-test Rust source files under crates/gyre-server/src/
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
VIOLATIONS=0
CHECKED=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking for irretractable side effects inside rollback-capable operations..."

# ── Check 1 & 2: Notification/analytics calls inside loops in rollback functions ──

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" '
    # Skip test modules
    /^\s*#\[cfg\(test\)\]/ { in_test_module = 1; next }

    # Detect function boundaries
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        # Check previous function
        if (fn_name != "" && !has_exempt && has_rollback) {
            if (notify_in_loop) {
                printf "NOTIFICATION INSIDE ROLLBACK-CAPABLE LOOP: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has rollback capability (line %d) and emits notifications\n", rollback_line
                printf "  inside a loop (line %d). If a later iteration fails and triggers\n", notify_line
                printf "  rollback, already-sent notifications cannot be retracted.\n"
                printf "  Fix: Collect notification data in the loop; emit after all\n"
                printf "  iterations succeed (post-loop success block).\n"
                printf "  See: specs/reviews/task-027.md R1 F1\n\n"
                violations++
            }
            if (analytics_in_loop) {
                printf "ANALYTICS EVENT INSIDE ROLLBACK-CAPABLE LOOP: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has rollback capability (line %d) and records analytics\n", rollback_line
                printf "  inside a loop (line %d). If a later iteration fails and triggers\n", analytics_line
                printf "  rollback, already-recorded analytics events are irretractable.\n"
                printf "  Fix: Collect analytics data in the loop; record after all\n"
                printf "  iterations succeed (post-loop success block).\n"
                printf "  See: specs/reviews/task-027.md R1 F1\n\n"
                violations++
            }
        }
        if (fn_name != "" && has_rollback) checked++

        # Parse function name
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        fn_name = m[1]
        fn_start = NR
        has_rollback = 0
        rollback_line = 0
        notify_in_loop = 0
        notify_line = 0
        analytics_in_loop = 0
        analytics_line = 0
        loop_depth = 0
        has_exempt = 0
        # Skip test functions
        if (fn_name ~ /^test_/ || in_test_module) fn_name = ""
        next
    }
    fn_name != "" {
        # Check for exemption
        if ($0 ~ /transactional-side-effect:ok/) has_exempt = 1

        # Track loop depth
        if ($0 ~ /\bfor\b.*\{/ || $0 ~ /\bwhile\b.*\{/ || $0 ~ /\bloop\s*\{/) {
            loop_depth++
        }
        # Track closing braces (approximate — sufficient for heuristic)
        # We only need to know if we are inside ANY loop, not precise nesting
        if (loop_depth > 0 && $0 ~ /^\s*\}/) {
            loop_depth--
        }

        # Detect rollback indicators
        if ($0 ~ /rollback_|reset_branch|\.requeue|revert.*status|MrStatus::Open/) {
            if (!has_rollback) {
                has_rollback = 1
                rollback_line = NR
            }
        }

        # Detect notification calls inside loops (skip exempt lines)
        if (loop_depth > 0 && $0 ~ /notify_/ && $0 !~ /transactional-side-effect:ok/) {
            if (!notify_in_loop) {
                notify_in_loop = 1
                notify_line = NR
            }
        }

        # Detect analytics recording inside loops (skip exempt lines)
        if (loop_depth > 0 && $0 ~ /analytics\.record/ && $0 !~ /transactional-side-effect:ok/) {
            if (!analytics_in_loop) {
                analytics_in_loop = 1
                analytics_line = NR
            }
        }
    }
    END {
        # Check last function
        if (fn_name != "" && !has_exempt && has_rollback) {
            if (notify_in_loop) {
                printf "NOTIFICATION INSIDE ROLLBACK-CAPABLE LOOP: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has rollback capability (line %d) and emits notifications\n", rollback_line
                printf "  inside a loop (line %d). If a later iteration fails and triggers\n", notify_line
                printf "  rollback, already-sent notifications cannot be retracted.\n"
                printf "  Fix: Collect notification data in the loop; emit after all\n"
                printf "  iterations succeed (post-loop success block).\n"
                printf "  See: specs/reviews/task-027.md R1 F1\n\n"
                violations++
            }
            if (analytics_in_loop) {
                printf "ANALYTICS EVENT INSIDE ROLLBACK-CAPABLE LOOP: %s in %s:%d\n", fn_name, file, fn_start
                printf "  Function has rollback capability (line %d) and records analytics\n", rollback_line
                printf "  inside a loop (line %d). If a later iteration fails and triggers\n", analytics_line
                printf "  rollback, already-recorded analytics events are irretractable.\n"
                printf "  Fix: Collect analytics data in the loop; record after all\n"
                printf "  iterations succeed (post-loop success block).\n"
                printf "  See: specs/reviews/task-027.md R1 F1\n\n"
                violations++
            }
        }
        if (fn_name != "" && has_rollback) checked++
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file" | while IFS= read -r line; do
        case "$line" in
            SUMMARY:*)
                c=$(echo "$line" | cut -d: -f2)
                v=$(echo "$line" | cut -d: -f3)
                echo "$c $v" >> /tmp/check-txn-side-effects-$$
                ;;
            *)
                echo "$line"
                ;;
        esac
    done
done

# ── Tally results ─────────────────────────────────────────────────────

if [ -f /tmp/check-txn-side-effects-$$ ]; then
    while read -r c v; do
        CHECKED=$((CHECKED + c))
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-txn-side-effects-$$
    rm -f /tmp/check-txn-side-effects-$$
fi

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Transactional side-effect lint passed: ${CHECKED} rollback-capable functions checked."
    echo "No irretractable side effects found inside rollback-capable loops."
    exit 0
else
    echo "Fix: Defer irretractable side effects (notifications, analytics) to a"
    echo "     post-loop success block. Collect the data inside the loop; emit"
    echo "     only after all steps succeed."
    echo "     Exempt with: // transactional-side-effect:ok — <reason>"
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} functions checked."
    exit 1
fi
