#!/usr/bin/env bash
# Message delivery parity lint: every handler that sends a message must push to
# BOTH message_dispatch_tx (MessageConsumer pipeline) AND message_broadcast_tx
# (WebSocket delivery).
#
# Rationale (from review TASK-001 F7): handle_message_send dispatched to
# message_dispatch_tx but not message_broadcast_tx, causing MCP-sent messages
# to never reach WebSocket clients. The same gap existed in the REST handler.
#
# Approach: find every Rust function body that calls .send() or .try_send() on
# message_dispatch_tx, and verify the same function also pushes to
# message_broadcast_tx. Annotate with `// delivery-parity: one-channel-only`
# on the send line to suppress (e.g., for intentionally one-channel paths).

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL_FILE=$(mktemp)
echo "0" > "$FAIL_FILE"

for file in $(find "$SERVER_SRC" -name '*.rs'); do
    OUTPUT=$(awk '
    /^[[:space:]]*(pub )?(async )?fn [a-zA-Z_]/ {
        if (func_name != "" && (has_dispatch || has_broadcast)) {
            if (has_dispatch && !has_broadcast && !dispatch_suppressed) {
                printf "DELIVERY PARITY: %s:%s fn %s sends to message_dispatch_tx but NOT message_broadcast_tx\n", FILENAME, func_start, func_name
                printf "  WebSocket clients will not receive messages from this code path.\n"
                printf "  Suppress with: // delivery-parity: one-channel-only\n\n"
                fail = 1
            }
        }
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, arr)
        func_name = arr[1]
        func_start = NR
        has_dispatch = 0
        has_broadcast = 0
        dispatch_suppressed = 0
        broadcast_suppressed = 0
    }

    /message_dispatch_tx\.(try_)?send/ {
        if (/delivery-parity: one-channel-only/) {
            dispatch_suppressed = 1
        } else {
            has_dispatch = 1
        }
    }

    /message_broadcast_tx\.(try_)?send/ && !/\.subscribe/ {
        if (/delivery-parity: one-channel-only/) {
            broadcast_suppressed = 1
        } else {
            has_broadcast = 1
        }
    }

    END {
        if (func_name != "" && has_dispatch && !has_broadcast && !dispatch_suppressed) {
            printf "DELIVERY PARITY: %s:%s fn %s sends to message_dispatch_tx but NOT message_broadcast_tx\n", FILENAME, func_start, func_name
            printf "  WebSocket clients will not receive messages from this code path.\n"
            printf "  Suppress with: // delivery-parity: one-channel-only\n\n"
            fail = 1
        }
        if (fail) exit 1
    }
    ' "$file" 2>&1) || true

    if [ -n "$OUTPUT" ]; then
        echo "$OUTPUT"
        if echo "$OUTPUT" | grep -q "^DELIVERY PARITY:"; then
            echo "1" > "$FAIL_FILE"
        fi
    fi
done

RESULT=$(cat "$FAIL_FILE")
rm -f "$FAIL_FILE"

if [ "$RESULT" = "1" ]; then
    exit 1
fi

echo "Message delivery parity check passed."
