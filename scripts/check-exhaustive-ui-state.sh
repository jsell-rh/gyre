#!/usr/bin/env bash
# Architecture lint: detect Svelte derived/computed state functions with
# fallthrough defaults that silently misclassify unmapped values into
# the wrong navigation or identity category.
#
# When a Svelte component derives a display value (active sidebar item,
# active tab, selected section) from a state variable via an if/else
# chain, the derivation must explicitly handle all possible values of
# the source variable. A fallthrough default that returns a specific
# navigation/identity value (e.g., `return 'specs'`) can silently
# misclassify unmapped values into the wrong category.
#
# This script detects if/else chains where:
# - There are 2+ conditional returns with string values
# - The chain ends with a bare return (no condition) of a string value
# - The returned value is NOT a known CSS/styling class (which are
#   safe generic fallbacks like 'muted', 'neutral', 'info')
#
# CSS/styling fallbacks ('muted', 'neutral', 'info', 'warning', etc.)
# are excluded because returning a neutral visual style for unknown
# enum values is safe — the unknown value gets a generic appearance.
# Navigation/identity values ('specs', 'inbox', 'explorer') are
# dangerous because the unknown value gets classified as a specific
# item, causing the wrong item to be highlighted/selected.
#
# Exempt with: // exhaustive-state:ok — <reason>
#
# See: specs/reviews/task-082.md F1, F3, F4
#
# Run by pre-commit and CI.

set -euo pipefail

WEB_SRC="web/src"
VIOLATIONS=0

if [ ! -d "$WEB_SRC" ]; then
    echo "Skipping exhaustive UI state check: $WEB_SRC not found"
    exit 0
fi

echo "Checking for non-exhaustive UI state derivations in Svelte components..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Known safe CSS/styling fallback values — returning these as a default
# is intentionally generic (neutral visual treatment for unknown values).
# These are excluded from detection.
CSS_SAFE_VALUES="muted|neutral|info|warning|error|success|primary|secondary|default|idle|healthy|open|activity|destructive|outline|ghost|link|subtle"

# Find .svelte files (exclude node_modules, test files)
for file in $(find "$WEB_SRC" -type f -name '*.svelte' \
    ! -path '*/node_modules/*' \
    ! -path '*/__tests__/*' \
    ! -name '*.test.*' \
    ! -name '*.spec.*' \
    | sort); do
    [ -f "$file" ] || continue

    # Detect if/else chains that end with a bare `return '<string>'`
    # as a fallthrough default, where the returned string is NOT a
    # known CSS/styling class.
    #
    # Pattern: A sequence of `if (...) return '...'` lines followed by
    # a bare `return '...'` line (no if/else condition) — the fallthrough
    # maps all unhandled values to a specific category.
    awk -v file="$file" -v safe="$CSS_SAFE_VALUES" '
    /exhaustive-state:ok/ { next }

    # Track conditional returns with string values
    /if\s*\(.*\)\s*return\s+'"'"'[a-z_]+'"'"'/ {
        conditional_returns++
        last_conditional_line = NR
        next
    }

    # A bare return with a string literal (no if/else) after 2+ conditional returns
    # Must be within 5 lines of the last conditional return
    /^\s*return\s+'"'"'[a-z_]+'"'"'\s*;?\s*$/ {
        if (conditional_returns >= 2 && NR - last_conditional_line <= 5) {
            # Extract the returned value
            val = $0
            gsub(/.*return\s+'"'"'/, "", val)
            gsub(/'"'"'.*/, "", val)

            # Check if the value is in the safe CSS list
            n = split(safe, safe_arr, "|")
            is_safe = 0
            for (i = 1; i <= n; i++) {
                if (val == safe_arr[i]) {
                    is_safe = 1
                    break
                }
            }

            if (!is_safe) {
                printf "%s:%d: fallthrough default returns '"'"'%s'"'"' — maps all unhandled values to a specific category\n  %s\n", file, NR, val, $0
            }
        }
        conditional_returns = 0
    }

    # Reset counter at function/block boundaries
    /^\s*(function |const |let |}\s*$|\$derived)/ {
        if (!/return/) {
            conditional_returns = 0
        }
    }
    ' "$file" 2>/dev/null >> "$HITS_FILE"

done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(grep -c "^[^ ]" "$HITS_FILE" || true)
    echo ""
    echo "NON-EXHAUSTIVE UI STATE DERIVATIONS found:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  A state derivation function with a fallthrough default that returns"
    echo "  a specific navigation/identity value (e.g., return 'specs') silently"
    echo "  misclassifies any unmapped source values into that category. The"
    echo "  sidebar, active item, or visual state will be wrong for unhandled"
    echo "  values."
    echo ""
    echo "  Fix: Enumerate every possible value of the source state variable and"
    echo "  map each one explicitly. If a catch-all default is truly needed, use"
    echo "  a neutral/safe value — not a specific category that could be wrong."
    echo ""
    echo "  If genuinely intentional, add '// exhaustive-state:ok — <reason>'"
    echo "  on the same line as the default return."
    echo ""
    echo "  See: specs/reviews/task-082.md F1, F3, F4"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Exhaustive UI state check passed."
exit 0
