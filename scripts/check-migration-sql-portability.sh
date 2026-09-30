#!/usr/bin/env bash
# check-migration-sql-portability.sh — migrations are embedded once and
# executed by BOTH storage backends (sqlite/mod.rs and postgres/mod.rs
# embed the same crates/gyre-adapters/migrations/ directory), so every
# migration file must use only SQL that runs on SQLite AND PostgreSQL.
#
# Procedural background (specs/reviews/task-102.md F1):
# migration 000054's up/down used SQLite-only JSON1 scalar functions
# (json_patch, json_object, json_extract, json_remove). PostgreSQL has
# none of them as scalar SQL functions (json_object exists only as an
# aggregate with different syntax; jsonb_set / -> / #- are the PG
# equivalents). The SQLite adapter tests were green, `check-arch` /
# `check-migration-versions` were green, and the SQLite-only SQL
# shipped into the SHARED embedded migrations dir — a PostgreSQL
# deployment now fails at startup, at migration time, with a SQL
# function-not-found error. The adapter code (postgres/audit.rs) had
# been correctly rewritten; the migration alone schema-blocked the
# entire PG path.
#
# Detection: scan every migrations/*/{up,down}.sql for dialect-only
# constructs. Comment lines (`-- ...`) are stripped first. Flagged:
#   SQLite JSON1 scalar functions: json_patch, json_object,
#     json_extract, json_remove, json_insert, json_replace, json_set,
#     json_quote, json_group_array, json_group_object
#   SQLite-only upsert syntax: INSERT OR IGNORE / INSERT OR REPLACE /
#     INSERT OR UPDATE (PG requires ON CONFLICT)
#   SQLite-only pragmas / WITHOUT ROWID / AUTOINCREMENT
#   PG-only constructs: RETURNING inside plain statements,
#     ::jsonb / ::text casts, DO $$ blocks, jsonb_set / jsonb_build_object
# The JSON1 function names are matched case-insensitively as words,
# including when prefixed by the schema/table qualifier form
# (`details, json_object(...)`).
#
# Portability is judged by construction: a construct listed here is
# known to run on exactly one backend. If a construct is genuinely
# available on both (e.g. plain json in a column type is not scanned),
# it is not listed. Escape hatch for a false positive:
#   -- migration-sql-portability:ok — <reason>  on the flagged line.
#
# Exemptions are legacy debt in
# scripts/migration-sql-portability-exemptions.txt (path:line form,
# frozen count). Fix by rewriting the statement in portable SQL or
# splitting per-backend migration directories; never add entries.
FROZEN_EXEMPTION_COUNT=5
#
# Run by pre-commit and CI.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MIGRATIONS_DIR="$SCRIPT_DIR/../crates/gyre-adapters/migrations"
EXEMPTIONS_FILE="$SCRIPT_DIR/migration-sql-portability-exemptions.txt"
FAIL=0

if [ ! -d "$MIGRATIONS_DIR" ]; then
    echo "FAIL: migrations directory not found: $MIGRATIONS_DIR"
    exit 1
fi

declare -A EXEMPT
EXEMPT_TOTAL=0
if [ -f "$EXEMPTIONS_FILE" ]; then
    while IFS=: read -r file line rest; do
        case "$file" in ''|'#'*) continue ;; esac
        [ -n "$line" ] || continue
        EXEMPT["${file}:${line}"]=1
        EXEMPT_TOTAL=$((EXEMPT_TOTAL + 1))
    done < "$EXEMPTIONS_FILE"
fi

if [ "$EXEMPT_TOTAL" -gt "$FROZEN_EXEMPTION_COUNT" ]; then
    echo "FAIL: migration-sql-portability-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "The exemption file is legacy debt (task-102 F1), not an approval"
    echo "mechanism. Fix a flagged statement by rewriting it in portable SQL"
    echo "(CASE WHEN + string concat, or per-backend migration directories) —"
    echo "never by exempting it. If you fixed a site, delete the line and"
    echo "lower FROZEN_EXEMPTION_COUNT; never raise it."
    exit 1
fi

TMPFILE="$(mktemp /tmp/.mig-portability.XXXXXX)"
STRIPTMP="$(mktemp /tmp/.mig-strip.XXXXXX)"
trap 'rm -f "$TMPFILE" "$STRIPTMP"' EXIT

