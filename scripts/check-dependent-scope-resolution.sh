#!/usr/bin/env bash
# Architecture lint: verify that functions iterating over dependent entities
# resolve each entity's scope (workspace_id) from the entity's own record,
# not from the function's workspace_id parameter.
#
# The anti-pattern: a function receives workspace_id as a parameter, then
# inside a `for` loop over dependent entities, uses that same workspace_id
# to create tasks, send notifications, or target MCP broadcasts. The correct
# pattern is to look up each dependent entity's workspace via a repo lookup
# (e.g., state.repos.find_by_id) inside the loop.
#
# This flaw is invisible in same-workspace deployments but causes tasks to
# appear in the wrong workspace, notifications to reach the wrong people,
# and MCP broadcasts to miss the correct orchestrators in multi-workspace
# tenants with cross-workspace dependencies.
#
# Run by pre-commit and CI.

set -euo pipefail

SERVER_DIR="crates/gyre-server/src"
VIOLATIONS=0
CHECKED=0

if [ ! -d "$SERVER_DIR" ]; then
    echo "ERROR: Cannot find $SERVER_DIR"
    exit 1
fi

echo "Checking for caller-scope propagation in dependent-entity loops..."

for file in $(find "$SERVER_DIR" -name '*.rs' -not -path '*/tests/*'); do
    [ -f "$file" ] || continue

    # Find all non-test function definitions
    fn_lines=$(grep -nE '^\s*(pub(\([a-z]+\))?\s+)?(async\s+)?fn\s+' "$file" 2>/dev/null | grep -v 'test_' | cut -d: -f1 || true)
    [ -z "$fn_lines" ] && continue

    total_lines=$(wc -l < "$file")
    fn_line_array=($fn_lines)
    fn_count=${#fn_line_array[@]}

    for idx in $(seq 0 $((fn_count - 1))); do
        fn_line=${fn_line_array[$idx]}

        # Get function name
        fn_name=$(sed -n "${fn_line}p" "$file" | grep -oE 'fn\s+[a-zA-Z_][a-zA-Z0-9_]*' | sed 's/fn //' || true)
        [ -z "$fn_name" ] && continue

        # Check if workspace_id appears in the parameter block (next 15 lines)
        param_block=$(sed -n "${fn_line},$((fn_line + 15))p" "$file")
        if ! echo "$param_block" | grep -qE 'workspace_id\s*:'; then
            continue
        fi

        # Determine function end: next fn definition or end of file
        if [ "$idx" -lt $((fn_count - 1)) ]; then
            next_fn_line=${fn_line_array[$((idx + 1))]}
            fn_end=$((next_fn_line - 1))
        else
            fn_end=$total_lines
        fi

        # Extract function body into a temp file to avoid argument length issues
        fn_body_file=$(mktemp)
        sed -n "${fn_line},${fn_end}p" "$file" > "$fn_body_file"

        CHECKED=$((CHECKED + 1))

        # Check for exemption
        if grep -q 'caller-scope:ok' "$fn_body_file"; then
            rm -f "$fn_body_file"
            continue
        fi

        # Check if function has a for loop
        if ! grep -qE 'for\s+\w+\s+in\s+' "$fn_body_file"; then
            rm -f "$fn_body_file"
            continue
        fi

        # Check if workspace_id is used for side-effect construction
        has_ws_usage=0
        grep -qE 'Id::new\(workspace_id\)|Id::new\(&workspace_id\)' "$fn_body_file" && has_ws_usage=1
        grep -qE 'list_by_workspace.*workspace_id' "$fn_body_file" && has_ws_usage=1
        grep -qE 'Destination::Workspace.*workspace_id' "$fn_body_file" && has_ws_usage=1

        if [ "$has_ws_usage" -eq 0 ]; then
            rm -f "$fn_body_file"
            continue
        fi

        # Check if there's a per-entity scope lookup (the correct pattern)
        has_entity_lookup=0
        grep -qE 'find_by_id|dep_workspace|entity_workspace|target_workspace|dependent.*workspace' "$fn_body_file" && has_entity_lookup=1
        # Check for reading .workspace_id from an entity (not assigning to it).
        # Assignment pattern: .workspace_id = (anti-pattern, LHS of assignment)
        # Read pattern: .workspace_id. or .workspace_id) or .workspace_id, (RHS usage)
        grep -qP '\.workspace_id\s*(?!\s*=)[\.\)\,\;]' "$fn_body_file" && has_entity_lookup=1

        rm -f "$fn_body_file"

        if [ "$has_entity_lookup" -eq 0 ]; then
            echo "CALLER-SCOPE PROPAGATION: $fn_name in $file:$fn_line"
            echo "  Function receives workspace_id parameter and uses it directly"
            echo "  for side-effect construction (tasks/notifications/broadcasts)"
            echo "  without resolving each dependent entity's own workspace."
            echo "  Fix: Inside the loop, look up the dependent entity's workspace_id"
            echo "  via state.repos.find_by_id(&entity_id) instead of using the"
            echo "  function parameter."
            echo ""
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done
done

if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Dependent scope resolution lint passed: ${CHECKED} functions with workspace_id param checked."
    echo "All resolve entity scope from entity records, not caller parameters."
    exit 0
else
    echo "Fix: Inside the loop, resolve each dependent entity's workspace_id from its"
    echo "     own repo/entity record (e.g., state.repos.find_by_id(&dep.repo_id))."
    echo "     Use the function parameter only as a fallback."
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} functions with workspace_id params."
    exit 1
fi
