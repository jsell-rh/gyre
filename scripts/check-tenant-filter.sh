#!/usr/bin/env bash
# Architecture lint: verify Diesel READ queries filter by tenant_id.
#
# Spec: specs/system/hierarchy-enforcement.md §3 (Consistent Tenant Filtering).
# "Every query method on every Diesel adapter must filter by tenant_id" —
# including find_by_id(), so a leaked UUID cannot cross tenants.
#
# The scan: for each fn whose name marks a read (list*/find*/get*/query*/
# search*/count*/load*) that builds a Diesel query (a terminal .load/.first/
# .get_result/... call in its body), require the pattern `tenant_id.eq(`
# somewhere in the method body. Write operations (create/update/delete) are
# out of scope — tenant_id rides in the VALUES clause there, not a WHERE.
#
# Exemptions (spec §3): adapters that enforce tenant isolation *structurally*
# may be skipped with a documented rationale — see SKIP_LIST below. These are
# tables with no tenant_id column whose isolation comes from querying by a
# globally-unique, tenant-bound key (workspace_id) or by the tenant identity
# itself, plus intentional cross-tenant housekeeping.
#
# Run by pre-commit and CI (blocking).
#
# NOTE: the scan is deliberately plain awk (POSIX) so it runs under mawk as
# well as gawk — the previous revision used gawk-only match(s,re,arr) and \s
# and silently died with a syntax error on mawk systems. The directory list
# must name the REAL adapter dirs: the Postgres adapters live in
# crates/gyre-adapters/src/postgres (a stale "src/pg" entry made the scan
# skip every Postgres adapter, spec §3).

set -euo pipefail

ADAPTER_DIRS=("crates/gyre-adapters/src/sqlite" "crates/gyre-adapters/src/postgres")

# ── Skip list (spec §3 structural-isolation exemptions) ─────────────────
# Each entry: "<basename>|<rationale>". Only files whose isolation is
# structural qualify; a table that HAS a tenant_id column must not be skipped.
SKIP_LIST=(
    "message.rs|messages are queried by globally-unique workspace_id (tenant-bound); per-message expiry methods are intentionally cross-tenant housekeeping."
    "user_workspace_state.rs|table keyed by (workspace_id, user); workspace_id is globally unique so isolation is structural. No tenant_id column, no UUID-guessing surface (no REST endpoint)."
    "workspace_membership.rs|workspace_memberships has no tenant_id column; rows are addressed only via globally-unique workspace_id, which is tenant-bound."
    "tenant.rs|the tenants table IS the tenant registry: find_by_id/find_by_slug resolve tenant identity itself (used by the OIDC/SCIM resolvers before tenant context exists), so a self-tenant filter is impossible by construction."
    "kv_store.rs|generic namespace+key store: no tenant_id column. KvJsonStore namespaces are a documented backlog item (spec §3 'KvJsonStore gap'), not Diesel-table adapters."
)

is_skipped() {
    local bname="$1"
    local entry
    for entry in "${SKIP_LIST[@]}"; do
        [ "${entry%%|*}" = "$bname" ] && return 0
    done
    return 1
}

# ── Scanner (POSIX awk) ─────────────────────────────────────────────────
# State machine over fn boundaries: a method is "checked" when its body
# contains a Diesel terminal op; it "passes" when the body also contains
# tenant_id.eq(. Lines inside #[cfg(test)] mod tests are skipped.
scan_file() {
    local file="$1"
    local label="$2"

    awk -v label="$label" -v file="$file" '
    /^[[:space:]]*(pub(\([^\)]*\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[a-zA-Z_]/ {
        flush()
        name = ""
        if (match($0, /fn[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*/)) {
            name = substr($0, RSTART, RLENGTH)
            sub(/fn[[:space:]]+/, "", name)
        }
        method = name
        start = NR
        has_diesel = 0; has_tenant = 0
        is_read = (method ~ /^(list|find|get|query|search|count|load)/)
        if (method ~ /^test_/ || intests) is_read = 0
        next
    }
    # #[cfg(test)] mod tests: adapter tests intentionally query cross-tenant
    # to prove isolation; do not lint them.
    /^[[:space:]]*(pub(\([^\)]*\))?[[:space:]]+)?mod[[:space:]]+tests[[:space:]]*\{/ { intests = 1 }
    method != "" {
        if ($0 ~ /\.(load|load_one|load_all|first|get_result|get_results)[^a-z_]/) has_diesel = 1
        if ($0 ~ /tenant_id[[:space:]]*[.][[:space:]]*eq[[:space:]]*\(/) has_tenant = 1
    }
    function flush() {
        if (method != "" && is_read && has_diesel && !has_tenant) {
            printf "TENANT FILTER MISSING: %s::%s in %s:%d\n", label, method, file, start
            printf "  This read query builds a Diesel query without tenant_id.eq().\n"
            printf "  Add .filter(<table>::tenant_id.eq(&self.tenant_id)) or justify a\n"
            printf "  structural-isolation skip in this script'\''s SKIP_LIST (spec §3).\n\n"
            violations++
        }
        if (method != "" && is_read && has_diesel) checked++
    }
    END {
        flush()
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file"
}

TOTAL_CHECKED=0
TOTAL_VIOLATIONS=0

for dir in "${ADAPTER_DIRS[@]}"; do
    if [ ! -d "$dir" ]; then
        echo "TENANT FILTER LINT ERROR: adapter dir '$dir' does not exist." >&2
        echo "  The scan silently skipped an entire backend before; it must name real dirs." >&2
        exit 2
    fi
    label=$(basename "$dir")

    for file in "$dir"/*.rs; do
        [ -f "$file" ] || continue
        bname=$(basename "$file")
        [ "$bname" = "mod.rs" ] && continue
        [ "$bname" = "schema.rs" ] && continue
        if is_skipped "$bname"; then
            continue
        fi

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

if [ "$TOTAL_CHECKED" -eq 0 ]; then
    echo "TENANT FILTER LINT ERROR: 0 read query methods checked — scanner wired wrong." >&2
    exit 2
fi

if [ "$TOTAL_VIOLATIONS" -eq 0 ]; then
    echo "Tenant filter lint passed: ${TOTAL_CHECKED} read query methods checked. All filter by tenant_id."
    exit 0
else
    echo "Fix: Add .filter(<table>::tenant_id.eq(&self.tenant_id)) to each read query."
    echo "${TOTAL_VIOLATIONS} violation(s) found out of ${TOTAL_CHECKED} read query methods."
    exit 1
fi
