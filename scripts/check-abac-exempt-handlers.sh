#!/usr/bin/env bash
# check-abac-exempt-handlers.sh — Verify that every ABAC-exempt route's
# handler performs real per-handler authorization (or is intentionally
# public), and that its doc comment does not defer specced enforcement or
# claim an unverified mitigation.
#
# Procedural background (specs/reviews/task-087.md F1b/F1c): the
# `GET /api/v1/trace-spans/:span_id/payload` route was ABAC-exempted on the
# promise of per-handler auth ("resolves the span's MR → workspace for
# authorization", per spec HSI §3a). The handler's only check was
# `_auth: AuthenticatedAgent` — no entity load, no tenant comparison, no
# Forbidden branch — so any authenticated agent in any workspace/tenant
# could read any span's full request/response payloads. The doc comment
# admitted the deferral ("deferred to a follow-up") AND claimed a
# mitigation that does not exist ("the storage layer is tenant-scoped so
# cross-tenant access returns None naturally" — the shared AppState
# repository instance is not tenant-scoped; zero `with_tenant()` calls
# exist).
#
# The fix-class rules this script enforces:
#
#   Rule 1 — an exempt handler that names an `auth`/`AuthenticatedAgent`
#   parameter (bound OR underscore-prefixed) MUST perform a real
#   authorization decision in its body: either
#     (a) call an auth-check helper (`check_*_auth`, `authorize`,
#         `verify_access`, `require_*`), or
#     (b) compare the loaded entity's scope against the caller's
#         (`tenant_id`/`workspace_id` from `auth.`), returning
#         Forbidden/NotFound on mismatch.
#   Rule 2 — a handler body containing a deferral phrase
#   ("deferred to a follow-up", "for now", "TODO.*auth", "not enforced")
#   in a doc comment about authorization is a finding: specced
#   enforcement must ship in the handler, not in a comment.
#   Rule 3 — mitigation claims ("naturally", "scoped so .* returns
#   None", "cannot be accessed without") in an auth doc comment must be
#   backed by a call into the repository layer that actually scopes
#   (`with_tenant`, `tenant-scoped` lookup taking the caller's tenant);
#   an unverifiable prose claim is a finding.
#
# Known intentionally-public exempt routes (version info, SCIM discovery
# docs) are listed in the PUBLIC_EXEMPT array below; SCIM user routes
# authenticate via the SCIM bearer token (check_scim_auth).
#
# Extension (specs/reviews/task-093.md F2/F3): routes listed in
# scripts/abac-route-registry-exemptions.txt are treated EXACTLY like
# RouteResourceMapping::exempt routes — they run with NO middleware policy
# evaluation, so the handler body is the only authorization surface, and the
# same per-handler rules apply. Two routes reached the exemption file by
# shipping unregistered and being retro-exempted (task-093 F1), then ran
# with broken handler checks: `spawn_workspace_orchestrator` consulted only
# `auth.agent_id` (any principal in any tenant could mint a
# workspace-scoped JWT), and `spawn_repo_orchestrator` relied on
# `check_repo_abac`, which returns Ok(()) when no per-repo policies exist
# ("No policies = unrestricted") and never verifies tenant containment.
# Rule 1b is correspondingly tightened: referencing `auth.agent_id` alone
# (an identity string, not a scope) is NOT an authorization decision —
# using it as `spawned_by` bookkeeping leaves every cross-tenant request
# authorized. Scope fields are tenant_id / workspace_id / user_id / role.
# Rule 4 (new): an ABAC-policy helper call (`check_repo_abac` etc.) is not
# a substitute for tenant containment — the policy engine's default is
# permissive when no policies are stored. The handler must compare the
# loaded entity's tenant/workspace against the caller's regardless.
#
# NOT flagged: handlers inside #[cfg(test)]/mod tests regions; lines
# carrying `// exempt-auth:ok`.
#
# Exempt a line with: `// exempt-auth:ok — <reason>`
#
# Usage: bash scripts/check-abac-exempt-handlers.sh [paths...]
#        (default: crates/gyre-server/src)
#
# Run by pre-commit and CI.

set -uo pipefail

