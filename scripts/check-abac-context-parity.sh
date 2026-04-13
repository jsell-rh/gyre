#!/usr/bin/env bash
# Architecture lint: detect ABAC AttributeContext construction sites with
# inconsistent baseline attribute sets.
#
# When ABAC evaluation occurs at multiple call sites (push flow, merge flow,
# dry-run endpoint), every site must set the same baseline subject attributes.
# The canonical baseline is defined by the ABAC middleware (abac_middleware.rs):
#   - subject.type
#   - subject.tenant_id
#
# This script finds all AttributeContext construction sites (identified by
# `AttributeContext::default()` followed by `ctx.set(` calls) and verifies
# each site sets the required baseline attributes. A site that sets
# `subject.type` but not `subject.tenant_id` is an asymmetric context that
# will cause multi-tenant policies to silently fail.
#
# Exemptions:
#   - Test code (#[test], #[cfg(test)]) is excluded
#   - Lines with `// abac-context:ok` are exempt
#   - The dry-run endpoint (policies.rs) is exempt because it accepts
#     tenant_id from the request body rather than from auth context
#
# See: specs/reviews/task-061.md F1
#
# Run by pre-commit and CI.

set -euo pipefail

CRATE_SRC="crates"
VIOLATIONS=0

if [ ! -d "$CRATE_SRC" ]; then
    echo "Skipping ABAC context parity check: $CRATE_SRC not found"
    exit 0
fi

echo "Checking ABAC AttributeContext construction parity..."

HITS_FILE=$(mktemp)
trap 'rm -f "$HITS_FILE"' EXIT

# Required baseline attributes that every non-exempt ABAC context must set.
REQUIRED_ATTRS=("subject.type" "subject.tenant_id")

# Files to exempt (dry-run accepts tenant_id from request body, not auth)
EXEMPT_FILES="policies.rs"

for file in $(find "$CRATE_SRC" -name '*.rs' -not -path '*/tests/*' -print 2>/dev/null | sort); do
    [ -f "$file" ] || continue

    # Skip test modules
    if grep -q '#\[cfg(test)\]' "$file" 2>/dev/null; then
        # Only scan the non-test portion (before #[cfg(test)])
        TEST_LINE=$(grep -n '#\[cfg(test)\]' "$file" | head -1 | cut -d: -f1)
        SCAN_CONTENT=$(head -n "${TEST_LINE}" "$file")
    else
        SCAN_CONTENT=$(cat "$file")
    fi

    # Skip exempt files
    basename_file=$(basename "$file")
    if echo "$EXEMPT_FILES" | grep -qw "$basename_file"; then
        continue
    fi

    # Find AttributeContext::default() construction sites
    # Extract line numbers where AttributeContext is constructed
    CONTEXT_LINES=$(echo "$SCAN_CONTENT" | grep -n 'AttributeContext::default()' 2>/dev/null | cut -d: -f1 || true)
    [ -z "$CONTEXT_LINES" ] && continue

    for ctx_line in $CONTEXT_LINES; do
        # Check for exemption comment on the construction line
        CTX_LINE_TEXT=$(echo "$SCAN_CONTENT" | sed -n "${ctx_line}p")
        if echo "$CTX_LINE_TEXT" | grep -q 'abac-context:ok' 2>/dev/null; then
            continue
        fi

        # Look ahead up to 30 lines for ctx.set() calls to find what attributes are set
        END_LINE=$((ctx_line + 30))
        TOTAL_LINES=$(echo "$SCAN_CONTENT" | wc -l)
        [ "$END_LINE" -gt "$TOTAL_LINES" ] && END_LINE=$TOTAL_LINES

        BLOCK=$(echo "$SCAN_CONTENT" | sed -n "${ctx_line},${END_LINE}p")

        for attr in "${REQUIRED_ATTRS[@]}"; do
            if ! echo "$BLOCK" | grep -q "\"${attr}\"" 2>/dev/null; then
                echo "${file}:${ctx_line}: AttributeContext missing \"${attr}\" — set in other ABAC contexts but not here" >> "$HITS_FILE"
            fi
        done
    done
done

if [ -s "$HITS_FILE" ]; then
    VIOLATIONS=$(wc -l < "$HITS_FILE")
    echo ""
    echo "ABAC CONTEXT PARITY — AttributeContext sites missing baseline attributes:"
    echo ""
    while IFS= read -r line; do
        echo "  $line"
    done < "$HITS_FILE"
    echo ""
    echo "  Every ABAC AttributeContext construction site must set the same"
    echo "  baseline attributes as the canonical ABAC middleware. Missing"
    echo "  subject.tenant_id causes multi-tenant policies to silently fail"
    echo "  for that evaluation path while working for others."
    echo ""
    echo "  Fix: Add the missing ctx.set(\"<attr>\", ...) call, sourcing"
    echo "  the value from the available auth/repo/tenant context."
    echo ""
    echo "  If the omission is intentional, add: // abac-context:ok"
    echo ""
    echo "  See: specs/reviews/task-061.md F1"
    echo ""
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi

echo "ABAC context parity check passed."
exit 0
