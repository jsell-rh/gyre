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
# (or raw-SQL `tenant_id = ?/$n`) somewhere in the method body whenever the
# body touches a table that HAS a tenant_id column. Which tables have the
# column is not hand-maintained: it is derived from the migrations
# themselves (CREATE TABLE blocks, `ADD COLUMN tenant_id` ALTERs, minus
# DROP TABLEs) — the migrations are the ground truth for what the column
# physically exists on. A query method on a table WITHOUT the column cannot
# carry a filter; such tables are reported as a non-failing backlog (they
# need either a tenant_id migration or a structural-isolation skip entry,
# spec §3) so the gap stays visible instead of silently silencing the scan.
# Write operations (create/update/delete) are out of scope — tenant_id rides
# in the VALUES clause there, not a WHERE.
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
MIGRATIONS_DIR="crates/gyre-adapters/migrations"

if [ ! -d "$MIGRATIONS_DIR" ]; then
    echo "TENANT FILTER LINT ERROR: migrations dir '$MIGRATIONS_DIR' not found (run from repo root)." >&2
    exit 2
fi

# ── Derive tenant-column tables from the migrations (ground truth) ──────
# Outputs "<tenant tables>|<all tables>" (pipe-joined). Tracks:
#   CREATE TABLE [IF [NOT] EXISTS] name           (block until `)`)
#   ALTER TABLE name ADD COLUMN [IF NOT EXISTS] tenant_id
#   DROP TABLE name                               (removes a later-deleted table)
#   ALTER TABLE x RENAME TO y                     (re-creates y from x's columns)
# SQLite table-recreation migrations use `<name>_new` temp tables that are
# renamed to the real name. The rename is load-bearing: without it, the
# preceding `DROP TABLE name` deletes the table from the set and every read
# method on it silently falls into the backlog bucket instead of being
# checked — exactly the class of silent scan-shrink this lint exists to
# prevent (found live: workspaces/tasks/agents/merge_requests/repositories/
# saved_views were ALL unscanned). `_new` names are normalized via base().
DERIVED=$(awk '
function base(n) { sub(/_new$/, "", n); return n }
function token_after_table(   i, nm) {
    nm = ""
    for (i = 1; i <= 10; i++) {
        if (c[i] == "TABLE") {
            nm = (c[i + 1] == "IF") ? ((c[i + 2] == "NOT") ? c[i + 4] : c[i + 3]) : c[i + 1]
            break
        }
    }
    gsub(/[^A-Za-z0-9_]/, "", nm)
    return nm
}
# SQL comments (`-- ...`) are not statements: skip them so a prose mention of
# CREATE TABLE inside a comment cannot register a phantom table (the word
# after "EXISTS" in a comment line was being picked up as a table name).
/^[ \t]*--/ { next }
/CREATE TABLE/ {
    intbl = 1
    split($0, c, " ")
    cur = token_after_table()
    if (cur != "") all[cur] = 1
    next
}
intbl && /^[ \t]*["]?tenant_id["]?[ \t]/ && cur != "" { tt[cur] = 1 }
intbl && /^[ \t]*\)/ { intbl = 0; cur = "" }
/^[ \t]*ALTER TABLE/ && /ADD COLUMN/ && /tenant_id/ {
    split($0, c, " ")
    nm = token_after_table()
    if (nm != "") { tt[nm] = 1; all[nm] = 1 }
}
# ALTER TABLE <src> RENAME TO <dst>: dst takes over the column set of src.
# src is the token immediately before RENAME (robust to `ALTER TABLE [IF
# EXISTS] x RENAME TO y`); both the raw key (workspaces_new) and its base
# form are consumed from tt/all so a later real DROP of the recreated table
# cannot be resurrected by a leftover _new key at END-normalization time.
# Runs after the ADD COLUMN rule so a recreation RENAME is the final word
# on table columns, matching the physical migration lifecycle.
/^[ \t]*ALTER TABLE/ && /RENAME TO/ {
    split($0, c, " ")
    src = ""; dst = ""
    for (i = 1; i <= 12; i++) {
        if (c[i] == "RENAME") { src = c[i - 1]; if (c[i + 1] == "TO") dst = c[i + 2]; break }
    }
    gsub(/[^A-Za-z0-9_]/, "", src); gsub(/[^A-Za-z0-9_]/, "", dst)
    if (src == "" || dst == "") next
    had_t = (src in tt) || (base(src) in tt)
    had_a = (src in all) || (base(src) in all)
    delete tt[src]; delete tt[base(src)]; delete tt[dst]
    delete all[src]; delete all[base(src)]; delete all[dst]
    if (had_t) tt[dst] = 1
    if (had_a) all[dst] = 1
}
/DROP TABLE/ {
    split($0, c, " ")
    nm = token_after_table()
    if (nm != "") { delete tt[nm]; delete all[nm] }
}
END {
    for (x in tt)  tf[base(x)] = 1
    for (x in all) af[base(x)] = 1
    t = ""; a = ""
    for (x in tf) t = (t == "" ? x : t "|" x)
    for (x in af) a = (a == "" ? x : a "|" x)
    printf "%s\n%s\n", t, a
}
' "$MIGRATIONS_DIR"/*/up.sql)

