#!/usr/bin/env bash
# Architecture lint: detect production functions that emit events via
# emit_event() or message_broadcast_tx.send() where the corresponding
# test functions never subscribe to or assert on the broadcast channel.
#
# When a function calls state.emit_event(...) to broadcast an event
# (e.g., cascade_test_triggered, cascade_test_failed), the test for that
# function must subscribe to message_broadcast_tx BEFORE the call and
# assert on the received message AFTER. Otherwise the test proves
# "no panic" but not "correct events were emitted."
#
# The established pattern (see constraint_check.rs):
#   let mut rx = state.message_broadcast_tx.subscribe();
#   function_under_test(&state, ...).await;
#   let msg = rx.try_recv().unwrap();
#   assert_eq!(msg.kind, MessageKind::SomeEvent);
#
# This script finds production functions that call emit_event() and
# checks whether ANY test in the same file subscribes to the broadcast
# channel. If no test subscribes, the event emission is untested.
#
# See: specs/reviews/task-022.md F1, F5
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "Skipping event emission coverage check: $SERVER_SRC not found"
    exit 0
fi

echo "Checking for untested event emissions..."

# ── Strategy ────────────────────────────────────────────────────────
#
# 1. Find non-test functions that call emit_event() or
#    message_broadcast_tx.send().
# 2. For each such function, find test functions in the same file that
#    call the production function.
# 3. Check whether those test functions subscribe to
#    message_broadcast_tx and assert on received events.
# 4. If any test calls the production function but never subscribes
#    to the broadcast channel → the test does not verify event emission.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
EXEMPTIONS_FILE="$SCRIPT_DIR/event-emission-coverage-exemptions.txt"

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

for file in $(grep -rl 'emit_event\|message_broadcast_tx\.\(try_\)\?send' "$SERVER_SRC" --include='*.rs' 2>/dev/null || true); do
    # Extract production function names (non-test) that call emit_event.
    PROD_FNS=$(awk '
        /^\s*#\[cfg\(test\)\]/ { in_test_mod = 1; next }
        /^\s*mod\s+tests\s*\{/ { if (in_test_mod) { test_depth = 1; next } }
        in_test_mod && /\{/ { test_depth++ }
        in_test_mod && /\}/ { test_depth--; if (test_depth <= 0) in_test_mod = 0 }

        !in_test_mod && /^\s*(pub(\(crate\))?\s+)?(async\s+)?fn\s+/ {
            match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, arr)
            current_fn = arr[1]
            fn_depth = 0
            has_emit = 0
        }
        !in_test_mod && current_fn != "" && /\{/ { fn_depth++ }
        !in_test_mod && current_fn != "" && /\}/ {
            fn_depth--
            if (fn_depth <= 0) {
                if (has_emit) print current_fn
                current_fn = ""
            }
        }
        !in_test_mod && current_fn != "" && /emit_event|message_broadcast_tx\.(try_)?send/ {
            if (!/\/\/.*emit_event|\/\/.*broadcast/) has_emit = 1
        }
    ' "$file" 2>/dev/null | sort -u)

    if [ -z "$PROD_FNS" ]; then
        continue
    fi

    # Now scan test functions that call any of these production functions
    # and check for broadcast channel subscription.
    for prod_fn in $PROD_FNS; do
        # Find test functions that call this production function.
        TEST_FNS=$(awk -v prod="$prod_fn" '
            /^\s*#\[tokio::test\]|^\s*#\[test\]/ { in_test = 1; test_line = NR; test_name = ""; next }
            in_test && /fn ([a-zA-Z_][a-zA-Z0-9_]*)/ {
                match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, arr)
                test_name = arr[1]
                calls_prod = 0
                has_subscribe = 0
                has_event_exempt = 0
                test_depth = 0
            }
            in_test && /\{/ { test_depth++ }
            in_test && /\}/ {
                test_depth--
                if (test_depth <= 0 && test_name != "") {
                    if (calls_prod && !has_subscribe && !has_event_exempt) {
                        print test_name
                    }
                    in_test = 0; test_name = ""
                }
            }
            in_test && test_name != "" {
                if ($0 ~ prod "\\(") calls_prod = 1
                if ($0 ~ /message_broadcast_tx\.subscribe|broadcast_tx\.subscribe/) has_subscribe = 1
                if ($0 ~ /\/\/\s*event-emission:ok/) has_event_exempt = 1
            }
        ' "$file" 2>/dev/null)

        if [ -n "$TEST_FNS" ]; then
            while IFS= read -r test_fn; do
                [ -z "$test_fn" ] && continue
                is_exempt "$test_fn" && continue

                echo ""
                echo "UNTESTED EVENT EMISSION: ${file}"
                echo "  Production function: ${prod_fn}() calls emit_event()"
                echo "  Test function: ${test_fn}() calls ${prod_fn}() but never subscribes"
                echo "  to message_broadcast_tx — event emission is not verified."
                echo ""
                echo "  Fix: Add broadcast channel subscription and assertion:"
                echo "    let mut rx = state.message_broadcast_tx.subscribe();"
                echo "    ${prod_fn}(&state, ...).await;"
                echo "    let msg = rx.try_recv().unwrap();"
                echo "    assert_eq!(msg.kind, MessageKind::ExpectedKind);"
                echo ""
                echo "  Exempt with: // event-emission:ok on the ${prod_fn}() call line,"
                echo "  or add '${test_fn}' to scripts/event-emission-coverage-exemptions.txt"
                echo ""
                echo "  See: specs/reviews/task-022.md F1"
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
    echo "Event emission coverage check passed."
    exit 0
else
    echo "Fix: Subscribe to message_broadcast_tx before calling the production"
    echo "     function, then assert on the received event after the call."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
