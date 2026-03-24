#!/usr/bin/env bash
# Architecture lint: verify all mutating API handlers have authorization extractors.
#
# Scans api_router() in crates/gyre-server/src/api/mod.rs for POST/PUT/DELETE
# route registrations, then checks each handler function for an auth extractor
# in its signature.
#
# Auth extractors recognized:
#   AdminOnly, RequireDeveloper, RequireAgent, RequireReadOnly, AuthenticatedAgent
#
# Run by pre-commit and CI. On failure, the message includes remediation instructions.

set -euo pipefail

API_MOD="crates/gyre-server/src/api/mod.rs"
API_SRC="crates/gyre-server/src/api"
SERVER_SRC="crates/gyre-server/src"

if [ ! -f "$API_MOD" ]; then
    echo "ERROR: Cannot find $API_MOD"
    exit 1
fi

FAIL=0
CHECKED=0
SKIPPED=0

# Known public/exempt handlers (no auth extractor required)
EXEMPT_HANDLERS=(
    "version_handler"
    "health_handler"
    "healthz_handler"
    "readyz_handler"
    "metrics_handler"
    "openid_configuration"
    "jwks"
    "spa_handler"
    "scim_service_provider_config"
    "scim_schemas"
    "scim_resource_types"
)

is_exempt() {
    local handler="$1"
    for exempt in "${EXEMPT_HANDLERS[@]}"; do
        if [ "$handler" = "$exempt" ]; then
            return 0
        fi
    done
    return 1
}

# Extract handler names from post(), put(), delete() calls in api_router()
# Matches patterns like: post(module::handler_name), put(handler_name), delete(module::handler)
HANDLERS=$(grep -oP '(post|put|delete)\([^)]+\)' "$API_MOD" | \
    grep -oP '\(([^)]+)\)' | \
    tr -d '()' | \
    sed 's/.*:://' | \
    sort -u)

AUTH_EXTRACTORS="AdminOnly\|RequireDeveloper\|RequireAgent\|RequireReadOnly\|AuthenticatedAgent"

for handler in $HANDLERS; do
    if is_exempt "$handler"; then
        SKIPPED=$((SKIPPED + 1))
        continue
    fi

    CHECKED=$((CHECKED + 1))

    # Find the handler function definition across all API source files
    # Look for `async fn handler_name(` and then check for auth extractors
    # in the function signature (may span multiple lines)
    HANDLER_FILE=$(grep -rl "async fn ${handler}" "$API_SRC" "$SERVER_SRC" 2>/dev/null | head -1)

    if [ -z "$HANDLER_FILE" ]; then
        echo "WARNING: Cannot find handler function '${handler}' in $API_SRC or $SERVER_SRC"
        continue
    fi

    # Extract ~15 lines from the function signature (enough to capture multi-line params)
    SIGNATURE=$(grep -A 15 "async fn ${handler}" "$HANDLER_FILE" | head -16)

    if ! echo "$SIGNATURE" | grep -q "$AUTH_EXTRACTORS"; then
        echo "AUTH VIOLATION: Handler '${handler}' in ${HANDLER_FILE} has no authorization extractor"
        echo "  Mutating endpoints (POST/PUT/DELETE) must include one of:"
        echo "    AdminOnly, RequireDeveloper, RequireAgent, RequireReadOnly, or AuthenticatedAgent"
        echo "  See: specs/development/api-conventions.md §6 — Authorization Contract"
        echo ""
        FAIL=1
    fi
done

if [ "$FAIL" -eq 0 ]; then
    echo "API auth lint passed: ${CHECKED} handlers checked, ${SKIPPED} exempt. All have auth extractors."
else
    echo ""
    echo "Fix: Add an appropriate auth extractor to each handler's function signature."
    echo "  Example: async fn my_handler(AdminOnly { .. }: AdminOnly, ...) -> ... { }"
fi

exit "$FAIL"
