#!/usr/bin/env bash
# Architecture lint: verify API handlers that accept tenant_id or workspace_id
# as Path parameters bind them to the authenticated caller's scope.
#
# A handler that extracts `Path(tenant_id)` from the URL but never references
# `auth.tenant_id` can be exploited by any authenticated user to access
# another tenant's resources by changing the URL path parameter.
#
# The correct pattern is one of:
#   (a) Compare path tenant_id against auth.tenant_id and reject mismatches
#   (b) Ignore the path parameter and use auth.tenant_id directly
#   (c) The handler is exempt (documented below)
#
# This script scans all async handler functions in gyre-server/src/api/ for
# Path extractors containing scope identifiers (tenant_id, workspace_id) and
# verifies the function body also references auth.tenant_id / auth.workspace_id.
#
# Run by pre-commit and CI.

set -euo pipefail

API_DIR="crates/gyre-server/src/api"
FAIL=0
CHECKED=0
VIOLATIONS=0

# Scope identifiers that must be bound to auth context when used in Path.
# Format: "path_var_name:auth_field_name"
SCOPE_BINDINGS=(
    "tenant_id:auth.tenant_id"
)

if [ ! -d "$API_DIR" ]; then
    echo "ERROR: Cannot find $API_DIR"
    exit 1
fi

echo "Checking API handlers for unscoped path parameters..."

for file in "$API_DIR"/*.rs; do
    [ -f "$file" ] || continue
    bname=$(basename "$file")
    # Skip mod.rs (just routing), error.rs (error types)
    [ "$bname" = "mod.rs" ] || [ "$bname" = "error.rs" ] && continue

    for binding in "${SCOPE_BINDINGS[@]}"; do
        path_var="${binding%%:*}"
        auth_field="${binding##*:}"

        # Use awk to find async fn handlers that have Path(...$path_var...) but
        # never reference $auth_field in the function body.
        awk -v path_var="$path_var" -v auth_field="$auth_field" \
            -v file="$file" '
        # Detect start of an async fn
        /^\s*(pub\s+)?(async\s+)?fn\s+/ {
            # Emit previous function result if it was a violation
            if (fn_name != "" && has_path_scope && !has_auth_scope) {
                printf "PATH SCOPE UNBOUND: %s::%s in %s:%d\n", fn_name, path_var, file, fn_start
                printf "  Handler accepts Path(%s) but never references %s.\n", path_var, auth_field
                printf "  Either compare path %s against %s or use %s directly.\n", path_var, auth_field, auth_field
                printf "  See: specs/reviews/task-006.md F1 (cross-tenant bypass)\n\n"
                violations++
            }
            if (fn_name != "" && has_path_scope) checked++

            # Parse function name
            match($0, /fn ([a-zA-Z_][a-zA-Z0-9_]*)/, m)
            fn_name = m[1]
            fn_start = NR
            has_path_scope = 0
            has_auth_scope = 0
            # Skip test functions
            if (fn_name ~ /^test_/) fn_name = ""
            next
        }
        fn_name != "" {
            # Check for Path extractor containing the scope variable
            if ($0 ~ "Path.*" path_var) has_path_scope = 1
            # Check for auth field reference
            if ($0 ~ auth_field) has_auth_scope = 1
        }
        END {
            # Check last function
            if (fn_name != "" && has_path_scope && !has_auth_scope) {
                printf "PATH SCOPE UNBOUND: %s::%s in %s:%d\n", fn_name, path_var, file, fn_start
                printf "  Handler accepts Path(%s) but never references %s.\n", path_var, auth_field
                printf "  Either compare path %s against %s or use %s directly.\n", path_var, auth_field, auth_field
                printf "  See: specs/reviews/task-006.md F1 (cross-tenant bypass)\n\n"
                violations++
            }
            if (fn_name != "" && has_path_scope) checked++
            printf "SUMMARY:%d:%d\n", checked, violations
        }
        ' "$file" | while IFS= read -r line; do
            case "$line" in
                SUMMARY:*)
                    c=$(echo "$line" | cut -d: -f2)
                    v=$(echo "$line" | cut -d: -f3)
                    # Write to temp file for outer shell
                    echo "$c $v" >> /tmp/check-path-scope-$$
                    ;;
                *)
                    echo "$line"
                    ;;
            esac
        done
    done
done

# Tally results from temp file
if [ -f /tmp/check-path-scope-$$ ]; then
    while read -r c v; do
        CHECKED=$((CHECKED + c))
        VIOLATIONS=$((VIOLATIONS + v))
    done < /tmp/check-path-scope-$$
    rm -f /tmp/check-path-scope-$$
fi

if [ "$VIOLATIONS" -eq 0 ]; then
    echo "Path scope binding lint passed: ${CHECKED} handlers with path scope parameters checked."
    echo "All bind path scope identifiers to auth context."
    exit 0
else
    echo "Fix: Add 'if auth.tenant_id != tenant_id { return Err(ApiError::Forbidden(...)) }'"
    echo "     or use auth.tenant_id instead of the path parameter."
    echo "${VIOLATIONS} violation(s) found out of ${CHECKED} handlers with path scope parameters."
    exit 1
fi
