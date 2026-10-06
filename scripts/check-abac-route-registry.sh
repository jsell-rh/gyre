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
# The exemption file is FROZEN at the baseline count below. Growing it —
# adding even one new route — is how task-093 F1 slipped through: the two
# orchestrator-spawn routes shipped in 2ae69e97 and were retro-exempted in
# 088e7048, converting a hard CI failure into a silent grandfather. The
# count must never rise; it must shrink as routes are moved into the
# resolver (delete the entry when you register a route).
FROZEN_EXEMPTION_COUNT=53
FROZEN_DUPLICATE_COUNT=17

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

# Duplicate-resolver detection (task-095 R3-F4): resolve() is first-match,
# so every duplicate entry after the first is dead code whose resource
# mapping silently never runs (the duplicate post-merge-gates entry mapped
# to "gate" but the first-match entry mapped it to "repo"). The five
# recovery routes were registered twice until task-095 R3-F4 removed the
# dead duplicates — 17 legacy paths remain duplicated; frozen so the count
# must shrink as duplicates are removed, never grow.
grep -o '"/api/v1/[^"]*"' "$ABAC" | tr -d '"' | sort | uniq -d > /tmp/.abac-dup-paths.$$
DUPLICATE_COUNT=$(wc -l < /tmp/.abac-dup-paths.$$)
if [ "$DUPLICATE_COUNT" -gt "$FROZEN_DUPLICATE_COUNT" ]; then
    echo "FAIL: $DUPLICATE_COUNT duplicate path entries in the ABAC resolver (baseline: $FROZEN_DUPLICATE_COUNT)."
    echo ""
    echo "ResourceResolver::resolve() is first-match, so every duplicate entry"
    echo "after the first is dead — its resource mapping silently never runs"
    echo "(task-095 R3-F4: the duplicate post-merge-gates mapping to \"gate\""
    echo "is shadowed by the earlier entry). Remove the duplicate entries;"
    echo "if you removed all duplicates of a path, the count shrinks — lower"
    echo "FROZEN_DUPLICATE_COUNT to match; never raise it."
    FAIL=1
fi

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
# Freeze enforcement input (task-093 F1): capture before the temp files are removed.
EXEMPT_TOTAL=$(wc -l < /tmp/.abac-exempt-paths.$$)
STALE_EXEMPTIONS=$(comm -23 /tmp/.abac-exempt-paths.$$ /tmp/.abac-uncovered.$$)

rm -f /tmp/.abac-router-paths.$$ /tmp/.abac-resolver-paths.$$ \
    /tmp/.abac-exempt-paths.$$ /tmp/.abac-uncovered.$$ /tmp/.abac-missing.$$

# Freeze enforcement (task-093 F1): the exemption file must never grow.
if [ "$EXEMPT_TOTAL" -gt "$FROZEN_EXEMPTION_COUNT" ]; then
    echo "FAIL: abac-route-registry-exemptions.txt grew to $EXEMPT_TOTAL entries (baseline: $FROZEN_EXEMPTION_COUNT)."
    echo ""
    echo "Adding routes to the exemption file is how specs/reviews/task-093.md F1"
    echo "shipped: new routes landed unregistered, then were retro-exempted so the"
    echo "check stays green. A new route MUST ship with a RouteResourceMapping (or"
    echo "RouteResourceMapping::exempt) in abac_middleware.rs — never an exemption"
    echo "file entry. If you removed an entry from the baseline, lower"
    echo "FROZEN_EXEMPTION_COUNT to the new count; never raise it."
    FAIL=1
fi

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
