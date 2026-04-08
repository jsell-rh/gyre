#!/usr/bin/env bash
# Architecture lint: detect silent result discarding on async operations.
#
# The pattern `let _ = port.create(...).await;` silently swallows errors from
# async operations.  When a handler creates side effects (notifications, MRs,
# events), silently discarding the result makes failures invisible — the
# handler returns success, tests pass, but the side effect never occurred.
#
# This script flags `let _ =` on `.await` calls in non-test handler code.
# Legitimate fire-and-forget patterns can be exempted with:
#   // discard-result:ok — <reason>
#
# See: specs/reviews/task-011.md F4
#
# Run by pre-commit and CI.

set -euo pipefail

RUST_SRC="crates"
FAIL=0

if [ ! -d "$RUST_SRC" ]; then
    echo "Skipping silent result discard check: $RUST_SRC not found"
    exit 0
fi

echo "Checking for silently discarded async results..."

# Find `let _ = ...await` patterns in non-test Rust files.
# This catches: let _ = something.await;  and  let _ = something.await.ok();
HITS=$(grep -rn 'let _ = .*\.await' "$RUST_SRC" \
    --include='*.rs' \
    | grep -v '/tests/\|_test\.rs\|#\[test\]\|// discard-result:ok' \
    || true)

if [ -n "$HITS" ]; then
    echo ""
    echo "SILENTLY DISCARDED ASYNC RESULTS found:"
    echo "$HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  The pattern 'let _ = port.operation(...).await' silently swallows errors."
    echo "  If the operation fails, the handler returns success and tests pass, but"
    echo "  the side effect (notification, MR, event) was never created."
    echo ""
    echo "  Fix options:"
    echo "    1. Handle the error:  .await.map_err(|e| tracing::warn!(...))?"
    echo "    2. Log on failure:    if let Err(e) = port.create(...).await { warn!(...) }"
    echo "    3. If intentional:    add '// discard-result:ok — <reason>' on the same line"
    echo ""
    echo "  See: specs/reviews/task-011.md F4 (silent notification creation failure)"
    echo ""
    FAIL=1
fi

if [ "$FAIL" -eq 0 ]; then
    echo "Silent result discard check passed."
    exit 0
else
    echo "Fix: Handle or log errors from async operations instead of discarding with 'let _ ='."
    exit 1
fi
