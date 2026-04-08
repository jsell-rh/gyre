#!/usr/bin/env bash
# Architecture lint: detect MCP handler parity violations against REST endpoints.
#
# MCP tools and resources that wrap REST endpoints must delegate to the same
# domain/service function the REST handler calls. When an MCP handler
# reimplements REST logic instead of delegating, three types of divergence occur:
#   1. Invented input parameters (MCP tool schema advertises params the REST
#      endpoint doesn't accept)
#   2. Missing output fields (hand-built JSON omits fields the REST handler's
#      response struct includes)
#   3. Logic divergence (reimplemented filters/predicates differ from REST)
#
# Detections:
#   Check 1 — Dead tool parameters: variables prefixed with _ that come from
#             MCP tool args parsing (let _NAME = args.get(...)). An underscore-
#             prefixed variable from tool args means the parameter is parsed
#             from user input, advertised in the schema, but never used.
#   Check 2 — Hand-built MCP responses: ANY json!({ occurrence in non-test
#             MCP handler code with 5+ unique field keys in the subsequent
#             30 lines, indicating manual response assembly rather than struct
#             serialization. Catches both `let var = json!({` and inline
#             json!({ inside closures/iterators (e.g., `.map(|n| json!({...}))`).
#   Check 3 — Debug-format enum serialization: MCP handlers that use
#             `format!("{:?}", VAR).to_lowercase()` instead of serializing
#             through serde. Debug format produces "enumvariant" for multi-word
#             variants (e.g., EnumVariant), while serde #[serde(rename_all =
#             "snake_case")] produces "enum_variant". These are different
#             strings — consumers that discriminate on type strings will
#             silently fail to match MCP values against REST values.
#
# Origin: specs/reviews/task-010.md F1 (dead depth param), F2/F3 (missing
#         briefing fields), F4 (edge filter divergence), F5 (hand-built JSON
#         in closures), F7 (Debug-format enum serialization) — all caused by
#         MCP handlers reimplementing REST logic instead of delegating.
#
# Exempt with: // mcp-parity:ok — <reason>
#
# Run by pre-commit and CI.

set -euo pipefail

MCP_FILE="crates/gyre-server/src/mcp.rs"
VIOLATIONS=0

if [ ! -f "$MCP_FILE" ]; then
    echo "Skipping MCP wrapper parity check: $MCP_FILE not found"
    exit 0
fi

echo "Checking MCP wrapper parity..."

# ── Check 1: Dead tool parameters ──────────────────────────────────────────
# Pattern: let _name = args.get("...")  or  let _name = args\n  .get("...")
# These are tool parameters that are parsed but unused. If the parameter is
# advertised in the tool schema, users will supply it expecting an effect.

DEAD_PARAMS=$(grep -n 'let _[a-z_]* = args' "$MCP_FILE" | grep -v 'mcp-parity:ok' || true)

if [ -n "$DEAD_PARAMS" ]; then
    while IFS= read -r line; do
        echo ""
        echo "DEAD TOOL PARAMETER: $MCP_FILE:$(echo "$line" | cut -d: -f1)"
        echo "  $line"
        echo "  A tool parameter is parsed from args but stored in an underscore-prefixed"
        echo "  variable, meaning it has no effect on results. If the REST endpoint this"
        echo "  tool wraps does not accept this parameter, remove it from the tool schema"
        echo "  and handler. If the REST endpoint does accept it, wire it through."
        echo ""
        echo "  Exempt with '// mcp-parity:ok — <reason>' on the same line."
        VIOLATIONS=$((VIOLATIONS + 1))
    done <<< "$DEAD_PARAMS"
fi

# ── Check 2: Hand-built MCP responses ─────────────────────────────────────
# Find ALL json!({ occurrences in non-test MCP handler code (not just
# `let var = json!({` — also catches json!({ inside closures, iterators,
# .map(), .push(), etc.), then count unique "field_name": patterns in the
# subsequent 30 lines. 5+ unique fields signals hand-built response assembly
# instead of struct serialization.
#
# Prior version only matched `let VAR = json!({`, which missed the most
# common hand-built pattern: per-item JSON construction inside .map() closures
# (e.g., `.map(|n| json!({ "id": ..., "name": ..., ... }))`). This allowed
# R2 findings F5/F7 to slip through.

TOTAL_LINES=$(wc -l < "$MCP_FILE")
TEST_BOUNDARY=$(grep -n '#\[cfg(test)\]' "$MCP_FILE" | head -1 | cut -d: -f1 || echo "$TOTAL_LINES")

# Collect ALL json!({ lines into an array — not just `let var = json!({`
mapfile -t JSON_LINES < <(grep -n 'json!({' "$MCP_FILE" | grep -v 'json!({})' || true)

