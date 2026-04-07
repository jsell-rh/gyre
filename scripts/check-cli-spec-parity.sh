#!/usr/bin/env bash
# CLI Spec Parity Lint: detect common CLI-spec signature drift.
#
# Checks:
# 1. CLI commands whose spec signature shows optional params ([--param])
#    but whose clap struct declares them as required (non-Option<T>).
# 2. CLI commands that require a subcommand for their primary action
#    when the spec defines a flat command.
# 3. CLI parameters that accept raw IDs where the spec says <name>/<slug>.
#
# This script reads spec excerpts from task files and cross-references
# the clap struct in crates/gyre-cli/src/main.rs.
#
# Run during pre-commit and CI. Not exhaustive — supplements the
# implementation checklist item #18.

set -euo pipefail

CLI_MAIN="crates/gyre-cli/src/main.rs"

if [ ! -f "$CLI_MAIN" ]; then
    echo "SKIP: $CLI_MAIN not found"
    exit 0
fi

FAIL=0

# --- Check 1: Required fields that should be Option<T> ---
# These are fields whose spec signature uses [brackets] (optional)
# but the clap struct declares as bare String (required).
#
# Pattern: look for struct fields of type `String` (not Option<String>)
# in command variants where the help text says "Workspace slug" or similar,
# cross-referencing spec-optional params.

# Find non-Option workspace params in CLI command structs.
# The spec universally marks --workspace as optional ([--workspace <slug>]).
# Any `workspace: String` (not Option<String>) in a command variant is a violation.
WORKSPACE_REQUIRED=$(grep -n 'workspace: String' "$CLI_MAIN" 2>/dev/null | grep -v 'Option<String>' | grep -v '//' || true)
if [ -n "$WORKSPACE_REQUIRED" ]; then
    while IFS= read -r line; do
        echo "CLI-SPEC PARITY: --workspace is declared required but spec marks it optional [--workspace <slug>]"
        echo "  $CLI_MAIN:$line"
        echo "  Fix: Change 'workspace: String' to 'workspace: Option<String>'"
        echo ""
        FAIL=1
    done <<< "$WORKSPACE_REQUIRED"
fi

# --- Check 2: Subcommand-required commands that should be flat ---
# Look for commands where a `command: XxxCommands` field exists but the
# spec defines a flat command (no subcommand needed for primary action).
# We check specifically for Inbox since the spec defines `gyre inbox [--flags]`
# as a flat command. The pattern `command: InboxCommands` without a
# `default_subcommand` or `Option<InboxCommands>` forces a subcommand.
INBOX_SUBCOMMAND_REQUIRED=$(grep -n 'command: InboxCommands' "$CLI_MAIN" 2>/dev/null | head -1 || true)
if [ -n "$INBOX_SUBCOMMAND_REQUIRED" ]; then
    # Check if it's wrapped in Option (allowing bare invocation)
    if ! grep -q 'command: Option<InboxCommands>' "$CLI_MAIN" 2>/dev/null; then
        echo "CLI-SPEC PARITY: 'gyre inbox' requires a subcommand but spec defines flat command"
        echo "  $CLI_MAIN:$INBOX_SUBCOMMAND_REQUIRED"
        echo "  Spec: gyre inbox [--workspace <slug>] [--priority <min>-<max>]"
        echo "  Fix: Make subcommand optional or set default_subcommand to 'list'"
        echo ""
        FAIL=1
    fi
fi

