#!/usr/bin/env bash
# Architecture lint: detect stale "(stubbed)" markers in doc comments.
#
# When a handler or function is fully implemented, its doc comments must not
# retain "(stubbed)" / "(stub)" markers from the stub era.  A stale marker
# misleads readers into thinking the function is not yet implemented.
#
# See: specs/reviews/task-011.md F3
#
# Run by pre-commit and CI.

set -euo pipefail

RUST_SRC="crates"
FAIL=0

if [ ! -d "$RUST_SRC" ]; then
    echo "Skipping stale stub marker check: $RUST_SRC not found"
    exit 0
fi

echo "Checking for stale stub markers in doc comments..."

# Find doc comments (//! or ///) containing "(stubbed)" or "(stub)" in non-test
# Rust files.  Markers that are genuinely still stubbed can be exempted with
# "// stub-marker:ok" on the same line.
HITS=$(grep -rn '//[!/].*([Ss]tubbed)\|//[!/].*([Ss]tub)' "$RUST_SRC" \
    --include='*.rs' \
    | grep -v '_test\.rs\|/tests/\|// stub-marker:ok' \
    || true)

if [ -n "$HITS" ]; then
    echo ""
    echo "STALE STUB MARKERS found in doc comments:"
    echo "$HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  Doc comments containing '(stubbed)' or '(stub)' indicate the function"
    echo "  was not yet implemented.  If the function is now fully implemented,"
    echo "  remove the stale marker.  If the function is genuinely still a stub,"
    echo "  add '// stub-marker:ok' on the same line to exempt it."
    echo ""
    echo "  See: specs/reviews/task-011.md F3 (stale stub marker after implementation)"
    echo ""
    FAIL=1
fi

if [ "$FAIL" -eq 0 ]; then
    echo "Stale stub marker check passed."
    exit 0
else
    echo "Fix: Remove '(stubbed)' markers from doc comments of implemented functions."
    exit 1
fi