{ read -r TENANT_TABLES; read -r ALL_TABLES; } <<< "$DERIVED"

if [ -z "$TENANT_TABLES" ]; then
    echo "TENANT FILTER LINT ERROR: derived 0 tenant-column tables from migrations — derivation broken." >&2
    exit 2
fi

# Secondary tables: everything the migrations created that has no tenant_id
# column. Read methods touching these are reported as backlog, not violations.
ALL_TBL_F=$(mktemp)
TEN_TBL_F=$(mktemp)
trap 'rm -f "$ALL_TBL_F" "$TEN_TBL_F"' EXIT
printf '%s\n' "$ALL_TABLES" | tr '|' '\n' | sort -u > "$ALL_TBL_F"
printf '%s\n' "$TENANT_TABLES" | tr '|' '\n' | sort -u > "$TEN_TBL_F"
NTT=$(comm -23 "$ALL_TBL_F" "$TEN_TBL_F" | grep -v '^_new$' | tr '\n' ' ')
rm -f "$ALL_TBL_F" "$TEN_TBL_F"

# ── Skip list (spec §3 structural-isolation exemptions) ─────────────────
# Each entry: "<basename>|<rationale>". Spec §3 names the exemption class:
# adapters enforcing tenant isolation structurally (e.g. MessageRepository
# querying by globally-unique workspace_id, which is tenant-bound). The
# messages table itself has a tenant_id column; it is skipped because its
# inbox/workspace queries are addressed by tenant-bound keys (workspace_id,
# to_id) rather than a tenant_id predicate, per the spec's own example.
# A skip entry is a per-file, spec-cited justification — not a way around
# a missing filter on an arbitrarily-addressed read.
SKIP_LIST=(
    "message.rs|messages are queried by globally-unique workspace_id (tenant-bound) or per-agent inbox key (to_id, tenant-bound agent); per-message expiry methods are intentionally cross-tenant housekeeping. Spec §3 names MessageRepository as the structural-isolation example."
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
# contains a Diesel terminal op AND references at least one table that has a
# tenant_id column; it "passes" when the body also contains tenant_id.eq(
# (or raw-SQL tenant_id = ?/$n). References to no-column tables are emitted
# as BACKLOG lines (informational). Lines inside #[cfg(test)] mod tests are
# skipped.
scan_file() {
    local file="$1"
    local label="$2"

    awk -v label="$label" -v file="$file" -v ttlist="$TENANT_TABLES" -v nttlist="$NTT" '
    BEGIN {
        nt = split(ttlist,  ttn, "|")
        nn = split(nttlist, ntn, " ")
    }
    # Does this line reference table NAME (diesel path `name::col` or raw SQL
    # FROM/INTO/UPDATE/JOIN name)? Dynamic regexes; names are [a-z_]+ so no
    # escaping needed.
    function refs(name) {
        return ($0 ~ ("(^|[^A-Za-z0-9_])" name "::")) \
            || ($0 ~ ("(FROM|INTO|UPDATE|JOIN)[ \t]+" name "[^A-Za-z0-9_]"))
    }
    /^[[:space:]]*(pub(\([^\)]*\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[a-zA-Z_]/ {
        flush()
        name = ""
        if (match($0, /fn[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*/)) {
            name = substr($0, RSTART, RLENGTH)
            sub(/fn[[:space:]]+/, "", name)
        }
        method = name
        start = NR
        has_diesel = 0; has_tenant = 0; tt_ref = 0
        delete ntt_ref
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
        if ($0 ~ /tenant_id[[:space:]]*=[[:space:]]*[?$]/) has_tenant = 1
        if (is_read) {
            for (i = 1; i <= nt; i++)  if (!tt_ref && refs(ttn[i]))  tt_ref = 1
            for (j = 1; j <= nn; j++)  if (refs(ntn[j]))             ntt_ref[ntn[j]] = 1
        }
    }
    function flush() {
        if (method != "" && is_read && has_diesel) {
            if (tt_ref) {
                checked++
                if (!has_tenant) {
                    printf "TENANT FILTER MISSING: %s::%s in %s:%d\n", label, method, file, start
                    printf "  This read query touches a tenant-column table without any\n"
                    printf "  tenant_id.eq()/tenant_id = ? filter in its body.\n"
                    printf "  Add .filter(<table>::tenant_id.eq(&self.tenant_id)) or justify a\n"
                    printf "  structural-isolation skip in this script'\''s SKIP_LIST (spec §3).\n\n"
                    violations++
                }
            } else {
                for (t in ntt_ref)
                    printf "BACKLOG:%s:%s::%s\n", t, label, method
            }
        }
    }
    END {
        flush()
        printf "SUMMARY:%d:%d\n", checked, violations
    }
    ' "$file"
}

TOTAL_CHECKED=0
TOTAL_VIOLATIONS=0
BACKLOG_TMP=$(mktemp)
trap 'rm -f "$BACKLOG_TMP"' EXIT

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
        # Print violation lines (everything except SUMMARY/BACKLOG)
        echo "$output" | grep -v -e "^SUMMARY:" -e "^BACKLOG:" || true
        echo "$output" | grep "^BACKLOG:" >> "$BACKLOG_TMP" || true
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

# ── Backlog report (non-failing): tables read by adapters with no tenant_id
# column. Each needs a tenant_id migration (then it moves into the checked
# set) or a documented structural-isolation SKIP_LIST entry.
if [ -s "$BACKLOG_TMP" ]; then
    echo "Tenant filter backlog (tables with no tenant_id column — spec §3 gap class):"
    cut -d: -f2 "$BACKLOG_TMP" | sort | uniq -c | sort -rn | while IFS= read -r line; do
        echo "  $line"
    done
    echo "  $(wc -l < "$BACKLOG_TMP" | tr -d ' ') read methods touch these tables; isolation is"
    echo "  structural (tenant-bound parent keys) or deferred. Adding a tenant_id column to a"
    echo "  table here automatically brings its read methods into the checked set."
    echo ""
fi

if [ "$TOTAL_VIOLATIONS" -eq 0 ]; then
    echo "Tenant filter lint passed: ${TOTAL_CHECKED} read query methods on tenant-column tables checked. All filter by tenant_id."
    exit 0
else
    echo "Fix: Add .filter(<table>::tenant_id.eq(&self.tenant_id)) to each read query."
    echo "${TOTAL_VIOLATIONS} violation(s) found out of ${TOTAL_CHECKED} read query methods."
    exit 1
fi