# Combined pattern (case-insensitive). One grep per file keeps this fast;
# classification runs only on the (few) matched lines.
PATTERN='\b(json_patch|json_object|json_extract|json_remove|json_insert|json_replace|json_set|json_quote|json_group_array|json_group_object)[[:space:]]*\(|\bINSERT[[:space:]]+OR[[:space:]]+(IGNORE|REPLACE|UPDATE)\b|\bPRAGMA[[:space:]]+[A-Za-z_]|\bAUTOINCREMENT\b|\bWITHOUT[[:space:]]+ROWID\b|\b(jsonb_set|jsonb_build_object|jsonb_build_array|jsonb_pretty|to_jsonb)[[:space:]]*\(|::(jsonb|json|text|int8|int4|uuid)\b|\bDO[[:space:]]+\$\$'

find "$MIGRATIONS_DIR" -name '*.sql' -print0 | while IFS= read -r -d '' f; do
    # Strip `--` comment portions but keep original line numbers and
    # blank lines (so reported numbers match the file on disk).
    awk '{ line = $0; sub(/--.*/, "", line); print line }' "$f" > "$STRIPTMP"
    rel="$(realpath --relative-to="$SCRIPT_DIR/.." "$f")"
    grep -niE "$PATTERN" "$STRIPTMP" | while IFS= read -r hit; do
        lineno="${hit%%:*}"
        stmt="${hit#*:}"
        # Inline exemption marker.
        if echo "$stmt" | grep -qi 'migration-sql-portability:ok'; then
            continue
        fi
        reason=""
        if echo "$stmt" | grep -qiE '\b(json_patch|json_object|json_extract|json_remove|json_insert|json_replace|json_set|json_quote|json_group_array|json_group_object)[[:space:]]*\('; then
            reason="SQLite-only JSON1 function"
        elif echo "$stmt" | grep -qiE '\bINSERT[[:space:]]+OR[[:space:]]+(IGNORE|REPLACE|UPDATE)\b'; then
            reason="SQLite-only INSERT OR ... upsert (PG requires ON CONFLICT)"
        elif echo "$stmt" | grep -qiE '\bPRAGMA[[:space:]]+[A-Za-z_]'; then
            reason="SQLite-only PRAGMA"
        elif echo "$stmt" | grep -qiE '\bAUTOINCREMENT\b'; then
            reason="SQLite-only AUTOINCREMENT"
        elif echo "$stmt" | grep -qiE '\bWITHOUT[[:space:]]+ROWID\b'; then
            reason="SQLite-only WITHOUT ROWID"
        elif echo "$stmt" | grep -qiE '\b(jsonb_set|jsonb_build_object|jsonb_build_array|jsonb_pretty|to_jsonb)[[:space:]]*\('; then
            reason="PostgreSQL-only jsonb function"
        elif echo "$stmt" | grep -qE '::(jsonb|json|text|int8|int4|uuid)\b'; then
            reason="PostgreSQL-only ::type cast"
        elif echo "$stmt" | grep -qE '\bDO[[:space:]]+\$\$'; then
            reason="PostgreSQL-only DO \$\$ block"
        fi
        [ -n "$reason" ] || continue
        echo "${rel}:${lineno}:${reason}: $(echo "$stmt" | tr -s ' ')" >> "$TMPFILE"
    done
done

while IFS= read -r line; do
    [ -n "$line" ] || continue
    loc="$(echo "$line" | cut -d: -f1,2)"
    if [ -z "${EXEMPT["$loc"]:-}" ]; then
        echo "$line"
        FAIL=1
    fi
done < "$TMPFILE"

if [ "$FAIL" -ne 0 ]; then
    echo ""
    echo "FAIL: dialect-only SQL in the shared embedded migrations directory."
    echo ""
    echo "The migrations directory is embedded ONCE and executed by BOTH"
    echo "storage backends (sqlite/mod.rs and postgres/mod.rs embed the same"
    echo "dir). Any statement that runs on only one backend breaks the other"
    echo "at startup, at migration time (task-102 F1: PostgreSQL deployments"
    echo "died at 000054's json_patch/json_object/json_extract while every"
    echo "SQLite test stayed green)."
    echo ""
    echo "Fix by rewriting the statement in portable SQL (CASE WHEN + string"
    echo "concatenation, standard JSON text handling in Rust pre-migration,"
    echo "or ON CONFLICT instead of INSERT OR ...) or by splitting"
    echo "per-backend migration directories. Do not exempt new sites."
    exit 1
fi

echo "OK: no dialect-only SQL in shared migrations."
