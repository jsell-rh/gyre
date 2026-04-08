#!/usr/bin/env bash
# Response Consumption Lint: detect CLI display functions that silently drop
# fields or sections from server response structs.
#
# Checks:
# 1. (Reserved — previously hardcoded BriefingResponse fields; now handled
#    dynamically by Check 3. Removed after R9/F19.)
# 2. Nested item structs (structs used as Vec<T> elements in responses) must
#    have more than just their `title` field accessed in CLI display code.
# 3. ALL *Response structs with 2+ Vec<T> fields: every Vec field must be
#    referenced in CLI display code. Fields are extracted dynamically from
#    the server struct definitions — no hardcoded lists.
#
# This script cross-references server response struct definitions with CLI
# display code to catch incomplete consumption.
#
# Run during pre-commit and CI. Supplements implementation checklist item #22.

set -euo pipefail

CLI_MAIN="crates/gyre-cli/src/main.rs"
SERVER_API_DIR="crates/gyre-server/src/api"

if [ ! -f "$CLI_MAIN" ] || [ ! -d "$SERVER_API_DIR" ]; then
    echo "SKIP: CLI or server API not found"
    exit 0
fi

FAIL=0

# --- Check 1: Composite response section coverage ---
# Dynamically handled by Check 3 below for ALL *Response structs, including
# BriefingResponse. No hardcoded field lists — the check extracts Vec fields
# directly from the server struct definitions so new fields are caught
# automatically. (Removed hardcoded BRIEFING_VEC_FIELDS after R9/F19 showed
# that a hardcoded list missed completed_agents.)

# --- Check 2: Item field coverage ---
# When a CLI display function accesses a Vec<Item> and renders each item,
# verify it accesses more than just ["title"]. A display loop that only
# reads item["title"] is silently dropping all other fields.
#
# Strategy: find for-loops over briefing item arrays, check if any field
# besides "title" is accessed within the loop body.

# Extract for-loop blocks that iterate over briefing items
# Pattern: `for item in items {` followed by item["..."] accesses
ITEM_LOOPS=$(awk '
    /for item in items/ { in_loop = 1; start = NR; fields = "" }
    in_loop && /item\["([^"]+)"\]/ {
        match($0, /item\["([^"]+)"\]/, m)
        if (m[1] != "" && fields !~ m[1]) {
            fields = fields " " m[1]
        }
    }
    in_loop && /^[[:space:]]*\}/ {
        # Count distinct fields
        n = split(fields, arr, " ")
        if (n <= 1 && n > 0) {
            print start ": only accesses field:" fields " (expected: title, description, spec_path, timestamp at minimum)"
        }
        in_loop = 0
    }
' "$CLI_MAIN" 2>/dev/null || true)

if [ -n "$ITEM_LOOPS" ]; then
    while IFS= read -r entry; do
        [ -z "$entry" ] && continue
        echo "RESPONSE CONSUMPTION: CLI display loop accesses only one field from BriefingItem"
        echo "  $CLI_MAIN:$entry"
        echo "  BriefingItem has fields: title, description, entity_type, entity_id, spec_path, timestamp"
        echo "  Rendering only 'title' silently drops description, spec references, and timestamps."
        echo "  Fix: Render description (when non-empty), spec_path (when present), and timestamp."
        echo ""
        FAIL=1
    done <<< "$ITEM_LOOPS"
fi

# --- Check 3: Dynamic composite response detection ---
# Scan ALL server response structs for those with 2+ Vec<T> fields.
# For each, verify the CLI references all Vec field names.
# This covers every composite type — including BriefingResponse — by
# extracting Vec fields directly from the struct definition. No hardcoded
# field lists that can fall out of sync when new fields are added.
for rs_file in "$SERVER_API_DIR"/*.rs; do
    [ -f "$rs_file" ] || continue

    # Find structs with 2+ Vec fields (potential composite responses)
    COMPOSITE_STRUCTS=$(awk '
        /^pub struct \w+Response/ {
            struct_name = $3
            gsub(/\{/, "", struct_name)
            vec_count = 0
            vec_fields = ""
            in_struct = 1
            next
        }
        in_struct && /^\}/ {
            if (vec_count >= 2) {
                print struct_name ":" vec_fields
            }
            in_struct = 0
        }
        in_struct && /pub \w+: Vec</ {
            match($0, /pub ([a-z_]+):/, m)
            if (m[1] != "") {
                vec_count++
                vec_fields = vec_fields " " m[1]
            }
        }
    ' "$rs_file" 2>/dev/null || true)

    if [ -n "$COMPOSITE_STRUCTS" ]; then
        while IFS= read -r composite; do
            [ -z "$composite" ] && continue
            STRUCT_NAME=$(echo "$composite" | cut -d: -f1)
            FIELDS=$(echo "$composite" | cut -d: -f2-)

            for field in $FIELDS; do
                [ -z "$field" ] && continue
                if ! grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                    echo "RESPONSE CONSUMPTION: $STRUCT_NAME.$field is not referenced in CLI display code"
                    echo "  Server: $rs_file — $STRUCT_NAME has Vec field '$field'"
                    echo "  CLI: $CLI_MAIN — no string literal '\"$field\"' found"
                    echo "  If the CLI renders this response type, it may be silently dropping this section."
                    echo "  Fix: Either render this field or add a code comment explaining the exclusion."
                    echo ""
                    FAIL=1
                fi
            done
        done <<< "$COMPOSITE_STRUCTS"
    fi
done

if [ "$FAIL" -eq 0 ]; then
    echo "Response consumption lint passed."
fi

exit "$FAIL"
