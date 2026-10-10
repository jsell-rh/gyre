---
title: "Executable Spec Assertions — gyre:assert parsing and knowledge graph validation"
spec_ref: "system-explorer.md §9"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §9. Executable Spec Assertions"
commits: ["f89aa1565efe2200f40f628c24f1d1828a5859f3", "eaae6c7dc48e9e71ecdcf91a90cc4b6711334e44", "7b182b57c82ec4e3c94562e98e5fd87ac09be810", "b79f5c0bc8a5a0d1f800f0c0105c6c9a19434af6", "4119f9bd2385a50ef3a80df1438ef44328ac9a96", "6aadd3a0240a9ec0066bb0e730e0e9b16c220ef1", "f09e23603dbc4626523d05d06e34b26d4749b820", "3778c14a06aab7bac9df7bd185d8a81409d39d3b", "483f9b1b3029eb6fd7c7b9139ad27dcdb77bfab8", "68548e19f5ff2060c83ea2357525c9f225f280bc", "b8143187c42389e6c10a593d354ddb27e4664134", "cda15008584c10c66ad0d256483098b48ed4c5c3"]
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



## Shipped

Verification-repair round (assignments `3ed85f66`/`b6833db7`; this round
repairs durable verification finding `fc532b04` — checks.sh exit 1 on the
verify merge of candidate `bc905bea` into base `c9b0a6f9`).

Contract-state acceptance criteria are satisfied by production code
(independently reviewed at `51f29e95`, verdict `complete`, mutation-verified
in `specs/reviews/task-180.md`): the `<!-- gyre:assert ... -->` parser and
evaluator (`gyre-domain/src/spec_assertions.rs`), post-push checking on all
three push paths with persistence via the `SpecAssertionResultRepository`
port (SQLite + PostgreSQL + mem), priority-9 `SpecAssertionFailure` Inbox
notifications, `GET /api/v1/repos/:id/specs/:path/assertions`, and the
ExplorerView inline ✔/✘ rendering.

Defects found and fixed across the repair rounds since that review:

1. **Fenced examples parsed as live assertions.** `parse_assertions` had no
   code-fence awareness, so the `gyre:assert` examples inside fenced blocks —
   including system-explorer.md §9's own documentation block and
   specs/tasks/task-014.md's excerpt — were evaluated against the knowledge
   graph on every push. Those examples reference entities that do not exist
   (`SearchService`/`FullTextPort`, `all_have Endpoint auth_middleware`), so
   any repo documenting the feature would generate failing assertions and
   priority-9 notifications forever. The parser now tracks ``` / ~~~ fences
   and only extracts assertions from prose. (Reported line numbers remain
   absolute across fences.)

2. **Stale rows contradicted the port's replace contract.** All three
   adapters early-returned on an empty batch, and the push check skipped
   files without assertions — so a spec whose assertions were removed (or a
   deleted spec file) kept serving its old rows through
   `GET .../assertions` as the "latest push's check", exactly what the
   `save_results` delete-then-insert transaction and the migration's comment
   promise cannot happen. The port gained `delete_by_spec` and
   `list_spec_paths`; the push check now clears rows for specs that no
   longer carry assertions and sweeps stored paths absent from the pushed
   tree (deleted/renamed specs). Present-path registration happens before
   the content read so an unreadable-but-present spec is never swept.

3. **E0195 compile regression (interrupted round).** An edit had replaced
   the port's `#[async_trait]` attribute instead of adding alongside it,
   desugaring the impls' lifetimes away from the trait declaration. Both
   attributes restored; `cargo check -p gyre-server` clean, clippy
   changed-lines clean at checkpoint `f89aa156`.

4. **Verification failure fc532b04 (this round): checkpoint-swept dist regen.**
   `dev-check.sh:8` runs `git diff --check HEAD^1 HEAD` on the verify merge
   (candidate merged into the assigned base). Checkpoint `f89aa156`'s
   `git add -A` swept in a full `npm run build` output (build.rs rebuilds the
   SPA on any cargo build without `SKIP_WEB_BUILD=1`), renaming every
   content-hashed asset — so the diff presented the whole bundle as added
   lines, including a dependency-sourced trailing-whitespace byte pair
   (`" \t"` inside a svelte-runtime template literal, present byte-identical
   in the base bundle too). `git diff --check c9b0a6f9..fca7596a` reproduces
   exit 2. Fixed by reverting `web/dist` to the base state (`420e961e`) —
   committed dist regen is not required (verify itself rebuilds and then
   reverts dist, dev-check.sh:85-86; main's dist is 5 web/src commits
   behind; no code references bundle hashes). Product code unchanged.

5. **Attribution drift on the verify merge (this round).** Base `6bf777a6`'s
   tree predates main's `e96d25ab` (task-219), which appended `6bf777a6` to
   `specs/tasks/task-200.md`'s `commits:` list. Since the verify merge keeps
   the base's task-200.md unless the candidate carries the fix, the
   attribution gate would fail on the merge and identically on the base
   baseline probe (exit 81 → baseline detour). Fixed by carrying main's
   exact one-line fix (`0c4a767e`, byte-identical copy of origin/main's
   task-200.md; `git diff 6bf777a6 HEAD -- specs/tasks/task-200.md` matches
   `git diff 6bf777a6 origin/main` exactly).

Test evidence (this round; code-state head `0c4a767e` — the branch tip
differs from it only by this task-file record; CARGO_TARGET_DIR=/tmp/gyre-target,
exact commands/counts and probe-head SHAs in
`/tmp/stage/review-evidence/task180-verify-repair-round.md`):

- `git diff --check 6bf777a6..<branch tip>` — **exit 0** (the failing
  gate's condition on this assignment's base; re-run at the branch tip
  after each record-only amend).
- `bash scripts/check-task-commit-attribution.sh` — **exit 0** (OK).
- `cargo test -p gyre-domain --lib spec_assertions` — **51 passed, 0 failed**.
- `cargo test -p gyre-server --lib push_check` — **3 passed, 0 failed**.
- `cargo test -p gyre-server --lib spec_assertions` — **6 passed, 0 failed**.
- `cd web && npx ci`-installed deps; `npx vitest run
  src/__tests__/Inbox.test.js` — **28 passed, 0 failed** (3.72s).

Sandbox transport limitation (recorded, not a code defect): this runtime
cannot open a TCP listener (`accept` → EOPNOTSUPP), so live HTTP exercise of
`GET /api/v1/repos/:id/specs/:path/assertions` against a running server is
deferred to host verification / GitHub CI; the endpoint is covered
in-process by the five handler tests above.

Task contract integrity: the assigned contract prose (including Acceptance
Criteria checkboxes) is untouched; completion is reported through
`progress:` and this section. Attribution lists all 12 product-surface
commits on the branch; this round's two commits touch only `web/dist/` and
`specs/tasks/task-200.md` (both outside the attribution gate's product
surface and the review-stage `invalid`-change allowlist, stages.py:115-117).