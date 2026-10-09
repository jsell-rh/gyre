---
title: "Executable Spec Assertions — gyre:assert parsing and knowledge graph validation"
spec_ref: "system-explorer.md §9"
depends_on: []
progress: not-started
coverage_sections:
  - "system-explorer.md §9. Executable Spec Assertions"
commits: ["7b182b57c82ec4e3c94562e98e5fd87ac09be810", "b79f5c0bc8a5a0d1f800f0c0105c6c9a19434af6", "4119f9bd2385a50ef3a80df1438ef44328ac9a96", "6aadd3a0240a9ec0066bb0e730e0e9b16c220ef1", "f09e23603dbc4626523d05d06e34b26d4749b820", "3778c14a06aab7bac9df7bd185d8a81409d39d3b", "483f9b1b3029eb6fd7c7b9139ad27dcdb77bfab8", "68548e19f5ff2060c83ea2357525c9f225f280bc", "b8143187c42389e6c10a593d354ddb27e4664134", "cda15008584c10c66ad0d256483098b48ed4c5c3"]
---

## Spec Excerpt

system-explorer.md §9 defines executable spec assertions — HTML comments embedded in spec markdown that are checked against the knowledge graph on every push:

```markdown
<!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->
- `gyre-domain` MUST NOT depend on `gyre-adapters`

<!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->
- `SearchService` MUST implement `FullTextPort`

<!-- gyre:assert type="all_have" node_type="Endpoint" property="auth_middleware" -->
- All API endpoints MUST have auth middleware
```

**Assertion types:**
- `no_dependency` — validates that crate `from` has no dependency edge to crate `to` in the knowledge graph
- `implements` — validates that `subject` node implements `trait` via an `Implements` edge
- `all_have` — validates that all nodes of `node_type` have the named `property` (non-null)

**Results:** Assertion results appear in the spec's inline view (green checkmark for passing, red X for failing). Failures appear in the Inbox as priority-9 action items ("Spec assertion failure" — View Code, Update Spec).

**Validation timing:** Assertions are checked against the knowledge graph on every push (when the knowledge graph is rebuilt from the updated code).

## Implementation Plan

1. **Spec assertion parser** (domain layer, `gyre-domain`):
   - Parse `<!-- gyre:assert ... -->` comments from spec markdown content
   - Extract assertion type and parameters into a `SpecAssertion` struct
   - Support types: `no_dependency`, `implements`, `all_have`
   - Return `Vec<SpecAssertion>` for a given spec content string

2. **Assertion evaluator** (domain layer):
   - Given a `SpecAssertion` and access to the knowledge graph (via ports), evaluate pass/fail
   - `no_dependency`: query graph for dependency edges from `from` to `to`, fail if any exist
   - `implements`: query graph for `Implements` edge from `subject` to `trait`, fail if missing
   - `all_have`: query graph for all nodes of `node_type`, fail if any lack `property`
   - Return `AssertionResult { assertion, passed: bool, details: String }`

3. **Assertion check integration**:
   - Add assertion evaluation to the post-push knowledge graph rebuild flow
   - After graph extraction, parse all specs for assertions and evaluate them
   - Store results associated with the spec path and commit SHA

4. **Inbox notification for failures**:
   - When an assertion fails, create a priority-9 Inbox notification
   - Item type: "Spec assertion failure" with actions: View Code, Update Spec
   - Link to the failing spec and the knowledge graph node(s) involved

5. **API endpoint for assertion status**:
   - `GET /api/v1/repos/:id/specs/:path/assertions` — returns assertion results for a spec
   - Response: `[{ type, params, passed, details, checked_at }]`
   - Used by the frontend to show green checkmark / red X in spec views

6. **Tests**:
   - Parser correctly extracts all assertion types from markdown
   - Evaluator correctly validates each assertion type against mock graph data
   - Failed assertions create Inbox notifications
   - API endpoint returns correct assertion results

## Acceptance Criteria

- [ ] `<!-- gyre:assert -->` comments parsed from spec markdown
- [ ] `no_dependency` assertion validates against knowledge graph
- [ ] `implements` assertion validates against knowledge graph
- [ ] `all_have` assertion validates against knowledge graph
- [ ] Failed assertions create priority-9 Inbox notifications
- [ ] `GET /repos/:id/specs/:path/assertions` returns assertion results
- [ ] Assertion results shown in spec inline view (green ✓ / red ✗)
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §9 "Executable Spec Assertions" for the full specification. The knowledge graph API is in `crates/gyre-server/src/api/graph.rs` — check how nodes and edges are queried. The spec content is retrieved via `GET /api/v1/specs/:path?repo_id=` — check `crates/gyre-server/src/api/specs.rs`. For Inbox notifications, check the existing notification creation pattern in `crates/gyre-domain/` (search for priority levels and notification types). The new endpoint should be registered in `crates/gyre-server/src/api/mod.rs`. Verify the route path matches this task before implementing: `GET /api/v1/repos/:id/specs/:path/assertions`.
