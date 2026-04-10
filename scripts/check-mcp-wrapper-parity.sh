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
#   Check 4 — Simplified parameter defaults: MCP handlers that call a shared
#             assemble_* function but skip stateful parameter resolution that
#             the REST handler performs before the same call (e.g., looking up
#             user_workspace_state.get_last_seen() for a `since` default).
#   Check 5 — Stale route annotations: doc comments like `/// GET /api/v1/...`
#             on non-handler functions (shared utilities extracted from handlers
#             that kept the route annotation instead of leaving it on the
#             actual handler).
#   Check 6 — Raw LLM response passthrough: MCP handlers that call
#             stream_complete (LLM endpoint) and return the raw collected
#             text via tool_result() without performing JSON parsing or
#             validation. The REST handler for the same LLM endpoint
#             typically parses the raw LLM text as JSON, validates expected
#             fields (e.g., {diff, explanation}), and sends structured error
#             events on invalid output. An MCP handler that skips this
#             validation returns raw, unparsed LLM text — the consumer
#             gets no error indication and no structured data when the LLM
#             produces invalid output.
#   Check 7 — REST→MCP parameter drift: REST request structs have fields
#             that the corresponding MCP tool's inputSchema does not include.
#             When a REST endpoint gains a parameter, the MCP tool must be
#             updated to maintain HSI §11 parity. MCP callers silently
#             cannot access the new functionality.
#
# Origin: specs/reviews/task-010.md F1 (dead depth param), F2/F3 (missing
#         briefing fields), F4 (edge filter divergence), F5 (hand-built JSON
#         in closures), F7 (Debug-format enum serialization), F8 (briefing
#         since-default parity), F9 (stale route annotation after extraction)
#         — all caused by MCP handlers reimplementing REST logic or carrying
#         stale metadata after refactoring.
#         specs/reviews/task-012.md R2 F5 (raw LLM response passthrough) —
#         MCP handler returned raw LLM text without JSON parsing/validation
#         that the REST handler performs.
#         specs/reviews/task-028.md R1 F4 (REST→MCP parameter drift) —
#         CreateMrRequest gained depends_on but gyre_create_mr tool schema
#         was not updated.
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

# ── Check 4: Simplified parameter defaults skipping stateful resolution ──
# When an MCP handler calls a shared assemble_* function, the parameters
# it passes must be resolved the same way the REST handler resolves them.
# The most common failure: the REST handler resolves a "since" parameter
# via (1) explicit param, (2) user_workspace_state.get_last_seen(), (3)
# hardcoded fallback — but the MCP handler skips step 2 and falls back
# directly to the hardcoded default.
#
# Detection strategy: for each assemble_* call in mcp.rs, find the
# corresponding REST handler in api/*.rs that calls the same function.
# If the REST handler references a stateful resolution function (e.g.,
# get_last_seen, user_workspace_state) that the MCP handler does not,
# the MCP handler has a simplified default.
#
# Origin: specs/reviews/task-010.md F8 — briefing MCP handler skipped
#         last_seen_at lookup, producing 24h of data instead of
#         since-last-visit for returning users.

API_DIR="crates/gyre-server/src/api"

# Find all assemble_* calls in MCP file (non-test only)
mapfile -t ASSEMBLE_CALLS < <(grep -n 'assemble_[a-z_]*(' "$MCP_FILE" | head -20)