# --- Check 3: Help text saying "ID" where spec says name/slug ---
# Catch help strings like "Repository ID" for flags the spec defines as <name>.
REPO_ID_HELP=$(grep -n '"Repository ID"' "$CLI_MAIN" 2>/dev/null || true)
if [ -n "$REPO_ID_HELP" ]; then
    # Check if this is on a --repo flag (where spec says <name>)
    # vs a --repo-id flag (which is explicit about wanting an ID)
    while IFS= read -r line; do
        LINENUM=$(echo "$line" | cut -d: -f1)
        # Check the field name on the next line — if it's `repo:` not `repo_id:`, it's a violation
        FIELD=$(sed -n "$((LINENUM + 1))p" "$CLI_MAIN" 2>/dev/null || true)
        if echo "$FIELD" | grep -q 'repo:' 2>/dev/null && ! echo "$FIELD" | grep -q 'repo_id:' 2>/dev/null; then
            echo "CLI-SPEC PARITY: --repo help says 'Repository ID' but spec says '--repo <name>'"
            echo "  $CLI_MAIN:$line"
            echo "  Fix: Accept a human-readable repo name and resolve to ID (like resolve_workspace_slug)"
            echo ""
            FAIL=1
        fi
    done <<< "$REPO_ID_HELP"
fi

# --- Check 4: --repo-id as required arg where spec doesn't include it ---
# The spec defines `gyre spec assist <path> "<instruction>"` — no --repo-id.
# A mandatory repo_id flag not in the spec is an invented parameter.
SPEC_ASSIST_REPO_ID=$(grep -n 'repo_id: String' "$CLI_MAIN" 2>/dev/null | grep -v 'Option<String>' || true)
if [ -n "$SPEC_ASSIST_REPO_ID" ]; then
    # Only flag if this is inside the SpecCommands/Assist context
    while IFS= read -r line; do
        LINENUM=$(echo "$line" | cut -d: -f1)
        # Check surrounding context for Assist variant
        CONTEXT=$(sed -n "$((LINENUM - 10)),$((LINENUM))p" "$CLI_MAIN" 2>/dev/null || true)
        if echo "$CONTEXT" | grep -q 'Assist' 2>/dev/null; then
            echo "CLI-SPEC PARITY: 'gyre spec assist' has required --repo-id not in spec signature"
            echo "  $CLI_MAIN:$line"
            echo "  Spec: gyre spec assist <path> \"<instruction>\""
            echo "  Fix: Infer repo from context or make --repo-id optional"
            echo ""
            FAIL=1
        fi
    done <<< "$SPEC_ASSIST_REPO_ID"
fi

# --- Check 5: Invented parameter dependencies ---
# Look for bail!/anyhow::bail! messages that say "--X requires --Y" or similar.
# These indicate invented constraints between optional parameters.
# Flag them for manual spec review.
INVENTED_DEPS=$(grep -n 'bail!.*--.*requires' "$CLI_MAIN" 2>/dev/null || true)
if [ -n "$INVENTED_DEPS" ]; then
    while IFS= read -r line; do
        echo "CLI-SPEC PARITY: Possible invented parameter dependency — verify against spec"
        echo "  $CLI_MAIN:$line"
        echo "  If spec defines both parameters as independently optional, this is a violation."
        echo "  Fix: Infer missing context (git remote, config, global search) instead of requiring another flag."
        echo ""
        FAIL=1
    done <<< "$INVENTED_DEPS"
fi

# --- Check 6: Client query params not in server Query extractor structs ---
# Detect query parameter names sent by client.rs that do not appear in any
# query extractor struct in the server API. We target structs whose names
# match *Params or *Query (both naming conventions are used for Axum
# Query<T> extractors), excluding *Response and *Request structs.
# A param that exists in a response struct but not an extractor struct
# is still silently dropped.
CLI_CLIENT="crates/gyre-cli/src/client.rs"
SERVER_API_DIR="crates/gyre-server/src/api"