if [ $# -eq 0 ]; then
    set -- crates/gyre-server/src
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

GYRE_SCRIPT_DIR="$SCRIPT_DIR" python3 - "$@" <<'PYEOF'
import os
import re
import sys
from pathlib import Path

# Routes exempted in abac_middleware.rs that are intentionally public or
# authenticated by a non-ABAC mechanism that does not resolve a resource.
# Anything NOT here must pass the per-handler auth rules.
PUBLIC_EXEMPT = {
    '/api/v1/version',
    '/scim/v2/ServiceProviderConfig',
    '/scim/v2/Schemas',
    '/scim/v2/ResourceTypes',
}

MW_PATH = Path('crates/gyre-server/src/abac_middleware.rs')

TEST_START = re.compile(r'\s*(#\[[^\]]*\]\s*)?(mod tests|#\[cfg\(test\)\])')

# RouteResourceMapping::exempt("...") and RouteResourceMapping::api("...", ...)
EXEMPT_ROUTE = re.compile(r'RouteResourceMapping::exempt\(\s*"([^"]+)"\s*\)')
API_ROUTE = re.compile(r'RouteResourceMapping::api\(\s*"([^"]+)"')

# Route registrations: .route("/path", get(handler)...) — capture path and handler chain
ROUTE_REG = re.compile(r'\.route\(\s*"([^"]+)"\s*,\s*([a-z]+)\(([^)]+)\)')
# Method router chains like get(a).post(b)
METHOD_CHAIN = re.compile(r'\b(get|post|put|delete|patch)\(\s*([A-Za-z0-9_:]+)\s*\)')

# A parameter named auth-ish, bound or underscore-prefixed (i.e., possibly unused)
AUTH_PARAM = re.compile(r'^(?:pub\s+)?async\s+fn\s+\w+')

DEFERRAL = re.compile(
    r'deferred to a follow-up|for now|not enforced|TODO[^\n]*auth|future (work|version)[^\n]*auth',
    re.IGNORECASE,
)
MITIGATION_CLAIM = re.compile(
    r'(naturally|scoped so|cannot be accessed|already (scop|filter)ed)[^\n]*(None|denied|without)',
    re.IGNORECASE,
)
# Repository-layer scoping calls that can legitimately back a mitigation claim
SCOPING_CALL = re.compile(r'with_tenant\(|list_for_user\(|tenant_id\.eq\(|filter\([^\n]*tenant')

def find_files(paths):
    for p in paths:
        path = Path(p)
        if path.is_dir():
            yield from sorted(path.rglob('*.rs'))
        elif path.suffix == '.rs':
            yield path

def strip_tests(text):
    """Return (lines, test_start_index) where lines after test_start are tests."""
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if TEST_START.match(line):
            return lines, i
    return lines, None

def parse_handler(lines, name):
    """Find `async fn NAME` (or `fn NAME`) in pre-split `lines`; return
    (start, end) or None. Lines are split once per file by the caller —
    splitting inside made the check O(files × handlers × filesize)."""
    fn_re = re.compile(r'^\s*(pub\s+)?(async\s+)?fn\s+' + re.escape(name) + r'\s*[\(<]')
    for i, line in enumerate(lines):
        if fn_re.match(line):
            # Walk to matching closing brace at column 0 (or next fn)
            depth = 0
            started = False
            for j in range(i, len(lines)):
                depth += lines[j].count('{') - lines[j].count('}')
                if '{' in lines[j]:
                    started = True
                if started and depth <= 0:
                    return i, j
            return i, len(lines) - 1
    return None

def doc_comment_above(lines, fn_start):
    """Collect doc-comment lines immediately above the fn, skipping
    attribute lines (#[instrument], #[serde(...)]) between doc and fn."""
    docs = []
    i = fn_start - 1
    # Skip attribute lines between the doc comment and the fn.
    while i >= 0 and lines[i].strip().startswith('#['):
        i -= 1
    while i >= 0 and (lines[i].strip().startswith('///') or lines[i].strip().startswith('//!')):
        docs.append(lines[i])
        i -= 1
    return '\n'.join(docs)

def main():
    errors = 0
    exempt_lines = set()
    exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'abac-exempt-handlers-exemptions.txt'
    if exempt_path.exists():
        for raw in exempt_path.read_text().splitlines():
            raw = raw.split('#', 1)[0].strip()
            if raw:
                exempt_lines.add(raw)

    # Step 1: collect exempt route patterns from the middleware.
    if not MW_PATH.exists():
        print(f"check-abac-exempt-handlers: WARNING — {MW_PATH} not found; cannot collect exempt routes")
        return
    mw_text = MW_PATH.read_text(encoding='utf-8', errors='replace')
    exempt_patterns = set(EXEMPT_ROUTE.findall(mw_text))
    # api-mapped routes are ABAC-enforced; excluded
    exempt_patterns -= PUBLIC_EXEMPT

    # Routes exempted via the registry-check's exemption file (task-093
    # F2/F3): these run with NO middleware policy evaluation at all, so the
    # per-handler rules apply to them exactly as to ::exempt routes.
    reg_exempt_path = Path(os.environ.get('GYRE_SCRIPT_DIR', '.')) / 'abac-route-registry-exemptions.txt'
    if reg_exempt_path.exists():
        for raw in reg_exempt_path.read_text().splitlines():
            raw = raw.split('#', 1)[0].strip()
            if raw.startswith('/api/'):
                exempt_patterns.add(raw)

    if not exempt_patterns:
        print("check-abac-exempt-handlers: OK (no non-public exempt routes)")
        return

    # Step 2: map route patterns to registered handler names in api/mod.rs.
    mod_rs = Path('crates/gyre-server/src/api/mod.rs')
    route_to_handlers = {}  # registered path -> [handler names]
    if mod_rs.exists():
        mod_text = mod_rs.read_text(encoding='utf-8', errors='replace')
        # Scan `.route("path", ...)` with paren counting so method-router
        # chains like get(a).post(b) and multi-line entries parse fully.
        i = 0
        while True:
            m = re.compile(r'\.route\(\s*"([^"]+)"\s*,').search(mod_text, i)
            if not m:
                break
            path = m.group(1)
            # Walk forward from the comma to the matching close paren of .route(.
            depth = 1
            j = m.end()
            while j < len(mod_text) and depth > 0:
                ch = mod_text[j]
                if ch == '(':
                    depth += 1
                elif ch == ')':
                    depth -= 1
                j += 1
            chain = mod_text[m.end():j - 1]
            handlers = METHOD_CHAIN.findall(chain)
            if handlers:
                route_to_handlers[path] = [h for _, h in handlers]
            i = j

    # Convert exempt patterns (:id params) to registered path lookup by
    # matching literal segments.
    def pattern_matches(pattern, path):
        pp = pattern.strip('/').split('/')
        qq = path.strip('/').split('/')
        if len(pp) != len(qq):
            return False
        for a, b in zip(pp, qq):
            if a.startswith(':'):
                continue
            if a != b:
                return False
        return True

    targets = {}  # handler name -> exempt pattern
    for pattern in sorted(exempt_patterns):
        matched = False
        for reg_path, handlers in route_to_handlers.items():
            if pattern_matches(pattern, reg_path):
                matched = True
                for h in handlers:
                    targets[h] = pattern
        if not matched:
            print(f"ERROR: ABAC-exempt route {pattern} is not registered in api/mod.rs")
            print("  An exempt mapping with no route means the mapping is stale, or the")
            print("  route is mounted elsewhere. Either way the exemption cannot be")
            print("  verified by this check.")
            errors += 1

    if not targets:
        print("check-abac-exempt-handlers: OK (no handlers to check)")
        return

    # Step 3: for each target handler, apply the rules.
    checked = 0
    for src in find_files(sys.argv[1:]):
        try:
            text = src.read_text(encoding='utf-8', errors='replace')
        except OSError:
            continue
        lines = text.splitlines()
        for handler, pattern in sorted(targets.items()):
            parsed = parse_handler(lines, handler.split('::')[-1])
            if parsed is None:
                continue
            fn_start, fn_end = parsed
            checked += 1
            sig = '\n'.join(lines[fn_start:fn_start + 10])
            body = '\n'.join(lines[fn_start:fn_end + 1])
            docs = doc_comment_above(lines, fn_start)

            # Rule 2: deferral language about auth in the doc comment.
            if DEFERRAL.search(docs) and re.search(r'auth|ABAC|workspace|tenant|permission', docs, re.IGNORECASE):
                key = f"{src}:{fn_start + 1}"
                if key not in exempt_lines and '// exempt-auth:ok' not in docs:
                    print(f"ERROR: deferred authorization in doc comment at {key}")
                    print(f"  handler `{handler}` (route {pattern}) documents enforcement as deferred.")
                    print("  A spec-mandated per-handler authorization must ship in the handler")
                    print("  body, not in a comment promising a follow-up. This is the")
                    print("  specs/reviews/task-087.md F1b flaw class (ABAC-exempt route with")
                    print("  no per-handler authorization).")
                    print("  Implement the check, or exempt with: // exempt-auth:ok — <reason>")
                    print()
                    errors += 1
            # Rule 3: mitigation claims must be backed by a scoping call.
            if MITIGATION_CLAIM.search(docs) and not SCOPING_CALL.search(body):
                key = f"{src}:{fn_start + 1}"
                if key not in exempt_lines and '// exempt-auth:ok' not in docs:
                    print(f"ERROR: unbacked mitigation claim in doc comment at {key}")
                    print(f"  handler `{handler}` (route {pattern}) claims the storage layer")
                    print("  scopes access 'naturally', but the handler body never calls a")
                    print("  tenant-scoped repository method. Prose is not enforcement.")
                    print("  This is the specs/reviews/task-087.md F1c flaw class.")
                    print("  Call a scoped lookup, or exempt with: // exempt-auth:ok — <reason>")
                    print()
                    errors += 1

            # Rule 1: an auth parameter (bound or _) requires a real decision.
            # Tightened per task-093 F2: `auth.agent_id` alone is NOT a
            # decision — it is an identity string, not a scope. Using it as
            # bookkeeping (`spawned_by: &auth.agent_id`) leaves every
            # cross-tenant request authorized. Only scope fields count.
            has_auth_param = re.search(r'_?\s*auth\s*:\s*(AuthenticatedAgent|AuthContext)', sig) is not None
            if not has_auth_param:
                continue  # no auth identity at all is a different check's job
            decision = (
                re.search(r'check_\w*_auth\(|authorize\w*\(|verify_access\(|require_\w+\(', body)
                or re.search(r'auth\.(tenant_id|workspace_id|user_id|role)', body)
                or re.search(r'resolve_\w+\(&auth\)', body)
                or re.search(r'Forbidden|NotFound', body)
            )
            if decision:
                # Rule 4 (task-093 F3): an ABAC-policy helper call is not a
                # substitute for tenant containment. The policy engine's
                # default is permissive when no policies are stored for the
                # entity ("No policies = unrestricted"), and the global
                # token / API-key paths bypass it entirely. A handler that
                # ONLY calls check_*_abac (or similar) without also comparing
                # the loaded entity's scope against the caller's is
                # cross-tenant reachable in default deployments. The Forbidden
                # scan ignores `.map_err(ApiError::Forbidden)` lines: merely
                # mapping a policy error type is not an added decision branch.
                abac_only = (
                    re.search(r'check_\w*abac\w*\(', body)
                    and not re.search(r'auth\.(tenant_id|workspace_id|user_id|role)', body)
                    and not re.search(r'Forbidden', '\n'.join(
                        l for l in body.splitlines() if '.map_err(' not in l))
                )
                if not abac_only:
                    continue
                key = f"{src}:{fn_start + 1}"
                if key not in exempt_lines and '// exempt-auth:ok' not in docs:
                    print(f"ERROR: ABAC-policy helper is not tenant containment at {key}")
                    print(f"  handler `{handler}` (route {pattern}) gates on an ABAC policy")
                    print("  check (e.g. check_repo_abac) but never compares the loaded")
                    print("  entity's tenant/workspace against the caller's scope. The policy")
                    print("  engine returns Ok when no per-entity policies are stored")
                    print("  (\"No policies = unrestricted\") and bypasses entirely for global")
                    print("  tokens / API keys — so in default deployments any authenticated")
                    print("  principal in any tenant passes. This is the")
                    print("  specs/reviews/task-093.md F3 flaw class.")
                    print("  Add an explicit tenant/workspace containment check, or exempt")
                    print("  with: // exempt-auth:ok — <reason>")
                    print()
                    errors += 1
                continue
            key = f"{src}:{fn_start + 1}"
            if key not in exempt_lines and '// exempt-auth:ok' not in docs:
                print(f"ERROR: ABAC-exempt handler performs no authorization at {key}")
                print(f"  handler `{handler}` (route {pattern}) names an auth parameter but")
                print("  its body never consults it: no entity-scope comparison, no")
                print("  Forbidden/NotFound branch, no auth helper call. Any authenticated")
                print("  agent can access this resource. This is the")
                print("  specs/reviews/task-087.md F1b flaw class.")
                print("  Load the entity, compare tenant/workspace, return Forbidden on")
                print("  mismatch (see api/users.rs dismiss_notification), or exempt with:")
                print("  // exempt-auth:ok — <reason>")
                print()
                errors += 1

    if errors:
        print(f"check-abac-exempt-handlers: FAILED — {errors} violation(s) across {checked} checked handler(s)")
        sys.exit(1)
    print(f"check-abac-exempt-handlers: OK ({checked} handler(s) checked)")

main()
PYEOF
