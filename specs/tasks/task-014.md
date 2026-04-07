# TASK-014: Spec Assertion Real Validation

**Spec reference:** `system-explorer.md` §9 (Executable Spec Assertions)
**Depends on:** None (consumes existing knowledge graph and spec content)
**Progress:** `complete`

## Spec Excerpt

From `system-explorer.md` §9:

> Specs can contain structural assertions that are checked against the knowledge graph on every push:
>
> ```markdown
> <!-- gyre:assert type="no_dependency" from="gyre-domain" to="gyre-adapters" -->
> - `gyre-domain` MUST NOT depend on `gyre-adapters`
>
> <!-- gyre:assert type="implements" subject="SearchService" trait="FullTextPort" -->
> - `SearchService` MUST implement `FullTextPort`
>
> <!-- gyre:assert type="all_have" node_type="Endpoint" property="auth_middleware" -->
> - All API endpoints MUST have auth middleware
> ```
>
> Assertion results appear in the spec's inline view: green checkmark for passing, red X for failing. Failures appear in the Inbox as action items.

## Current State

All requirements implemented during HSI milestone:
- `crates/gyre-domain/src/spec_assertions.rs` — full parser for `<!-- gyre:assert ... -->` HTML comments with subject types (Module, Type, Endpoint, Function) and predicates (depends_on, NOT depends_on, has_implementors, governed_by, calls, NOT calls, test_coverage, complexity, churn, field_count)
- `crates/gyre-server/src/api/spec_assertions.rs` — REST endpoint returning real pass/fail results (no stubs)
- `crates/gyre-server/src/graph_extraction.rs` — push-triggered assertion checking wired as Step 6b, creates `SpecAssertionFailure` notifications on failure
- Evaluation runs `spec_assertions::parse_assertions` → `spec_assertions::evaluate_assertions` against knowledge graph nodes/edges

## Implementation Plan

1. **Parser: Extract `gyre:assert` directives from spec markdown:**
   - Scan for `<!-- gyre:assert ... -->` HTML comments
   - Parse attributes: `type`, `from`, `to`, `subject`, `trait`, `node_type`, `property`
   - Supported assertion types per spec:
     - `no_dependency` — verify no edge of type `DependsOn` from module `from` to module `to`
     - `implements` — verify node `subject` has edge `Implements` to trait `trait`
     - `all_have` — verify all nodes of `node_type` have attribute `property` set (non-null)
   - Return a list of `SpecAssertion` structs

2. **Checker: Evaluate assertions against the knowledge graph:**
   - For each assertion, query the in-memory graph (from `GraphRepository`)
   - `no_dependency`: check edges between matching node pairs
   - `implements`: find subject node by name, check for implements edge to trait
   - `all_have`: filter nodes by type, check property presence
   - Return pass/fail for each with explanation

3. **Wire into push-triggered extraction:**
   - After graph extraction completes (`graph_extraction.rs`), run assertion checks
   - For failed assertions, create priority-9 `SpecAssertionFailure` notifications

4. **Wire the REST endpoint:**
   - `GET /api/v1/repos/:id/spec/assertions` should return real results
   - Input: repo_id (the spec content is read from the repo's specs)
   - Output: list of `{spec_path, assertion_type, assertion_params, status: pass|fail, explanation}`

5. **Add tests:**
   - Parse `gyre:assert` directives from sample markdown
   - `no_dependency` assertion passes when no dependency exists
   - `no_dependency` assertion fails when dependency exists
   - `implements` assertion passes/fails correctly
   - `all_have` assertion passes/fails correctly
   - Notification created on failure

## Acceptance Criteria

- [ ] `gyre:assert` HTML comments are parsed from spec markdown
- [ ] `no_dependency` assertion type checks graph edges between modules
- [ ] `implements` assertion type checks trait implementation edges
- [ ] `all_have` assertion type checks node attribute presence
- [ ] `GET /api/v1/repos/:id/spec/assertions` returns real pass/fail results
- [ ] Failed assertions create priority-9 notifications
- [ ] Push-triggered graph extraction runs assertion checks automatically
- [ ] Tests cover each assertion type (pass and fail cases)

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-domain/src/spec_assertions.rs` for existing types
3. Read `crates/gyre-server/src/api/spec_assertions.rs` for the current handler
4. Read `crates/gyre-server/src/graph_extraction.rs` for the push-triggered hook
5. Read `crates/gyre-ports/src/graph.rs` for the GraphRepository port
6. Implement the parser, checker, and wire the handler + push hook
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of HSI milestone PRs (pre-task-creation). Status updated to `complete` by project manager after code verification.