if [ -f "$CLI_CLIENT" ] && [ -d "$SERVER_API_DIR" ]; then
    # Extract query param names from client.rs: .query(&[("param_name", ...)])
    CLIENT_PARAMS=$(grep -oP '\.query\(&\[\("(\K[^"]+)' "$CLI_CLIENT" 2>/dev/null | sort -u || true)

    if [ -n "$CLIENT_PARAMS" ]; then
        # Extract field names from *Params and *Query structs.
        # Strategy: find "struct Xxx(Params|Query)" blocks, then extract pub field
        # names within the next ~20 lines (until closing brace).
        PARAMS_FIELDS=""
        while IFS= read -r params_line; do
            PARAMS_FILE=$(echo "$params_line" | cut -d: -f1)
            PARAMS_LINENUM=$(echo "$params_line" | cut -d: -f2)
            # Extract pub field names from the struct body — stop at closing brace.
            FIELDS=$(sed -n "${PARAMS_LINENUM},\$p" "$PARAMS_FILE" 2>/dev/null | \
                sed '/^}/q' | \
                grep -oP 'pub\s+\K\w+(?=\s*:)' || true)
            if [ -n "$FIELDS" ]; then
                PARAMS_FIELDS="${PARAMS_FIELDS}${FIELDS}"$'\n'
            fi
        done < <(grep -rn 'struct \w*\(Params\|Query\)\b' "$SERVER_API_DIR"/*.rs 2>/dev/null | \
            grep -v 'Request\|Response' || true)

        PARAMS_FIELDS=$(echo "$PARAMS_FIELDS" | sort -u)

        while IFS= read -r param; do
            [ -z "$param" ] && continue
            if ! echo "$PARAMS_FIELDS" | grep -qx "$param" 2>/dev/null; then
                echo "CLI-SPEC PARITY: Client sends query param '$param' but no server Query extractor struct has this field"
                echo "  $CLI_CLIENT: .query(&[(\"$param\", ...)])"
                echo "  The server will silently ignore this parameter — results will be wrong/unfiltered."
                echo "  Fix: Add '$param' field to the appropriate server Query/Params struct."
                echo ""
                FAIL=1
            fi
        done <<< "$CLIENT_PARAMS"
    fi
fi

# --- Check 7: CLI endpoint URL vs server route registration ---
# Detect endpoint URLs in client.rs that don't match any route in mod.rs.
SERVER_MOD="crates/gyre-server/src/api/mod.rs"
if [ -f "$CLI_CLIENT" ] && [ -f "$SERVER_MOD" ]; then
    # Extract URL path patterns from client.rs (after base_url):
    # e.g., /api/v1/merge-requests/{mr_id}/timeline
    CLIENT_ENDPOINTS=$(grep -oP '"\{\}/api/v1/\K[^"]+' "$CLI_CLIENT" 2>/dev/null | \
        sed 's/{[^}]*}/:param/g' | sort -u || true)

    if [ -n "$CLIENT_ENDPOINTS" ]; then
        # Extract registered routes from mod.rs
        SERVER_ROUTES=$(grep -oP '"/api/v1/\K[^"]+' "$SERVER_MOD" 2>/dev/null | \
            sed 's/:[^/]*/:param/g' | sort -u || true)

        while IFS= read -r endpoint; do
            [ -z "$endpoint" ] && continue
            if ! echo "$SERVER_ROUTES" | grep -qxF "$endpoint" 2>/dev/null; then
                # Get the original line for context
                ORIG=$(grep -n "api/v1/$endpoint" "$CLI_CLIENT" 2>/dev/null | head -1 || true)
                # Undo param substitution for display
                echo "CLI-SPEC PARITY: Client calls endpoint path that doesn't match any server route"
                echo "  $CLI_CLIENT: /api/v1/$endpoint"
                echo "  No matching route found in $SERVER_MOD"
                echo "  Fix: Verify the endpoint URL against the spec and server route registration."
                echo ""
                FAIL=1
            fi
        done <<< "$CLIENT_ENDPOINTS"
    fi
fi

# --- Check 8: POST/PUT/PATCH without JSON body when server expects Json<T> ---
# Detect client POST/PUT/PATCH calls that never chain .json(...) but whose
# server handler expects a Json<T> extractor.  A POST without a body to a
# handler with Json<T> fails at runtime every invocation (400/415).
if [ -f "$CLI_CLIENT" ] && [ -d "$SERVER_API_DIR" ]; then
    # Strategy:
    #  1. Find POST/PUT/PATCH blocks in client.rs (between .post/.put/.patch and .send()).
    #  2. For each block, extract the endpoint path.
    #  3. Check if the block includes .json( — if not, look up the server handler.
    #  4. If the handler has Json<T> in its signature, flag it.

    # Extract method+endpoint pairs for POST/PUT/PATCH calls that lack .json(
    # We look for sequences like:
    #   .post(format!("{}/api/v1/...", ...))
    #   ...
    #   .send()
    # where no .json( appears between the HTTP method and .send().

    # Use awk to find POST/PUT/PATCH blocks without .json(
    # Handles multi-line format!() blocks where the endpoint path is on a
    # different line than the .post() call.
    BODYLESS_POSTS=$(awk '
        /\.(post|put|patch)\(/ {
            method_line = NR
            in_block = 1
            has_json = 0
            endpoint = ""
            collecting_ep = 1
        }
        in_block && collecting_ep {
            if (match($0, /api\/v1\/[^"]+/)) {
                endpoint = substr($0, RSTART, RLENGTH)
                collecting_ep = 0
            }
        }
        in_block && /\.json\(/ { has_json = 1 }
        in_block && /\.send\(/ {
            if (!has_json && endpoint != "") {
                print method_line ":" endpoint
            }
            in_block = 0
            endpoint = ""
            collecting_ep = 0
        }
    ' "$CLI_CLIENT" 2>/dev/null || true)

    if [ -n "$BODYLESS_POSTS" ]; then
        while IFS= read -r entry; do
            [ -z "$entry" ] && continue
            LINE=$(echo "$entry" | cut -d: -f1)
            EP=$(echo "$entry" | cut -d: -f2-)

            # Derive the handler function name from the route.
            # Normalize the client endpoint to match the server route format:
            #   client: api/v1/notifications/{id}/resolve
            #   server: /api/v1/notifications/:id/resolve
            ROUTE_PATTERN=$(echo "$EP" | sed 's|{[^}]*}|:[^/]*|g')

            # Search server API files for a handler with Json< in its signature
            # that matches this endpoint path.
            # Look for the route in mod.rs first to find the handler name.
            # Find the line number of the matching route, then look at the next
            # line for the handler name (Axum style: .route("path", post(handler))).
            ROUTE_LINENUM=$(grep -n "$ROUTE_PATTERN" "$SERVER_MOD" 2>/dev/null | head -1 | cut -d: -f1 || true)
            HANDLER=""
            if [ -n "$ROUTE_LINENUM" ]; then
                # Handler may be on the same line or the next line
                HANDLER=$(sed -n "${ROUTE_LINENUM},$((ROUTE_LINENUM + 2))p" "$SERVER_MOD" 2>/dev/null | \
                    grep -oP '(post|put|patch|delete|get)\(\K\w+' | head -1 || true)
            fi

            if [ -n "$HANDLER" ]; then
                # Check if the handler function signature includes Json<
                HAS_JSON_EXTRACTOR=$(grep -A 5 "fn $HANDLER" "$SERVER_API_DIR"/*.rs 2>/dev/null | grep 'Json<' || true)
                if [ -n "$HAS_JSON_EXTRACTOR" ]; then
                    echo "CLI-SPEC PARITY: Client POST/PUT/PATCH sends no JSON body but server handler expects Json<T>"
                    echo "  $CLI_CLIENT:$LINE: /$EP"
                    echo "  Server handler '$HANDLER' requires a Json<T> extractor — request will fail at runtime (400/415)."
                    echo "  Fix: Add .json(&serde_json::json!({})) or .json(&payload) to the request builder."
                    echo ""
                    FAIL=1
                fi
            fi
        done <<< "$BODYLESS_POSTS"
    fi
fi

if [ "$FAIL" -eq 0 ]; then
    echo "CLI-spec parity lint passed."
fi

exit "$FAIL"
