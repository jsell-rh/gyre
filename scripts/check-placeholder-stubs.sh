#!/usr/bin/env bash
# Architecture lint: detect placeholder/stub values in production code.
#
# When an endpoint response or data structure contains a placeholder value
# (empty array, empty string, zero) with an adjacent comment indicating
# future work ("would be", "TODO", "will be", "loaded from"), it is an
# incomplete implementation — not a finished feature.
#
# The most common failure mode: an export/bundle endpoint returns
# `"trust_anchors": []` with a comment "trust anchors would be loaded from
# tenant config." The endpoint compiles, tests pass (because they don't
# check trust_anchors content), but offline consumers cannot verify anything
# because the trust anchors are missing.
#
# This script detects:
#   Check 1: Empty arrays/vecs with TODO-like comments in non-test Rust code
#   Check 2: Empty strings with TODO-like comments in non-test Rust code
#
# Exempt with: // placeholder:ok — <reason>
#
# See: specs/reviews/task-008.md F4 (empty trust_anchors in export endpoint)
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_SRC="crates/gyre-server/src"
FAIL=0
VIOLATIONS=0

if [ ! -d "$SERVER_SRC" ]; then
    echo "ERROR: Cannot find $SERVER_SRC"
    exit 1
fi

echo "Checking for placeholder/stub values in production code..."

# ── Check 1: Empty arrays/vecs with TODO-like comments ──────────────────
#
# Patterns:
#   "field": [],  // would be ...
#   field: vec![], // TODO: ...
#   field: Vec::new(), // will be loaded from ...

echo ""
echo "=== Check 1: Empty arrays with placeholder comments ==="

# Placeholder comment patterns (case-insensitive)
PLACEHOLDER_PAT='(would be|will be|TODO|FIXME|HACK|loaded from|to be|not yet|placeholder|stub|future)'

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    # Use awk to skip test modules
    awk -v file="$file" -v pat="$PLACEHOLDER_PAT" '
    /^\s*#\[cfg\(test\)\]/ { in_test = 1 }
    in_test { next }

    # Match empty array patterns with adjacent placeholder comments
    # Covers: [], vec![], Vec::new() — all with trailing // comment
    /(\[\]|vec!\[\]|Vec::new\(\))\s*,?\s*\/\// {
        if ($0 ~ pat && $0 !~ /placeholder:ok/) {
            printf "%s:%d: %s\n", file, NR, $0
        }
    }
    ' "$file"
done > /tmp/check-placeholder-$$

if [ -s /tmp/check-placeholder-$$ ]; then
    echo ""
    echo "PLACEHOLDER EMPTY ARRAYS found in production code:"
    while IFS= read -r line; do
        echo "  $line"
        VIOLATIONS=$((VIOLATIONS + 1))
    done < /tmp/check-placeholder-$$
    echo ""
    echo "  Empty arrays with TODO-like comments indicate incomplete implementations."
    echo "  Either populate the array from available data, or remove the placeholder"
    echo "  comment and document WHY the empty value is correct."
    echo ""
    echo "  Add '// placeholder:ok' on the line if genuinely intentional."
    echo ""
    FAIL=1
fi
rm -f /tmp/check-placeholder-$$

# ── Check 2: Empty strings with TODO-like comments ──────────────────────

echo ""
echo "=== Check 2: Empty strings with placeholder comments ==="

for file in $(find "$SERVER_SRC" -name '*.rs' -type f | sort); do
    [ -f "$file" ] || continue

    awk -v file="$file" -v pat="$PLACEHOLDER_PAT" '
    /^\s*#\[cfg\(test\)\]/ { in_test = 1 }
    in_test { next }

    # Match String::new() or "".to_string() or "" with placeholder comments
    /(String::new\(\)|""\.to_string\(\)|: ""\s*,)\s*\/\// {
        if ($0 ~ pat && $0 !~ /placeholder:ok/) {
            printf "%s:%d: %s\n", file, NR, $0
        }
    }
    ' "$file"
done > /tmp/check-placeholder-str-$$

if [ -s /tmp/check-placeholder-str-$$ ]; then
    echo ""
    echo "PLACEHOLDER EMPTY STRINGS found in production code:"
    while IFS= read -r line; do
        echo "  $line"
        VIOLATIONS=$((VIOLATIONS + 1))
    done < /tmp/check-placeholder-str-$$
    echo ""
    echo "  Empty strings with TODO-like comments indicate incomplete implementations."
    echo "  Either populate from available data, or remove the placeholder comment."
    echo ""
    echo "  Add '// placeholder:ok' on the line if genuinely intentional."
    echo ""
    FAIL=1
fi
rm -f /tmp/check-placeholder-str-$$

# ── Result ──────────────────────────────────────────────────────────────

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "Placeholder stub lint passed."
    exit 0
else
    echo "Fix: Replace placeholder values with real data from available sources."
    echo "     If a field requires data from a repository or service, load it."
    echo "     Empty arrays/strings with TODO comments are not finished implementations."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
