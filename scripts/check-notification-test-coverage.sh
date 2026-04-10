#!/usr/bin/env bash
# Architecture lint: detect production functions that create notifications via
# notify_*() where the corresponding test functions never query
# state.notifications to assert on the created notification.
#
# When a function calls notify_mr_merged(), notify_rich(), or
# state.notifications.create() to send a notification, the test for that
# function must query state.notifications (e.g., list_for_user, list_recent)
# AFTER the call and assert on the received notification. Otherwise the test
# proves "no panic" but not "correct notifications were sent."
#
# The established pattern (see merge_processor.rs rollback tests):
#   process_next(&state).await.unwrap();
#   let notifs = state.notifications.list_for_user(
#       &Id::new("agent-1"), Some(&Id::new("ws-1")),
#       None, None, None, 100, 0,
#   ).await.unwrap();
#   assert!(!notifs.is_empty(), "author should be notified");
#   assert_eq!(notifs[0].notification_type, NotificationType::MrMerged);
#
# This script finds production functions that call notify_*() and checks
# whether ANY test that calls the production function queries
# state.notifications. If no test does, the notification emission is untested.
#
# See: specs/reviews/task-027.md R2 F3
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "Skipping notification test coverage check: $SERVER_SRC not found"
    exit 0
fi

echo "Checking for untested notification emissions..."

# ── Strategy ────────────────────────────────────────────────────────
#
# 1. Find non-test functions that call notify_*() or
#    state.notifications.create().
# 2. For each such function, find test functions in the same file that
#    call the production function.
# 3. Check whether those test functions query state.notifications
#    (list_for_user, list_recent, list_by_workspace, etc.) and assert.
# 4. If any test calls the production function but never queries
#    state.notifications → the test does not verify notification emission.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/notification-test-coverage-exemptions.txt"

# Load exemptions (one test function name per line, # comments ok).
EXEMPTIONS=""
if [ -f "$EXEMPTIONS_FILE" ]; then
    EXEMPTIONS=$(grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' || true)
fi

is_exempt() {
    local test_name="$1"
    if [ -n "$EXEMPTIONS" ]; then
        echo "$EXEMPTIONS" | grep -qFx "$test_name" && return 0
    fi
    return 1
}

for file in $(grep -rl 'notify_\|notifications\.create' "$SERVER_SRC" --include='*.rs' 2>/dev/null || true); do
    # Extract production function names (non-test) that call notify_*.
    PROD_FNS=$(awk '
        /^\s*#\[cfg\(test\)\]/ { in_test_mod = 1; next }
        /^\s*mod\s+tests\s*\{/ { if (in_test_mod) { test_depth = 1; next } }
        in_test_mod && /\{/ { test_depth++ }
        in_test_mod && /\}/ { test_depth--; if (test_depth <= 0) in_test_mod = 0 }

        !in_test_mod && /^\s*(pub(\(crate\))?\s+)?(async\s+)?fn\s+/ {
            match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, arr)
            current_fn = arr[1]
            fn_depth = 0
            has_notify = 0
        }
        !in_test_mod && current_fn != "" && /\{/ { fn_depth++ }
        !in_test_mod && current_fn != "" && /\}/ {
            fn_depth--
            if (fn_depth <= 0) {
                if (has_notify) print current_fn
                current_fn = ""
            }
        }
        !in_test_mod && current_fn != "" && /notify_[a-z]|notifications\.create/ {
            if (!/\/\/.*notify_|\/\/.*notifications/) has_notify = 1
        }
    ' "$file" 2>/dev/null | sort -u)

    if [ -z "$PROD_FNS" ]; then
        continue
    fi

    # Now scan test functions that call any of these production functions
    # and check for notification query + assertion.
    for prod_fn in $PROD_FNS; do
        # Find test functions that call this production function.
        TEST_FNS=$(awk -v prod="$prod_fn" '
            /^\s*#\[tokio::test\]|^\s*#\[test\]/ { in_test = 1; test_line = NR; test_name = ""; next }
            in_test && /fn ([a-zA-Z_][a-zA-Z0-9_]*)/ {
                match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, arr)
                test_name = arr[1]
                calls_prod = 0
                has_notif_query = 0
                has_notif_exempt = 0
                test_depth = 0
            }
            in_test && /\{/ { test_depth++ }
            in_test && /\}/ {
                test_depth--
                if (test_depth <= 0 && test_name != "") {
                    if (calls_prod && !has_notif_query && !has_notif_exempt) {
                        print test_name
                    }
                    in_test = 0; test_name = ""
                }
            }
            in_test && test_name != "" {
                if ($0 ~ prod "\\(") calls_prod = 1
                # Detect notification query — handles multi-line method chains like:
                #   state\n  .notifications\n  .list_for_user(...)
                # Check for .notifications accessor OR query method names independently.
                if ($0 ~ /\.notifications[^(]|\.notifications$/) has_notif_query = 1
                if ($0 ~ /list_for_user|list_recent/) has_notif_query = 1
                if ($0 ~ /\/\/\s*notification-coverage:ok/) has_notif_exempt = 1
            }
        ' "$file" 2>/dev/null)

        if [ -n "$TEST_FNS" ]; then
            while IFS= read -r test_fn; do
                [ -z "$test_fn" ] && continue
                is_exempt "$test_fn" && continue

                echo ""
                echo "UNTESTED NOTIFICATION EMISSION: ${file}"
                echo "  Production function: ${prod_fn}() calls notify_*()"
                echo "  Test function: ${test_fn}() calls ${prod_fn}() but never queries"
                echo "  state.notifications — notification emission is not verified."
                echo ""
                echo "  Fix: Add notification query and assertion:"
                echo "    ${prod_fn}(&state, ...).await;"
                echo "    let notifs = state.notifications.list_for_user("
                echo "        &Id::new(\"user-id\"), Some(&Id::new(\"ws-id\")),"
                echo "        None, None, None, 100, 0,"
                echo "    ).await.unwrap();"
                echo "    assert!(!notifs.is_empty(), \"user should be notified\");"
                echo "    assert_eq!(notifs[0].notification_type, NotificationType::Expected);"
                echo ""
                echo "  Exempt with: // notification-coverage:ok on the ${prod_fn}() call line,"
                echo "  or add '${test_fn}' to scripts/notification-test-coverage-exemptions.txt"
                echo ""
                echo "  See: specs/reviews/task-027.md R2 F3"
                echo ""
                VIOLATIONS=$((VIOLATIONS + 1))
                FAIL=1
            done <<< "$TEST_FNS"
        fi
    done
done

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Notification test coverage check passed."
    exit 0
else
    echo "Fix: Query state.notifications after calling the production function,"
    echo "     then assert on the notification type, priority, and key fields."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
