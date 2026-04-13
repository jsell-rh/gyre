#!/usr/bin/env bash
# Architecture lint: detect no-op callback stubs in Svelte and JS/TS files.
#
# When a component wires up an event handler callback with a body
# containing only comments (no executable code), the callback fires but
# nothing happens. The comment describes the INTENDED behavior that was
# never implemented. The component appears to implement the feature —
# the prop is passed, the child component calls the callback — but no
# observable effect occurs. Tests that only verify "callback was invoked"
# pass, masking the fact that the callback is a stub.
#
# This script specifically targets arrow functions with COMMENT-ONLY
# bodies — not truly empty () => {} bodies, which are legitimate for
# prop defaults, .catch() handlers, and context fallbacks.
#
# Detection pattern:
#   Arrow function with body containing only comments:
#     (x) => { /* highlight span on canvas */ }
#     (span) => { // TODO: set reactive variable }
#
# Exempt with: // noop-callback:ok — <reason>
#
# See: specs/reviews/task-042.md F1 (onSpanSelect comment-only stub)
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
VIOLATIONS=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping noop callback check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for no-op callback stubs in frontend code..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Find .svelte, .js, .ts files (exclude node_modules, test files)
for file in $(find "$WEB_SRC" -type f \( -name '*.svelte' -o -name '*.js' -o -name '*.ts' -o -name '*.jsx' -o -name '*.tsx' \) \
    ! -path '*/node_modules/*' \
    ! -path '*/__tests__/*' \
    ! -name '*.test.*' \
    ! -name '*.spec.*' \
    | sort); do
    [ -f "$file" ] || continue

    # Detect arrow functions with comment-only bodies on a single line.
    # Pattern: => { /* ... */ }  or  => { // ... }
    # where after removing all comments, the body is empty.
    #
    # This does NOT flag:
    #   - Truly empty bodies: () => {}  (legitimate defaults)
    #   - Bodies with actual code: (x) => { doSomething(); }
    #   - .catch(() => {})  (error swallowing — different concern)
    awk -v file="$file" '
    # Skip lines with exemption
    /noop-callback:ok/ { next }

    # Only match lines containing => { ... } with a comment inside the braces
    /=>\s*\{[^}]*(\/\*|\/\/)[^}]*\}/ {
        # Extract the body between the first { after => and the matching }
        body = $0
        # Remove everything up to and including => {
        sub(/.*=>\s*\{/, "", body)
        # Remove first } and everything after (captures the function body end)
        sub(/\}.*/, "", body)

        # Save original body for comment detection
        original_body = body

        # Remove block comments /* ... */
        gsub(/\/\*[^*]*\*+([^/*][^*]*\*+)*\//, "", body)
        # Remove line comments // ... (to end of body, since body is extracted)
        gsub(/\/\/.*/, "", body)
        # Remove whitespace
        gsub(/[[:space:]]/, "", body)

        # If body is now empty, it was comment-only (had comments but no code)
        if (body == "" && original_body ~ /(\/\*|\/\/)/) {
            printf "%s:%d: arrow function with comment-only body (no-op stub)\n  %s\n", file, NR, $0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(grep -c "^[^ ]" "$HITS_FILE" || true)
    echo ""
    echo "NO-OP CALLBACK STUBS found in frontend code:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  An arrow function with a comment-only body is a no-op stub."
    echo "  The comment describes the intended behavior, but no code implements"
    echo "  it. The callback fires but produces no observable effect. Tests that"
    echo "  verify 'callback was invoked' pass, masking the missing implementation."
    echo ""
    echo "  Fix: Implement the callback body to produce an observable side effect"
    echo "  (set a reactive variable, dispatch an event, update state)."
    echo "  Remove the comment that describes the intended behavior — replace it"
    echo "  with actual code."
    echo ""
    echo "  If genuinely intentional, add '// noop-callback:ok — <reason>'"
    echo "  on the same line."
    echo ""
    echo "  See: specs/reviews/task-042.md F1"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "No-op callback check passed."
exit 0
