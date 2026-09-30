#!/usr/bin/env bash
# Architecture lint: every registered /api/v1/ route must appear in the ABAC
# ResourceResolver registry (or be explicitly exempted).
#
# The ABAC middleware (abac_middleware.rs) resolves routes by EXACT pattern
# match against a hardcoded `routes` vec. Unmatched patterns fall through with
# NO policy evaluation — an unregistered route is a cross-tenant authorization
# gap, even though require_auth still runs. Historically nothing enforced this:
# the middleware's fallthrough comment claims "check-api-auth.sh catches
# missing entries at CI time," but check-api-auth.sh only checks middleware
# chain presence (task-095 review F2: 5 recovery endpoints — and 58 total
# routes — were unregistered at review time).
#
# This script closes that loop:
#   1. Extract every "/api/v1/..." path literal from api/mod.rs (the router).
#   2. Extract every "/api/v1/..." path literal from abac_middleware.rs
#      (the resolver registry, including RouteResourceMapping::exempt entries).
#   3. Fail on any router path present in NEITHER the registry NOR
#      scripts/abac-route-registry-exemptions.txt.
#
# Exemptions are legacy debt, not approval: the file is seeded with the routes
# that predate this check. It should SHRINK as routes are added to the
# resolver — never grow. Do not add new routes to the exemption file.
#
# Run by pre-commit and CI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
API_MOD="crates/gyre-server/src/api/mod.rs"
ABAC="crates/gyre-server/src/abac_middleware.rs"
EXEMPTIONS_FILE="$SCRIPT_DIR/abac-route-registry-exemptions.txt"

FAIL=0

if [ ! -f "$API_MOD" ]; then
    echo "FAIL: $API_MOD not found (run from repo root)"
    exit 1
fi
if [ ! -f "$ABAC" ]; then
    echo "FAIL: $ABAC not found (run from repo root)"
    exit 1
fi

# ── Extract route patterns ──────────────────────────────────────────────

# Router paths: every /api/v1/ string literal in api/mod.rs. Path literals
# always appear on their own line inside .route(...) calls (single- or
# multi-line forms), so a plain literal scan is reliable.
grep -o '"/api/v1/[^"]*"' "$API_MOD" | tr -d '"' | sort -u > /tmp/.abac-router-paths.$$

# Registry patterns: every /api/v1/ string literal in the resolver's routes
# vec (includes exempt entries — an exempt mapping still resolves, so it
# covers the route).
grep -o '"/api/v1/[^"]*"' "$ABAC" | tr -d '"' | sort -u > /tmp/.abac-resolver-paths.$$

# Exemptions: one route path per line, # comments allowed.
if [ -f "$EXEMPTIONS_FILE" ]; then
    grep -v '^\s*#' "$EXEMPTIONS_FILE" | grep -v '^\s*$' | sort -u > /tmp/.abac-exempt-paths.$$
else
    : > /tmp/.abac-exempt-paths.$$
fi

# ── Compare ─────────────────────────────────────────────────────────────

# Routes covered by the resolver directly.
comm -23 /tmp/.abac-router-paths.$$ /tmp/.abac-resolver-paths.$$ > /tmp/.abac-uncovered.$$

# Routes covered by exemptions (legacy debt).
comm -23 /tmp/.abac-uncovered.$$ /tmp/.abac-exempt-paths.$$ > /tmp/.abac-missing.$$

MISSING_COUNT=$(wc -l < /tmp/.abac-missing.$$)

# Stale exemptions: exempted routes that ARE now in the resolver (or no
# longer registered at all) should be removed from the exemption file.
STALE_EXEMPTIONS=$(comm -23 /tmp/.abac-exempt-paths.$$ /tmp/.abac-uncovered.$$)

rm -f /tmp/.abac-router-paths.$$ /tmp/.abac-resolver-paths.$$ \
    /tmp/.abac-exempt-paths.$$ /tmp/.abac-uncovered.$$ /tmp/.abac-missing.$$

# ── Result ──────────────────────────────────────────────────────────────

if [ -n "$STALE_EXEMPTIONS" ]; then
    echo "WARN: stale exemptions in $EXEMPTIONS_FILE (route now covered or unregistered — remove the line):"
    echo "$STALE_EXEMPTIONS" | sed 's/^/  /'
fi

if [ "$MISSING_COUNT" -gt 0 ]; then
    echo "FAIL: $MISSING_COUNT registered /api/v1/ route(s) missing from the ABAC route registry:"
    cat /tmp/.abac-missing.$$ 2>/dev/null || true
    echo ""
    echo "Each route registered in api/mod.rs must have a RouteResourceMapping entry"
    echo "in abac_middleware.rs (ResourceResolver::new) — or a RouteResourceMapping::exempt"
    echo "entry if it is intentionally outside ABAC. Unregistered routes fall through"
    echo "the middleware with NO policy evaluation (cross-tenant authorization gap)."
    echo "Do NOT add routes to $EXEMPTIONS_FILE to silence this check."
    exit 1
fi

echo "OK: all registered /api/v1/ routes resolve in the ABAC registry (or are exempted legacy entries)."