for entry in "${ASSEMBLE_CALLS[@]}"; do
    [ -z "$entry" ] && continue
    lineno=$(echo "$entry" | cut -d: -f1)

    # Skip test code
    if [ "$lineno" -ge "$TEST_BOUNDARY" ]; then
        continue
    fi

    # Skip exempted lines
    line_text=$(sed -n "${lineno}p" "$MCP_FILE")
    if echo "$line_text" | grep -q 'mcp-parity:ok'; then
        continue
    fi

    # Extract the function name (e.g., assemble_briefing)
    fn_name=$(echo "$line_text" | grep -oP 'assemble_[a-z_]+' | head -1)
    [ -z "$fn_name" ] && continue

    # Find the REST handler file(s) that define this function
    REST_FILES=$(grep -rl "pub async fn $fn_name\|pub fn $fn_name" "$API_DIR" 2>/dev/null || true)
    [ -z "$REST_FILES" ] && continue

    # Check if the REST handler callers of this assemble function use
    # stateful resolution that the MCP handler should also use.
    # Look for stateful patterns in a window around each REST call site
    # (not the whole file — other functions in the file may use different
    # stateful lookups that are irrelevant to this assemble function).
    for rest_file in $REST_FILES; do
        # Find call sites of fn_name in the REST handler file
        # (exclude the function definition itself)
        REST_CALL_LINES=$(grep -n "${fn_name}(" "$rest_file" \
            | grep -v "pub async fn\|pub fn" 2>/dev/null || true)
        [ -z "$REST_CALL_LINES" ] && continue

        while IFS= read -r rest_call; do
            [ -z "$rest_call" ] && continue
            rest_lineno=$(echo "$rest_call" | cut -d: -f1)

            # Check a 50-line window before the REST call for stateful patterns
            REST_WINDOW_START=$((rest_lineno > 50 ? rest_lineno - 50 : 1))
            STATEFUL_PATTERNS=$(sed -n "${REST_WINDOW_START},${rest_lineno}p" "$rest_file" \
                | grep -c 'user_workspace_state\|get_last_seen\|get_user_preference' || true)
            STATEFUL_PATTERNS=${STATEFUL_PATTERNS:-0}

            if [ "$STATEFUL_PATTERNS" -gt 0 ]; then
                # The REST caller uses stateful resolution before calling fn_name.
                # Check if the MCP handler also references these patterns.
                WINDOW_START=$((lineno > 50 ? lineno - 50 : 1))
                MCP_STATEFUL=$(sed -n "${WINDOW_START},${lineno}p" "$MCP_FILE" \
                    | grep -c 'user_workspace_state\|get_last_seen\|get_user_preference' || true)
                MCP_STATEFUL=${MCP_STATEFUL:-0}

                if [ "$MCP_STATEFUL" -eq 0 ]; then
                    HANDLER=$(head -n "$lineno" "$MCP_FILE" \
                        | grep -oP '(strip_prefix\("\K[^"]+|async fn \K[a-z_]+)' \
                        | tail -1 || echo "unknown")

                    echo ""
                    echo "SIMPLIFIED PARAMETER DEFAULT: $MCP_FILE:$lineno ($HANDLER → $fn_name)"
                    echo "  The REST handler in $rest_file:$rest_lineno resolves parameters"
                    echo "  via stateful lookups (user_workspace_state, get_last_seen, etc.)"
                    echo "  before calling $fn_name, but the MCP handler passes a simplified"
                    echo "  default."
                    echo ""
                    echo "  The MCP handler must replicate the REST handler's full parameter"
                    echo "  resolution chain — including intermediate steps that consult user"
                    echo "  state — to maintain HSI §11 parity."
                    echo ""
                    echo "  Exempt with '// mcp-parity:ok — <reason>' on the $fn_name() call line."
                    VIOLATIONS=$((VIOLATIONS + 1))
                fi
            fi
        done <<< "$REST_CALL_LINES"
    done
done

# ── Check 5: Stale route annotations on non-handler functions ────────────
# When shared functions are extracted from HTTP handlers, route annotation
# doc comments (/// GET /api/v1/..., /// POST /api/v1/...) sometimes
# follow the extracted logic instead of staying on the handler. A route
# annotation on a non-handler function is misleading.
#
# Detection: find /// GET|POST|PUT|DELETE|PATCH /api/ doc comments and
# verify the function they annotate has handler-like extractors (State,
# Path, Query, Json) in its signature. Functions without extractors are
# likely shared utilities, not handlers.
#
# Origin: specs/reviews/task-010.md F9 — assemble_concept_results had a
#         stale route annotation from get_graph_concept after extraction.

