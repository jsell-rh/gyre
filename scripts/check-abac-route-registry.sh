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
FROZEN_EXEMPTION_COUNT=46
# Baseline is the REAL resolver duplicate count in the non-test region of
# abac_middleware.rs (task-171 review: the whole-file grep counted test
# route fixtures as resolver duplicates, tripping this guard on every run
# while the FAIL was invisible because the exit code ignored it). After
# the rebase onto main, 2 paths are flagged; both are false positives —
# the `/// Axum route pattern` doc-comment example (/api/v1/tasks/:id)
# and the gate-agent allowlist consts REVIEWER_ALLOWED_ROUTES /
# VALIDATOR_ALLOWED_ROUTES (/api/v1/version) — there are no real
# duplicate resolver entries. Frozen so the count must shrink as the
# scan tightens or duplicates are removed, never grow.
FROZEN_DUPLICATE_COUNT=2

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
#
# Scan only the PRODUCTION region of abac_middleware.rs (everything before
# the #[cfg(test)] module): test fixtures register routers whose path
# literals are not resolver entries, and counting them both inflates the
# duplicate baseline and can mask a genuinely unregistered route as
# "covered" (task-171 review finding). The stripping is guarded below so a
# misplaced #[cfg(test)] can never silently gut the scan.
sed '/^#\[cfg(test)\]/,$d' "$ABAC" > /tmp/.abac-resolver-region.$$ 2>/dev/null
# Sanity guard: the stripped region must still contain the resolver. If a
# #[cfg(test)] block ever moves above the resolver vec, fail loudly instead
# of scanning an empty region (which would make every route look missing).
if ! grep -q 'RouteResourceMapping' /tmp/.abac-resolver-region.$$; then
    echo "FAIL: $ABAC production region (before #[cfg(test)]) contains no RouteResourceMapping — the test-region strip gutted the scan. Fix the script's region detection."
    rm -f /tmp/.abac-resolver-region.$$
    exit 1
fi
grep -o '"/api/v1/[^"]*"' /tmp/.abac-resolver-region.$$ | tr -d '"' | sort -u > /tmp/.abac-resolver-paths.$$
rm -f /tmp/.abac-resolver-region.$$

# Duplicate-resolver detection (task-095 R3-F4 cosmetic sub-finding): the
# five recovery routes are registered twice in the resolver — the first
# match wins in `resolve()`, so every later duplicate entry is dead code,
# including its resource mapping (the duplicate post-merge-gates entry
# maps to "gate" but the first-match entry maps it differently). Frozen so
# the count must shrink as duplicates are removed, never grow. Same
# non-test region as the coverage scan — test route fixtures are not
# resolver entries.
sed '/^#\[cfg(test)\]/,$d' "$ABAC" | grep -o '"/api/v1/[^"]*"' | tr -d '"' | sort | uniq -d > /tmp/.abac-dup-paths.$$
DUPLICATE_COUNT=$(wc -l < /tmp/.abac-dup-paths.$$)
if [ "$DUPLICATE_COUNT" -gt "$FROZEN_DUPLICATE_COUNT" ]; then
    echo "FAIL: $DUPLICATE_COUNT duplicate path entries in the ABAC resolver (baseline: $FROZEN_DUPLICATE_COUNT)."
    echo ""
    echo "ResourceResolver::resolve() is first-match, so every duplicate entry"
    echo "after the first is dead — its resource mapping silently never runs"
    echo "(task-095 R3-F4: the duplicate post-merge-gates entries disagreed on"
    echo "resource type — only the first-match mapping ever ran). Remove the duplicate entries;"
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
# Capture before the temp files are removed — the MISSING branch below
# prints the list after `rm -f` (task-171: the old `cat` of the deleted
# file always printed nothing, hiding which routes were missing).
MISSING_PATHS=$(cat /tmp/.abac-missing.$$)

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
    echo "$MISSING_PATHS" | sed '/^$/d' | sed 's/^/  /'
    echo ""
    echo "Each route registered in api/mod.rs must have a RouteResourceMapping entry"
    echo "in abac_middleware.rs (ResourceResolver::new) — or a RouteResourceMapping::exempt"
    echo "entry if it is intentionally outside ABAC. Unregistered routes fall through"
    echo "the middleware with NO policy evaluation (cross-tenant authorization gap)."
    echo "Do NOT add routes to $EXEMPTIONS_FILE to silence this check."
    exit 1
fi

# Honor the FAIL flag (task-171 review: the duplicate-count and
# exemption-count branches set FAIL=1 but the script unconditionally
# printed OK and exited 0, so a tripped guard was invisible to pre-commit
# and CI — engineers learned to ignore the FAIL line).
if [ "$FAIL" -ne 0 ]; then
    exit 1
fi

echo "OK: all registered /api/v1/ routes resolve in the ABAC registry (or are exempted legacy entries)."
