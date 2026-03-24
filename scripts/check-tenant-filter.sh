#!/usr/bin/env bash
# Architecture lint: verify Diesel READ queries filter by tenant_id.
#
# Scans adapter implementations for methods named list*, find*, get*, query*
# that build Diesel queries, and verifies they include a tenant_id filter.
#
# Write operations (create, save, update, delete) are excluded — tenant_id
# is in the VALUES clause for those, not in a WHERE filter.
#
# Run by pre-commit and CI.

set -euo pipefail

ADAPTER_DIRS=("crates/gyre-adapters/src/sqlite" "crates/gyre-adapters/src/pg")

scan_file() {
    local file="$1"
    local label="$2"

    awk '
    /^\s*(pub\s+)?(async\s+)?fn\s+/ {
        if (method != "" && is_read && has_diesel && !has_tenant) {
            printf "TENANT FILTER MISSING: %s::%s in %s:%d\n", label, method, file, start
            printf "  This read query does not filter by tenant_id.\n"
            printf "  Add .filter(<table>::tenant_id.eq(&self.tenant_id))\n"
            printf "  See: specs/system/hierarchy-enforcement.md §3\n\n"
            violations++
        }
        if (method != "" && is_read && has_diesel) checked++
        match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
        method = m[1]
        start = NR
        has_diesel = 0; has_tenant = 0
        is_read = (method ~ /^(list|find|get|query|search|count|load)/)
        if (method ~ /^test_/) is_read = 0
        next
    }
    method != "" {
        if ($0 ~ /\.(load|first|get_result|get_results)/) has_diesel = 1
        if ($0 ~ /tenant_id/) has_tenant = 1
    }
    END {
        if (method != "" && is_read && has_diesel && !has_tenant) {
            printf "TENANT FILTER MISSING: %s::%s in %s:%d\n", label, method, file, start
            printf "  This read query does not filter by tenant_id.\n"
            printf "  Add .filter(<table>::tenant_id.eq(&self.tenant_id))\n"
            printf "  See: specs/system/hierarchy-enforcement.md §3\n\n"
            violations++
        }
        if (method != "" && is_read && has_diesel) checked++
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' label="$label" file="$file" "$file"
}

TOTAL_CHECKED=0
TOTAL_VIOLATIONS=0

for dir in "${ADAPTER_DIRS[@]}"; do
    [ -d "$dir" ] || continue
    label=$(basename "$dir")

    for file in "$dir"/*.rs; do
        [ -f "$file" ] || continue
        bname=$(basename "$file")
        [ "$bname" = "mod.rs" ] || [ "$bname" = "schema.rs" ] && continue

        output=$(scan_file "$file" "$label")
        # Print violation lines (everything except SUMMARY)
        echo "$output" | grep -v "^SUMMARY:" || true
        # Parse summary
        summary=$(echo "$output" | grep "^SUMMARY:" | tail -1)
        if [ -n "$summary" ]; then
            checked=$(echo "$summary" | cut -d: -f2)
            violations=$(echo "$summary" | cut -d: -f3)
            TOTAL_CHECKED=$((TOTAL_CHECKED + checked))
            TOTAL_VIOLATIONS=$((TOTAL_VIOLATIONS + violations))
        fi
    done
done

if [ "$TOTAL_VIOLATIONS" -eq 0 ]; then
    echo "Tenant filter lint passed: ${TOTAL_CHECKED} read query methods checked. All filter by tenant_id."
    exit 0
else
    echo "Fix: Add .filter(<table>::tenant_id.eq(&self.tenant_id)) to each read query."
    echo "${TOTAL_VIOLATIONS} violation(s) found out of ${TOTAL_CHECKED} read query methods."
    exit 1
fi
