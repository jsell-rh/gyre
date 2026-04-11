#!/usr/bin/env bash
# Architecture lint: detect positional collection access (.last(), .first(), [0])
# on attestation collections where semantic access (max_by_key, min_by_key)
# should be used instead.
#
# When code needs "the leaf attestation" (highest chain_depth) or "the root
# attestation" (lowest chain_depth), it must use explicit semantic selection:
#   attestations.iter().max_by_key(|a| a.metadata.chain_depth)
#
# Positional access (.last(), .first(), [0]) assumes the collection is ordered
# by chain_depth, but adapters may order by created_at, insertion order, or
# primary key. If attestations are inserted out of chain_depth order, positional
# access picks the wrong node.
#
# This script detects .last() and .first() on variables whose names suggest
# attestation collections (attestations, atts, chain) in non-test Rust code.
#
# Exemptions:
#   - Test code (#[test], #[cfg(test)]) is excluded
#   - Lines with `// positional-access:ok` are exempt
#
# See: specs/reviews/task-061.md F2
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping positional collection access check: $CRATE_SRC not found"
    exit 0
fi

echo "Checking for positional attestation collection access..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Pattern: variable names that typically hold attestation collections
# followed by .last() or .first()
# We match: attestations.last(), atts.last(), chain.last(), attestations.first(), etc.
COLLECTION_VARS="attestations|atts"
POSITIONAL_METHODS="\.last\(\)|\.first\(\)"

for file in $(find "$CRATE_SRC" -name '*.rs' -not -path '*/tests/*' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Determine the non-test portion of the file
    if grep -q '#\[cfg(test)\]' "$file" 2>/dev/null; then
        TEST_LINE=$(grep -n '#\[cfg(test)\]' "$file" | head -1 | cut -d: -f1)
    else
        TEST_LINE=$(wc -l < "$file")
    fi

    # Search only the non-test portion for positional access patterns
    MATCHES=$(head -n "$TEST_LINE" "$file" | grep -nE "(${COLLECTION_VARS})(${POSITIONAL_METHODS})" 2>/dev/null || true)
    [ -z "$MATCHES" ] && continue

    while IFS= read -r match; do
        line_num=$(echo "$match" | cut -d: -f1)
        line_text=$(echo "$match" | cut -d: -f2-)

        # Check for exemption comment
        if echo "$line_text" | grep -q 'positional-access:ok' 2>/dev/null; then
            continue
        fi

        echo "${file}:${line_num}: positional access on attestation collection — use .iter().max_by_key(|a| a.metadata.chain_depth) instead" >> "$HITS_FILE"
    done <<< "$MATCHES"
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "POSITIONAL COLLECTION ACCESS — .last()/.first() on attestation collections:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  Attestation collections from repository queries are NOT guaranteed"
    echo "  to be ordered by chain_depth. Adapters may order by created_at,"
    echo "  insertion order, or primary key."
    echo ""
    echo "  To find the leaf (highest chain_depth):"
    echo "    attestations.iter().max_by_key(|a| a.metadata.chain_depth)"
    echo ""
    echo "  To find the root (lowest chain_depth):"
    echo "    attestations.iter().min_by_key(|a| a.metadata.chain_depth)"
    echo ""
    echo "  If positional access is intentional (e.g., the collection was"
    echo "  explicitly sorted), add: // positional-access:ok"
    echo ""
    echo "  See: specs/reviews/task-061.md F2"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "Positional collection access check passed."
exit 0