for entry in "${JSON_LINES[@]}"; do
    [ -z "$entry" ] && continue
    lineno=$(echo "$entry" | cut -d: -f1)
    rest=$(echo "$entry" | cut -d: -f2-)

    # Skip test code
    if [ "$lineno" -ge "$TEST_BOUNDARY" ]; then
        continue
    fi

    # Check for exemption on the json!({ line
    if echo "$rest" | grep -q 'mcp-parity:ok'; then
        continue
    fi

    # Count unique "field_name": patterns in the next 30 lines after json!({
    WINDOW_START=$((lineno + 1))
    WINDOW_END=$((lineno + 30))
    UNIQUE_FIELDS=$(sed -n "${WINDOW_START},${WINDOW_END}p" "$MCP_FILE" \
        | grep -oP '"[a-z_]+":\s' \
        | sort -u \
        | wc -l || true)

    if [ "$UNIQUE_FIELDS" -ge 5 ]; then
        # Determine which handler this is in
        HANDLER=$(head -n "$lineno" "$MCP_FILE" \
            | grep -oP '(strip_prefix\("\K[^"]+|async fn \K[a-z_]+)' \
            | tail -1 || echo "unknown")

        echo ""
        echo "HAND-BUILT MCP RESPONSE: $MCP_FILE:$lineno ($HANDLER, $UNIQUE_FIELDS unique fields)"
        echo "  MCP handler constructs a json!({...}) response with $UNIQUE_FIELDS fields"
        echo "  instead of serializing a domain struct. Hand-built JSON is prone to field"
        echo "  omission and semantic divergence from the REST endpoint it wraps."
        echo ""
        echo "  Preferred pattern: call the same domain function the REST handler calls and"
        echo "  serialize via serde_json::to_value(&result). If the REST handler calls"
        echo "  get_workspace_briefing(), the MCP handler should call the same function."
        echo ""
        echo "  Exempt with '// mcp-parity:ok — <reason>' on the json!({ line."
        VIOLATIONS=$((VIOLATIONS + 1))
    fi
done

# ── Check 3: Debug-format enum serialization ─────────────────────────────
# Detect `format!("{:?}", VAR).to_lowercase()` in non-test MCP handler code.
# Rust's Debug format on enums produces "EnumVariant" → lowercased to
# "enumvariant". Serde's #[serde(rename_all = "snake_case")] produces
# "enum_variant". For single-word variants these coincidentally match
# (e.g., Module → "module"), but for multi-word variants they diverge:
#   - DependsOn  → Debug: "dependson",  serde: "depends_on"
#   - EnumVariant → Debug: "enumvariant", serde: "enum_variant"
#   - FieldOf    → Debug: "fieldof",    serde: "field_of"
# Consumers that match on type strings (e.g., filtering edges by "depends_on")
# will silently fail against MCP responses that serialize as "dependson".
#
# The correct pattern: serialize through the response struct (which uses serde),
# or use serde_json::to_value(&enum_value) for individual fields.

DEBUG_FMT=$(grep -n 'format!("{:?}"' "$MCP_FILE" | grep -v 'mcp-parity:ok' || true)

if [ -n "$DEBUG_FMT" ]; then
    while IFS= read -r line; do
        lineno=$(echo "$line" | cut -d: -f1)

        # Skip test code
        if [ "$lineno" -ge "$TEST_BOUNDARY" ]; then
            continue
        fi

        HANDLER=$(head -n "$lineno" "$MCP_FILE" \
            | grep -oP '(strip_prefix\("\K[^"]+|async fn \K[a-z_]+)' \
            | tail -1 || echo "unknown")

        echo ""
        echo "DEBUG-FORMAT ENUM SERIALIZATION: $MCP_FILE:$lineno ($HANDLER)"
        echo "  $line"
        echo "  Using format!(\"{:?}\", ...) to serialize an enum produces different"
        echo "  strings than serde for multi-word variants (e.g., \"dependson\" vs"
        echo "  \"depends_on\"). This causes silent mismatches between MCP and REST"
        echo "  responses for the same data."
        echo ""
        echo "  Fix: serialize through the response struct via serde_json::to_value(),"
        echo "  or use serde_json::to_value(&enum_value) for individual enum fields."
        echo ""
        echo "  Exempt with '// mcp-parity:ok — <reason>' on the same line."
        VIOLATIONS=$((VIOLATIONS + 1))
    done <<< "$DEBUG_FMT"
fi

echo ""
if [ "$VIOLATIONS" -eq 0 ]; then
    echo "MCP wrapper parity check passed."
    exit 0
else
    echo "Fix: MCP tools/resources that wrap REST endpoints should delegate to the"
    echo "     same domain/service function the REST handler calls, not reimplement"
    echo "     the logic. Serialize the domain struct, don't hand-build JSON."
    echo "${VIOLATIONS} violation(s) found."
    exit 1
fi
