#!/usr/bin/env bash
# Architecture lint: detect duplicate Diesel migration versions.
#
# Diesel derives a migration's version from everything before the first '_'
# in the directory name, with dashes removed:
#   2026-09-30-000051_post_merge_gates  ->  20260930000051
# Only ONE migration runs per version (alphabetically first wins), so two
# directories mapping to the same version silently drop the second migration
# — its tables/columns are never created on fresh databases (task-095 review
# F5: the post_merge_gate_configs table was dead on fresh installs because
# task-093 and task-095 both picked sequence number 000051).
#
# A guard test exists (no_duplicate_migration_version_prefixes in
# crates/gyre-adapters/src/sqlite/mod.rs) but only runs under `cargo test`,
# which agents skip in favor of targeted test runs. This script gives the
# same check to pre-commit and CI in <100ms with no compilation.
#
# When adding a migration: pick the NEXT unused 6-digit sequence number
# (`ls crates/gyre-adapters/migrations/ | sort | tail -3`).
#
# Run by pre-commit and CI.

set -euo pipefail

MIGRATIONS_DIR="crates/gyre-adapters/migrations"

if [ ! -d "$MIGRATIONS_DIR" ]; then
    echo "FAIL: $MIGRATIONS_DIR not found (run from repo root)"
    exit 1
fi

# Derive version = text before first '_', dashes removed (Diesel semantics).
# Sort by version; any version appearing more than once is a duplicate.
duplicates=$(ls "$MIGRATIONS_DIR" | awk -F'_' '{gsub(/-/, "", $1); print $1 "\t" $0}' \
    | sort | awk -F'\t' '{count[$1]++; names[$1] = names[$1] "\n  " $2} END {
        for (v in count) if (count[v] > 1) print "  version " v ":" names[v]
    }')

if [ -n "$duplicates" ]; then
    echo "FAIL: duplicate Diesel migration versions detected — only one migration"
    echo "runs per version (alphabetically first wins); the rest are silently dropped:"
    echo "$duplicates"
    echo ""
    echo "Renumber the newer migration directory to the next unused 6-digit sequence:"
    echo "  ls $MIGRATIONS_DIR | sort | tail -3"
    exit 1
fi

echo "OK: no duplicate Diesel migration versions."