for src_file in $(find crates/gyre-server/src/ -name '*.rs' -not -path '*/tests/*' 2>/dev/null); do
    # Find lines with route annotation doc comments
    ROUTE_ANNOTATIONS=$(grep -n '/// \(GET\|POST\|PUT\|DELETE\|PATCH\) /api/' "$src_file" 2>/dev/null || true)
    [ -z "$ROUTE_ANNOTATIONS" ] && continue

    while IFS= read -r annotation; do
        [ -z "$annotation" ] && continue
        ann_lineno=$(echo "$annotation" | cut -d: -f1)
        ann_text=$(echo "$annotation" | cut -d: -f2-)

        # Check exemption
        if echo "$ann_text" | grep -q 'mcp-parity:ok'; then
            continue
        fi

        # Find the function signature: scan forward from the annotation line
        # looking for "pub async fn" or "pub fn" within 10 lines, then
        # grab the full signature (up to the opening brace or return type)
        FN_START=$((ann_lineno + 1))
        FN_END=$((ann_lineno + 20))
        FN_SIG_BLOCK=$(sed -n "${FN_START},${FN_END}p" "$src_file")
        FN_FIRST=$(echo "$FN_SIG_BLOCK" | grep -m1 'pub async fn\|pub fn' || true)
        [ -z "$FN_FIRST" ] && continue

        # Skip zero-parameter functions — they're valid handlers that
        # just don't need any extractors (e.g., version_handler())
        if echo "$FN_FIRST" | grep -q '()'; then
            continue
        fi

        # Check if the function has handler extractors in its full signature
        # block (State<, Path<, Query<, Json<) — extractors may be on
        # subsequent parameter lines. Functions with parameters but no
        # extractors are shared utilities, not handlers.
        HAS_EXTRACTORS=$(echo "$FN_SIG_BLOCK" | grep -c 'State<\|Path<\|Query<\|Json<' || true)
        HAS_EXTRACTORS=${HAS_EXTRACTORS:-0}

        if [ "$HAS_EXTRACTORS" -eq 0 ]; then
            fn_name=$(echo "$FN_FIRST" | grep -oP '(pub async fn|pub fn) \K[a-z_]+' | head -1)
            echo ""
            echo "STALE ROUTE ANNOTATION: $src_file:$ann_lineno"
            echo "  $ann_text"
            echo "  This route annotation is on function '$fn_name' which has no handler"
            echo "  extractors (State, Path, Query, Json). It is likely a shared utility"
            echo "  function, not an HTTP handler. Route annotations should be on the"
            echo "  actual handler function."
            echo ""
            echo "  Fix: remove the route annotation from this function and ensure the"
            echo "  actual handler function has the route annotation."
            echo ""
            echo "  Exempt with '// mcp-parity:ok — <reason>' on the annotation line."
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done <<< "$ROUTE_ANNOTATIONS"
done

# ── Check 6: Raw LLM response passthrough ────────────────────────────────
# When an MCP handler calls stream_complete (LLM call), collects the raw
# text, and returns it via tool_result() without JSON parsing/validation,
# the consumer gets unvalidated LLM output. The REST handler for the same
# endpoint typically parses the raw text as JSON, validates required fields,
# and returns structured errors on invalid output. An MCP handler that
# skips this returns raw text where structured data is expected.
#
# Detection: find functions in mcp.rs that (a) call stream_complete and
# (b) return tool_result(variable) without any JSON parsing between
# the LLM call and the return. JSON parsing signals: serde_json::from_str,
# serde_json::from_value, .as_object(), .get("field").
#
# Origin: specs/reviews/task-012.md R2 F5 — MCP spec_assist handler
#         returned raw LLM text while REST handler parsed and validated
#         {diff, explanation} JSON.

# Find all functions in MCP file that call stream_complete
mapfile -t LLM_FN_LINES < <(grep -n 'stream_complete' "$MCP_FILE" | head -20)

for entry in "${LLM_FN_LINES[@]}"; do
    [ -z "$entry" ] && continue
    lineno=$(echo "$entry" | cut -d: -f1)

    # Skip test code
    if [ "$lineno" -ge "$TEST_BOUNDARY" ]; then
        continue
    fi

    # Check for exemption
    line_text=$(sed -n "${lineno}p" "$MCP_FILE")
    if echo "$line_text" | grep -q 'mcp-parity:ok'; then
        continue
    fi

    # Find the enclosing function
    FN_START=$(head -n "$lineno" "$MCP_FILE" \
        | grep -n 'async fn \|pub fn \|fn ' \
        | tail -1 \
        | cut -d: -f1 || echo "1")

    # Find function end (next fn definition or +300 lines)
    FN_END_OFFSET=$(tail -n +"$((lineno + 1))" "$MCP_FILE" \
        | grep -n 'async fn \|pub fn ' \
        | head -1 \
        | cut -d: -f1 || echo "300")
    FN_END=$((lineno + FN_END_OFFSET))

    fn_name=$(sed -n "${FN_START}p" "$MCP_FILE" \
        | grep -oP '(async fn|pub fn|fn) \K[a-z_]+' || echo "unknown")

    # Extract the function body between stream_complete and function end
    POST_LLM_BODY=$(sed -n "${lineno},${FN_END}p" "$MCP_FILE")

    # Check if the function returns tool_result() with the raw text
    HAS_TOOL_RESULT=$(echo "$POST_LLM_BODY" | grep -c 'tool_result(' || true)

    if [ "${HAS_TOOL_RESULT:-0}" -gt 0 ]; then
        # Check if there's JSON parsing between stream_complete and tool_result
        HAS_JSON_PARSING=$(echo "$POST_LLM_BODY" \
            | grep -c 'serde_json::from_str\|serde_json::from_value\|\.as_object()\|\.get("diff")\|\.get("explanation")\|parsed\[' || true)

        if [ "${HAS_JSON_PARSING:-0}" -eq 0 ]; then
            echo ""
            echo "RAW LLM RESPONSE PASSTHROUGH: $MCP_FILE:$lineno (fn $fn_name)"
            echo "  This MCP handler calls stream_complete (LLM call) and returns the raw"
            echo "  text via tool_result() without JSON parsing or validation."
            echo ""
            echo "  The REST handler for the same LLM endpoint parses the raw LLM text,"
            echo "  validates expected fields (e.g., {diff, explanation}), and returns"
            echo "  structured errors on invalid output. The MCP handler must perform the"
            echo "  same validation to maintain HSI §11 parity — otherwise, the consumer"
            echo "  gets raw, unvalidated LLM text instead of structured data."
            echo ""
            echo "  Fix: Parse the raw LLM text as JSON (serde_json::from_str), validate"
            echo "  the expected fields exist, and return a structured tool_result with"
            echo "  the validated data — or tool_error on validation failure."
            echo ""
            echo "  Exempt with '// mcp-parity:ok — <reason>' on the stream_complete line."
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    fi
done

