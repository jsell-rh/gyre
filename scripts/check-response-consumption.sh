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
# 4. Item struct field coverage: for Vec<T> element types in consumed
#    *Response structs, each pub field must appear as a string literal in
#    CLI code (or have an exclusion comment). Catches scalar field omissions
#    like BriefingCompletedAgent.conversation_sha being silently dropped.
# 5. Direct response scalar field coverage: for *Response structs where CLI
#    consumes >= 50% of non-infra scalar fields, all other scalars must be
#    referenced. Catches fields like NotificationResponse.entity_ref being
#    dropped.
# 6. Raw epoch display: detects timestamp values (completed_at, timestamp,
#    *_at) interpolated directly in println!/format! calls without going
#    through format_timestamp. Only runs if format_timestamp exists in the
#    CLI (confirming the helper is available).
#
# This script cross-references server response struct definitions with CLI
# display code to catch incomplete consumption.
#
# Enforced by pre-commit hook (response-consumption).
# Supplements implementation checklist item #22.

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
# Scan server response structs with 2+ Vec<T> fields that the CLI consumes.
# For each, verify the CLI references all Vec field names.
# This covers every composite type — including BriefingResponse — by
# extracting Vec fields directly from the struct definition. No hardcoded
# field lists that can fall out of sync when new fields are added.
#
# Scoping: Only check response structs where the CLI references MOST of
# their Vec field names (strictly > 50%). Rendering most-but-not-all
# sections of a composite response usually indicates an accidental
# omission. Rendering only 1 of 3 sections (e.g., nodes but not edges
# or warnings) is a deliberate design choice — a CLI command that shows
# a node table naturally omits edge data.
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

            # Count total and consumed Vec fields
            VEC_TOTAL=0
            VEC_CONSUMED=0
            for field in $FIELDS; do
                [ -z "$field" ] && continue
                VEC_TOTAL=$((VEC_TOTAL + 1))
                if grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                    VEC_CONSUMED=$((VEC_CONSUMED + 1))
                fi
            done

            # Scoping: only flag if CLI consumes strictly > 50% of Vec fields.
            # This distinguishes "rendering most sections, accidentally missed
            # one" from "deliberately rendering a subset".
            [ "$VEC_TOTAL" -eq 0 ] && continue
            VEC_MISSING=$((VEC_TOTAL - VEC_CONSUMED))
            [ "$VEC_CONSUMED" -le "$VEC_MISSING" ] && continue

            for field in $FIELDS; do
                [ -z "$field" ] && continue
                if ! grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                    echo "RESPONSE CONSUMPTION: $STRUCT_NAME.$field is not referenced in CLI display code"
                    echo "  Server: $rs_file — $STRUCT_NAME has Vec field '$field'"
                    echo "  CLI: $CLI_MAIN — no string literal '\"$field\"' found"
                    echo "  The CLI renders $VEC_CONSUMED of $VEC_TOTAL Vec sections — this missing section is likely an accidental omission."
                    echo "  Fix: Either render this field or add a code comment explaining the exclusion."
                    echo ""
                    FAIL=1
                fi
            done
        done <<< "$COMPOSITE_STRUCTS"
    fi
done

# --- Check 4: Item struct field coverage ---
# For structs used as Vec<T> elements in *Response structs, verify that each
# pub field is referenced (as a string literal) in CLI display code. A struct
# like BriefingCompletedAgent with fields {agent_id, spec_ref, decisions,
# uncertainties, conversation_sha, completed_at} should have ALL fields
# appear in the CLI — silent scalar field omissions produce display functions
# that compile and run but silently drop data.
#
# Scoping: Only check item types where the CLI references >= 50% of their
# pub fields as string literals. This confirms the CLI is actively rendering
# this item type in a display function — not just coincidentally referencing
# a generic field name. A display function that accesses 3 of 6 BriefingItem
# fields is clearly consuming BriefingItem and the remaining 3 fields need
# attention. A type where the CLI accesses only 1 of 18 fields is not being
# actively consumed and should not be flagged.
#
# Infrastructure fields that are legitimately omitted from display output:
INFRA_FIELDS="workspace_id|repo_id|resolved_at|dismissed_at|created_at|since|id|tenant_id"

