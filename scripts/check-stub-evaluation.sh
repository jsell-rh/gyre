#!/usr/bin/env bash
# Architecture lint: detect client-side stub implementations of operations
# that require server-side evaluation.
#
# When a spec requires "evaluate constraints against repo state" or "validate
# CEL expression," the implementation must call a server endpoint.  A client-
# side handler that performs string heuristics (checking for '.', '==', etc.)
# instead of calling the server is a stub — it produces false positives and
# false negatives.
#
# This script detects known stub patterns in Svelte components:
#   1. Functions named dryRun/dry_run/validate that don't make fetch/API calls
#   2. Expression "validation" via string includes/regex instead of server eval
#
# See: specs/reviews/task-007.md F5
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
FAIL=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping stub evaluation check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for client-side stub evaluations..."

# ── Check 1: dryRun / dry_run / validate functions without fetch ──────
# Find Svelte/TS files that define a dryRun-like function.  If the function
# body doesn't contain 'fetch(' or 'api.' or '$api' or 'invoke(' (common
# patterns for server calls), it's likely a stub.

# Extract function blocks that look like evaluation handlers
STUB_FILES=""

for file in $(grep -rl 'dryRun\|dry_run\|dryrun' "$WEB_SRC" --include='*.svelte' --include='*.ts' 2>/dev/null || true); do
    # Check if the file has a function named dryRun (or similar) that
    # does NOT contain a fetch/API call
    if grep -qE 'async\s+function\s+dryRun|function\s+dryRun|const\s+dryRun|let\s+dryRun' "$file" 2>/dev/null; then
        if ! grep -qE 'fetch\(|\.post\(|\.get\(|\.put\(|\$api|api\.|invoke\(' "$file" 2>/dev/null; then
            STUB_FILES="$STUB_FILES  $file\n"
        fi
    fi
done

if [ -n "$STUB_FILES" ]; then
    echo ""
    echo "CLIENT-SIDE STUB EVALUATION found:"
    echo -e "$STUB_FILES"
    echo "  These files define dryRun/validate functions but make no server API calls."
    echo "  If the spec says 'evaluate against repo state' or 'validate expression,'"
    echo "  the handler must call a server endpoint — client-side string checks are"
    echo "  not equivalent to server-side CEL/constraint evaluation."
    echo ""
    echo "  See: specs/reviews/task-007.md F5 (stub dry-run)"
    echo ""
    FAIL=1
fi

# ── Check 2: String heuristic validation of CEL expressions ──────────
# Detect patterns like `expression.includes('.')` or `expression.includes('==')`
# which are string-level heuristics, not real CEL parsing/evaluation.

HEURISTIC_HITS=$(grep -rn "\.includes('\\.')\|\.includes('==')\|\.includes('>=')\|\.includes('<=')\|\.includes('!=')" "$WEB_SRC" \
    --include='*.svelte' --include='*.ts' \
    | grep -i 'expression\|constraint\|cel\|valid' \
    | grep -v '// stub-check:ok' \
    || true)

if [ -n "$HEURISTIC_HITS" ]; then
    echo ""
    echo "STRING HEURISTIC VALIDATION of expressions found:"
    echo "$HEURISTIC_HITS" | while IFS= read -r line; do
        echo "  $line"
    done
    echo ""
    echo "  Checking if a string contains '.' or '==' is not CEL validation."
    echo "  Valid CEL like 'output.changed_files.size() < 50' may be flagged as invalid,"
    echo "  while invalid syntax like 'foo == bar == baz' passes.  Use the server's"
    echo "  CEL evaluator for real validation."
    echo ""
    echo "  Add '// stub-check:ok' if this is genuinely not a validation stub."
    echo ""
    FAIL=1
fi

# ── Result ──────────────────────────────────────────────────────────────

if [ "$FAIL" -eq 0 ]; then
    echo "Stub evaluation check passed."
    exit 0
else
    echo "Fix: Replace client-side heuristics with server-side API calls."
    echo "     Add a server endpoint that uses the domain's evaluator if one doesn't exist."
    exit 1
fi
