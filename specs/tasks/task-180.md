---
title: "Executable Spec Assertions — gyre:assert parsing and knowledge graph validation"
spec_ref: "system-explorer.md §9"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §9. Executable Spec Assertions"
commits: ["4119f9bd2385a50ef3a80df1438ef44328ac9a96", "6aadd3a0240a9ec0066bb0e730e0e9b16c220ef1", "f09e23603dbc4626523d05d06e34b26d4749b820", "3778c14a06aab7bac9df7bd185d8a81409d39d3b", "483f9b1b3029eb6fd7c7b9139ad27dcdb77bfab8", "68548e19f5ff2060c83ea2357525c9f225f280bc", "b8143187c42389e6c10a593d354ddb27e4664134", "cda15008584c10c66ad0d256483098b48ed4c5c3"]
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

- [x] `<!-- gyre:assert -->` comments parsed from spec markdown
- [x] `no_dependency` assertion validates against knowledge graph
- [x] `implements` assertion validates against knowledge graph
- [x] `all_have` assertion validates against knowledge graph
- [x] Failed assertions create priority-9 Inbox notifications
- [x] `GET /repos/:id/specs/:path/assertions` returns assertion results
- [x] Assertion results shown in spec inline view (green ✓ / red ✗)
- [x] Tests pass

## Agent Instructions

Read `system-explorer.md` §9 "Executable Spec Assertions" for the full specification. The knowledge graph API is in `crates/gyre-server/src/api/graph.rs` — check how nodes and edges are queried. The spec content is retrieved via `GET /api/v1/specs/:path?repo_id=` — check `crates/gyre-server/src/api/specs.rs`. For Inbox notifications, check the existing notification creation pattern in `crates/gyre-domain/` (search for priority levels and notification types). The new endpoint should be registered in `crates/gyre-server/src/api/mod.rs`. Verify the route path matches this task before implementing: `GET /api/v1/repos/:id/specs/:path/assertions`.

## Shipped

Recovered interrupted checkpoint: implementation was already complete and
byte-identical to the independently reviewed HEAD `51f29e9` (review verdict
`complete`, mutation-verified). This round re-verified behavior on current
HEAD `7120f07b` and restored `progress: ready-for-review` (the recovery
checkpoint had reset it to `not-started`).

Actual behavior (production code, no stubs):

- **Parser** (`gyre-domain/src/spec_assertions.rs`): `parse_assertions`
  extracts `<!-- gyre:assert ... -->` comments from spec markdown; attribute
  form covers all three §9 types (`no_dependency`, `implements`, `all_have`),
  plus the richer predicate form. Malformed assertions are skipped, not
  silently passed.
- **Evaluator**: real graph queries over `GraphNode`/`GraphEdge` —
  `no_dependency` fails on any DependsOn edge `from→to`; `implements` fails
  when no `Implements` edge `subject→trait` exists; `all_have` fails when any
  node of `node_type` lacks the property and fails closed on zero subject
  nodes (no vacuous pass) or unknown property.
- **Push integration**: `check_spec_assertions_on_push` runs after knowledge
  graph extraction on all three push paths (git HTTP receive-pack, mirror
  sync, initial mirror clone), persists full pass+fail result sets keyed by
  `(repo_id, spec_path, line, commit_sha)` via the new
  `SpecAssertionResultRepository` port (SQLite + PostgreSQL adapters,
  migration 000056, mem adapter with identical replace semantics), and
  suppresses duplicate notifications for same-commit re-extraction.
- **Inbox notifications**: each failed assertion creates priority-9
  `SpecAssertionFailure` notifications (default priority 9 in
  gyre-common/src/notification.rs) for Admin/Developer/Owner workspace
  members, with `entity_ref` linking the first failing spec so Inbox
  "Update Spec" opens it.
- **API**: `GET /api/v1/repos/:id/specs/:path/assertions` returns persisted
  last-push results (URL-encoded path, unknown repo 404s);
  `POST /api/v1/repos/:id/spec-assertions/check` performs a live check. Both
  ABAC-resolver mapped (no new exemptions; the check route was moved OUT of
  the frozen exemption file — count 53→52).
- **Inline view**: ExplorerView.svelte renders per-assertion `✔`/`✘` with
  pass/fail styling and an `N failing, M passing` summary; live check falls
  back to persisted GET results.

Test evidence (this round, HEAD 7120f07b, /tmp/gyre-target):

- `cargo test -p gyre-domain --lib spec_assertions` — 48 passed, 0 failed.
- `cargo test -p gyre-server --lib spec_assertions` — 6 passed, 0 failed.
- `cargo test -p gyre-server --lib push_check` — 2 passed (priority-9
  notification creation + persistence through the push path), 0 failed.
- `cargo test -p gyre-server --lib delete_repo_removes` — 1 passed (repo
  delete cleans orphaned assertion rows), 0 failed.
- `cd web && npm ci && npx vitest run src/__tests__/Inbox.test.js` — 28
  passed, 0 failed.
- Invariant gates all pass: check-arch, check-mem-port-contracts,
  check-abac-route-registry, check-migration-versions,
  check-migration-sql-portability, check-dead-message-kinds,
  check-in-memory-state-stores, check-inert-enforcement,
  check-fail-open-ref-resolution, check-lossy-secret-conversion,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-byte-slice-truncation, check-forwarded-header-trust,
  check-forged-scope-fields, check-unbounded-external-http,
  check-relative-path-defaults.
- Evidence: `/tmp/stage/review-evidence/task180-recovery-verification.md`.

Known pre-existing upstream issue (not this branch's regression):
`scripts/check-task-commit-attribution.sh` fails on commit `a781ede2`
(task-210) missing from task-210's frontmatter — reproduced identically at
the assigned upstream base `8c2d1775`, so it predates this branch. Left for
the task-210 owner; editing another task's review-scoping list is out of
scope here.

Live HTTP verification could not run in this sandbox (TCP listener probe
unsupported, Errno 95 per /tmp/stage/capabilities.json); route behavior is
covered by the in-process axum router tests above. Exact-head GitHub checks
remain with verification/publication.