for rs_file in "$SERVER_API_DIR"/*.rs; do
    [ -f "$rs_file" ] || continue

    # Step 1: Find *Response structs and their Vec<T> element types
    RESPONSE_STRUCTS=$(awk '
        /^pub struct \w+Response/ {
            struct_name = $3
            gsub(/\{/, "", struct_name)
            item_types = ""
            in_struct = 1
            next
        }
        in_struct && /^\}/ {
            if (item_types != "") {
                print struct_name "|" item_types
            }
            in_struct = 0
        }
        in_struct && /pub \w+: Vec</ {
            match($0, /Vec<([A-Z][a-zA-Z0-9_]+)>/, m2)
            if (m2[1] != "" && m2[1] !~ /^(String|Value|u8)$/) {
                item_types = item_types " " m2[1]
            }
        }
    ' "$rs_file" 2>/dev/null || true)

    if [ -n "$RESPONSE_STRUCTS" ]; then
        while IFS= read -r resp_entry; do
            [ -z "$resp_entry" ] && continue
            STRUCT_NAME=$(echo "$resp_entry" | cut -d'|' -f1)
            ITEM_TYPES=$(echo "$resp_entry" | cut -d'|' -f2)

            # Step 2: For each item type, check field coverage
            for item_type in $ITEM_TYPES; do
                [ -z "$item_type" ] && continue

                # Find the struct definition
                FIELDS=""
                for search_file in "$rs_file" "$SERVER_API_DIR"/*.rs; do
                    FIELDS=$(awk -v sname="$item_type" '
                        $0 ~ "^pub struct " sname " \\{" || $0 ~ "^pub struct " sname "$" {
                            in_struct = 1; next
                        }
                        in_struct && /^\}/ { in_struct = 0 }
                        in_struct && /pub [a-z_]+:/ {
                            match($0, /pub ([a-z_]+):/, m)
                            if (m[1] != "") print m[1]
                        }
                    ' "$search_file" 2>/dev/null || true)
                    [ -n "$FIELDS" ] && break
                done

                [ -z "$FIELDS" ] && continue

                # Count total non-infra fields and consumed fields
                TOTAL_FIELDS=0
                CONSUMED_FIELDS=0
                for field in $FIELDS; do
                    [ -z "$field" ] && continue
                    if echo "$field" | grep -qE "^($INFRA_FIELDS)$"; then
                        continue
                    fi
                    TOTAL_FIELDS=$((TOTAL_FIELDS + 1))
                    if grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                        CONSUMED_FIELDS=$((CONSUMED_FIELDS + 1))
                    fi
                done

                # Scoping: only flag if CLI consumes >= 50% of fields.
                # This confirms active rendering — not incidental references.
                [ "$TOTAL_FIELDS" -eq 0 ] && continue
                HALF=$(( (TOTAL_FIELDS + 1) / 2 ))  # ceiling division
                [ "$CONSUMED_FIELDS" -lt "$HALF" ] && continue

                for field in $FIELDS; do
                    [ -z "$field" ] && continue
                    # Skip infrastructure fields
                    if echo "$field" | grep -qE "^($INFRA_FIELDS)$"; then
                        continue
                    fi
                    if ! grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                        # Check if there's an explicit exclusion comment
                        if ! grep -q "// $field:" "$CLI_MAIN" 2>/dev/null; then
                            echo "RESPONSE CONSUMPTION: $item_type.$field is not referenced in CLI display code"
                            echo "  Server: $rs_file — $item_type has field '$field' (used in $STRUCT_NAME)"
                            echo "  CLI: $CLI_MAIN — no string literal '\"$field\"' found and no exclusion comment '// $field:'"
                            echo "  The CLI renders $CONSUMED_FIELDS of $TOTAL_FIELDS non-infra fields from $item_type — missing fields are likely accidental omissions."
                            echo "  Fix: Either render this field or add a code comment '// $field: <reason for exclusion>'."
                            echo ""
                            FAIL=1
                        fi
                    fi
                done
            done
        done <<< "$RESPONSE_STRUCTS"
    fi
done

# --- Check 5: Direct response struct field coverage ---
# For *Response structs that are consumed directly by CLI display functions,
# check that each pub scalar field (non-Vec) is referenced in CLI code.
# Target: NotificationResponse (consumed by inbox/divergence display).
#
# Scoping: Only check response structs where at least one of their scalar
# field names appears in CLI code (confirming the CLI consumes the type).
for rs_file in "$SERVER_API_DIR"/*.rs; do
    [ -f "$rs_file" ] || continue

    LEAF_STRUCTS=$(awk '
        /^pub struct \w+Response/ {
            struct_name = $3
            gsub(/\{/, "", struct_name)
            fields = ""
            in_struct = 1
            next
        }
        in_struct && /^\}/ {
            if (fields != "") {
                print struct_name ":" fields
            }
            in_struct = 0
        }
        in_struct && /pub [a-z_]+:/ {
            match($0, /pub ([a-z_]+):/, m)
            if (m[1] != "") {
                # Skip Vec fields (already covered by Check 3/4)
                if ($0 !~ /Vec</) {
                    fields = fields " " m[1]
                }
            }
        }
    ' "$rs_file" 2>/dev/null || true)

    if [ -n "$LEAF_STRUCTS" ]; then
        while IFS= read -r entry; do
            [ -z "$entry" ] && continue
            STRUCT_NAME=$(echo "$entry" | cut -d: -f1)
            FIELDS=$(echo "$entry" | cut -d: -f2-)

            # Count non-infra scalar fields and consumed scalar fields
            TOTAL_SCALARS=0
            CONSUMED_SCALARS=0
            for field in $FIELDS; do
                [ -z "$field" ] && continue
                # Skip infrastructure fields
                if echo "$field" | grep -qE "^($INFRA_FIELDS)$"; then
                    continue
                fi
                TOTAL_SCALARS=$((TOTAL_SCALARS + 1))
                if grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                    CONSUMED_SCALARS=$((CONSUMED_SCALARS + 1))
                fi
            done

            # Scoping: only flag if CLI consumes >= 50% of non-infra scalars.
            # This confirms the CLI is actively rendering this response type.
            [ "$TOTAL_SCALARS" -eq 0 ] && continue
            HALF_SCALARS=$(( (TOTAL_SCALARS + 1) / 2 ))  # ceiling division
            [ "$CONSUMED_SCALARS" -lt "$HALF_SCALARS" ] && continue

            for field in $FIELDS; do
                [ -z "$field" ] && continue
                # Skip infrastructure fields
                if echo "$field" | grep -qE "^($INFRA_FIELDS)$"; then
                    continue
                fi
                if ! grep -q "\"$field\"" "$CLI_MAIN" 2>/dev/null; then
                    if ! grep -q "// $field:" "$CLI_MAIN" 2>/dev/null; then
                        echo "RESPONSE CONSUMPTION: $STRUCT_NAME.$field (scalar) is not referenced in CLI display code"
                        echo "  Server: $rs_file — $STRUCT_NAME has field '$field'"
                        echo "  CLI: $CLI_MAIN — no string literal '\"$field\"' found and no exclusion comment '// $field:'"
                        echo "  The CLI renders $CONSUMED_SCALARS of $TOTAL_SCALARS non-infra scalar fields — missing fields are likely accidental omissions."
                        echo "  Fix: Either render this field or add a code comment '// $field: <reason for exclusion>'."
                        echo ""
                        FAIL=1
                    fi
                fi
            done
        done <<< "$LEAF_STRUCTS"
    fi
done

# --- Check 6: Raw epoch display ---
# Detect timestamp values rendered as raw integers instead of using
# format_timestamp (or similar formatting). When a CLI display function
# has access to a format_timestamp helper, all epoch values should go
# through it for human-readable output (e.g., "2024-03-24 22:40:00Z"
# instead of "1711324800").
#
# Strategy: find println!/format! calls that interpolate variables with
# timestamp-like names (completed_at, timestamp, *_at) without a
# format_timestamp call. We look for patterns like:
#   println!("... {completed_at}")  or  println!("... {ts}")
# in display functions that DON'T have a corresponding
#   format_timestamp(completed_at)  or  format_timestamp(ts)
# nearby.
#
# Only runs if format_timestamp exists in the CLI (confirming the helper
# is available and should be used consistently).
if grep -q 'fn format_timestamp' "$CLI_MAIN" 2>/dev/null; then
    # Find variable names that look like timestamps being interpolated
    # in println!/format! calls without format_timestamp wrapping.
    # Pattern: {var_name} where var_name matches *_at or timestamp
    EPOCH_VARS=$(grep -n 'println!\|format!' "$CLI_MAIN" 2>/dev/null | \
        grep -oP '\{(completed_at|updated_at|started_at|ended_at|timestamp|created_at_val|ts)\}' | \
        sed 's/[{}]//g' | sort -u || true)

    if [ -n "$EPOCH_VARS" ]; then
        while IFS= read -r var; do
            [ -z "$var" ] && continue
            # Find lines where this var is interpolated in a print statement
            RAW_USES=$(grep -n "println!\|format!" "$CLI_MAIN" 2>/dev/null | \
                grep "{${var}}" | grep -v "format_timestamp" || true)
            if [ -n "$RAW_USES" ]; then
                while IFS= read -r raw_line; do
                    [ -z "$raw_line" ] && continue
                    LINENUM=$(echo "$raw_line" | cut -d: -f1)
                    # Check if format_timestamp is called on this var within ±5 lines
                    CONTEXT=$(sed -n "$((LINENUM > 5 ? LINENUM - 5 : 1)),$((LINENUM + 5))p" "$CLI_MAIN" 2>/dev/null || true)
                    if ! echo "$CONTEXT" | grep -q "format_timestamp.*${var}\|format_timestamp(${var}" 2>/dev/null; then
                        echo "RESPONSE CONSUMPTION: Raw epoch display — '$var' is interpolated without format_timestamp"
                        echo "  $CLI_MAIN:$LINENUM: $(echo "$raw_line" | cut -d: -f2-)"
                        echo "  The format_timestamp helper exists in this file. Use it for human-readable output."
                        echo "  Fix: Replace raw interpolation with format_timestamp($var)."
                        echo ""
                        FAIL=1
                    fi
                done <<< "$RAW_USES"
            fi
        done <<< "$EPOCH_VARS"
    fi
fi

if [ "$FAIL" -eq 0 ]; then
    echo "Response consumption lint passed."
fi

exit "$FAIL"
