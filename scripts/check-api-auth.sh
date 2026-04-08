#!/usr/bin/env bash
# Architecture lint: verify the API authorization model is structurally sound.
#
# The auth model has two tiers:
#
# 1. **ABAC middleware** — applied to api_router() via layered middleware in
#    build_router() (lib.rs). All /api/v1/ routes go through:
#      require_auth_middleware → last_seen_middleware → abac_middleware
#    This is the primary auth enforcement mechanism (since M34 Slice 4).
#
# 2. **Per-handler auth** — routes mounted OUTSIDE the ABAC-protected api merge
#    (WebSocket, git HTTP, MCP, conversations) enforce auth via AuthenticatedAgent
#    extractors in their handler signatures.
#
# This script verifies:
#   Check 1: The ABAC middleware chain is present on the api router in build_router().
#   Check 2: Non-ABAC POST/PUT/DELETE routes have per-handler auth extractors.
#
# Run by pre-commit and CI.

set -euo pipefail

LIB_RS="crates/gyre-server/src/lib.rs"
SERVER_SRC="crates/gyre-server/src"
FAIL=0

if [ ! -f "$LIB_RS" ]; then
    echo "ERROR: Cannot find $LIB_RS"
    exit 1
fi

# ── Check 1: ABAC middleware chain structural presence ──────────────────

echo "Check 1: Verifying ABAC middleware chain on api router..."

# The build_router function must apply all three middleware layers to api_router().
# We check that the build_router function (or the section that creates the `api` binding)
# contains all three middleware references.

# Extract the section from api_router() creation to the .merge(api) call
API_SECTION=$(sed -n '/let api = api::api_router()/,/\.merge(api)/p' "$LIB_RS")

if [ -z "$API_SECTION" ]; then
    echo "FAIL: Cannot find 'let api = api::api_router()...merge(api)' pattern in $LIB_RS"
    echo "  The api_router must be wrapped with ABAC middleware layers."
    FAIL=1
else
    MISSING_LAYERS=()

    if ! echo "$API_SECTION" | grep -q 'require_auth_middleware'; then
        MISSING_LAYERS+=("require_auth_middleware")
    fi

    if ! echo "$API_SECTION" | grep -q 'last_seen_middleware'; then
        MISSING_LAYERS+=("last_seen_middleware")
    fi

    if ! echo "$API_SECTION" | grep -q 'abac_middleware'; then
        MISSING_LAYERS+=("abac_middleware")
    fi

    if [ ${#MISSING_LAYERS[@]} -gt 0 ]; then
        echo "FAIL: ABAC middleware chain incomplete on api router."
        echo "  Missing layers: ${MISSING_LAYERS[*]}"
        echo "  Required chain: require_auth_middleware → last_seen_middleware → abac_middleware"
        echo "  See: $LIB_RS (build_router function)"
        FAIL=1
    else
        echo "  OK: All three middleware layers present (require_auth, last_seen, abac)."
    fi
fi

# ── Check 2: Non-ABAC mutating routes have per-handler auth ────────────

echo "Check 2: Verifying per-handler auth on non-ABAC routes..."

# Routes mounted directly on the outer Router (not through .merge(api)) that use
# post/put/delete must have AuthenticatedAgent in their handler signature.
# We extract these from the build_router function OUTSIDE the api section.

# Known public/exempt handlers (no auth required)
EXEMPT_HANDLERS=(
    "health_handler"
    "healthz_handler"
    "readyz_handler"
    "metrics_handler"
    "openid_configuration"
    "jwks"
    "spa_handler"
    "version_handler"
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

# Extract the outer Router section (after .merge(api), which is the non-ABAC routes)
# Also include routes BEFORE the api merge (between Router::new() and .merge(api))
OUTER_ROUTES=$(sed -n '/Router::new()/,/\.with_state/p' "$LIB_RS" | \
    grep -E '(post|put|delete)\(' || true)

AUTH_EXTRACTORS="AdminOnly\|RequireDeveloper\|RequireAgent\|RequireReadOnly\|AuthenticatedAgent"
CHECKED=0
SKIPPED=0

for line in $( echo "$OUTER_ROUTES" | grep -oP '(post|put|delete)\([^)]+\)' | \
    grep -oP '\(([^)]+)\)' | tr -d '()' | sed 's/.*:://' | sort -u ); do

    handler="$line"

    if is_exempt "$handler"; then
        SKIPPED=$((SKIPPED + 1))
        continue
    fi

    CHECKED=$((CHECKED + 1))

    # Find the handler function definition
    HANDLER_FILE=$(grep -rl "async fn ${handler}" "$SERVER_SRC" 2>/dev/null | head -1)

    if [ -z "$HANDLER_FILE" ]; then
        echo "  WARNING: Cannot find handler function '${handler}' in $SERVER_SRC"
        continue
    fi

    # Extract ~15 lines from the function signature
    SIGNATURE=$(grep -A 15 "async fn ${handler}" "$HANDLER_FILE" | head -16)

    if ! echo "$SIGNATURE" | grep -q "$AUTH_EXTRACTORS"; then
        echo "  AUTH VIOLATION: Non-ABAC handler '${handler}' in ${HANDLER_FILE} has no auth extractor"
        echo "    Routes outside the ABAC-protected api router must include AuthenticatedAgent"
        echo "    in their function signature for per-handler auth enforcement."
        echo ""
        FAIL=1
    fi
done

if [ "$CHECKED" -gt 0 ] && [ "$FAIL" -eq 0 ]; then
    echo "  OK: ${CHECKED} non-ABAC handlers checked, ${SKIPPED} exempt. All have auth extractors."
elif [ "$CHECKED" -eq 0 ]; then
    echo "  OK: No non-ABAC mutating handlers found (${SKIPPED} exempt)."
fi

# ── Result ──────────────────────────────────────────────────────────────

if [ "$FAIL" -eq 0 ]; then
    echo ""
    echo "API auth lint passed."
else
    echo ""
    echo "Fix: Ensure the ABAC middleware chain is applied to api_router() and"
    echo "     non-ABAC mutating handlers have AuthenticatedAgent extractors."
    exit 1
fi