# ── Check 7: REST→MCP parameter drift ───────────────────────────────────
# When a REST endpoint's request struct gains a new field, the corresponding
# MCP tool's inputSchema must also include that parameter. This check
# detects REST fields missing from MCP tool schemas.
#
# Strategy: for known REST-to-MCP mappings, extract the REST struct's pub
# fields and the MCP tool's inputSchema properties, then compare.
#
# Origin: specs/reviews/task-028.md F4 — CreateMrRequest gained depends_on
#         but gyre_create_mr tool schema was not updated.
#
# Exempt with: // mcp-parity:ok — <reason> on the MCP tool schema line.

API_MERGE_FILE="crates/gyre-server/src/api/merge_requests.rs"

# Known REST struct → MCP tool mappings
# Format: "StructName:tool_name:file_path"
MAPPINGS=(
    "CreateMrRequest:gyre_create_mr:$API_MERGE_FILE"
)

for mapping in "${MAPPINGS[@]}"; do
    struct_name=$(echo "$mapping" | cut -d: -f1)
    tool_name=$(echo "$mapping" | cut -d: -f2)
    struct_file=$(echo "$mapping" | cut -d: -f3)

    if [ ! -f "$struct_file" ]; then
        continue
    fi

    # Extract pub fields from the REST request struct.
    # Look for lines between "pub struct $struct_name {" and the next "}"
    # that contain "pub field_name:"
    REST_FIELDS=$(sed -n "/pub struct ${struct_name}/,/^}/p" "$struct_file" \
        | grep -oP 'pub\s+\K[a-z_]+(?=\s*:)' \
        | sort || true)

    if [ -z "$REST_FIELDS" ]; then
        continue
    fi

    # Extract properties from MCP tool's inputSchema.
    # Find the tool definition by name and extract property keys from the
    # subsequent ~30 lines.
    TOOL_LINE=$(grep -n "\"name\": \"${tool_name}\"" "$MCP_FILE" | head -1 | cut -d: -f1 || true)
    if [ -z "$TOOL_LINE" ]; then
        continue
    fi

    TOOL_SCHEMA_END=$((TOOL_LINE + 40))
    MCP_FIELDS=$(sed -n "${TOOL_LINE},${TOOL_SCHEMA_END}p" "$MCP_FILE" \
        | grep -oP '"([a-z_]+)":\s*\{' \
        | grep -oP '"([a-z_]+)"' \
        | tr -d '"' \
        | sort || true)

    if [ -z "$MCP_FIELDS" ]; then
        continue
    fi

    # Compare: find REST fields not in MCP schema
    for rest_field in $REST_FIELDS; do
        if ! echo "$MCP_FIELDS" | grep -qw "$rest_field"; then
            # Check for exemption on the tool schema line
            EXEMPT=$(sed -n "${TOOL_LINE},${TOOL_SCHEMA_END}p" "$MCP_FILE" \
                | grep -c 'mcp-parity:ok' || true)
            if [ "${EXEMPT:-0}" -gt 0 ]; then
                continue
            fi

            echo ""
            echo "REST→MCP PARAMETER DRIFT: $MCP_FILE (tool: $tool_name)"
            echo "  REST struct $struct_name in $struct_file has field '$rest_field'"
            echo "  but MCP tool '$tool_name' inputSchema does not include it."
            echo ""
            echo "  When a REST endpoint gains a parameter, the corresponding MCP tool"
            echo "  must be updated to include the same parameter to maintain HSI §11"
            echo "  REST-MCP parity. MCP callers cannot access the new functionality."
            echo ""
            echo "  Fix: add '$rest_field' to the tool's inputSchema.properties and"
            echo "  wire it through the MCP handler."
            echo ""
            echo "  Exempt with '// mcp-parity:ok — <reason>' in the tool schema block."
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done
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
