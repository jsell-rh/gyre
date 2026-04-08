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
#   Check 2 — Hand-built MCP responses: MCP handlers that use
#             `let VAR = json!({` with 5+ unique field keys in the subsequent
#             30 lines, indicating manual response assembly rather than struct
#             serialization.
#
# Origin: specs/reviews/task-010.md F1 (dead depth param), F2/F3 (missing
#         briefing fields), F4 (edge filter divergence) — all caused by MCP
#         handlers reimplementing REST logic instead of delegating.
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
# Find `let VAR = json!({` lines (excluding test code), then count unique
# "field_name": patterns in the subsequent 30 lines. 5+ unique fields signals
# hand-built response assembly instead of struct serialization.

TOTAL_LINES=$(wc -l < "$MCP_FILE")
TEST_BOUNDARY=$(grep -n '#\[cfg(test)\]' "$MCP_FILE" | head -1 | cut -d: -f1 || echo "$TOTAL_LINES")

# Collect json!({ lines into an array to avoid subshell
mapfile -t JSON_LINES < <(grep -n 'let [a-z_]* = json!({' "$MCP_FILE" || true)

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
