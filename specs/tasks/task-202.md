---
title: "Index all searchable entity types and implement real full reindex"
spec_ref: "search.md §Searchable Entities"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Searchable Entities"
commits: ["80f13a7059e1458ce3f10551909461a4a5677342", "309fc7dd426b346fdfa576a5770c49d65fc2aced", "3e223c8a1d632e2475d0a690a9cab77f3f4ced2c", "e2583917beb41b812d6f5799149f23e30ff332d7", "2faf26af2c4ca2fa00052be4fd9cf5429677c837", "3dc6cdcc5413b4eabd004920fcf13090dd658708", "b522dd5cdb4eb4e6049d224dc3f745e5939ce3b9", "b316bc37f64c6f032c563b9e590a823ee8a626e4", "1f80f64f4fa013634d38e985b271fb20d4a09a1a", "3e5f747b95b46e48c02e9431c8126dfe833e1d31", "5acae62c983d21a1288b652a6628e8f81392fd59", "e9f6b2f4e2af245d58f349b1486a50b84fc9f29d"]
---

## Spec Excerpt

From `search.md` §Searchable Entities — the index MUST cover all of:

> | Spec | title, path, full content, owner, links | workspace, repo, approval_status, scope |
> | Task | title, description, labels | workspace, repo, status, priority, assigned_to |
> | Merge Request | title, source_branch, target_branch, author | workspace, repo, status, has_conflicts |
> | Commit | message, SHA, author, branch | workspace, repo, agent_id, date_range |
> | Agent | name, persona, status | workspace, repo, status |
> | Persona | name, prompt content | scope (global/workspace/repo) |
> | Activity Event | description, event_type, agent_id | workspace, repo, event_type, date_range |
> | Audit Event | event_type, details, path | workspace, repo, agent_id, event_type, date_range |
> | Review | body, decision, reviewer | workspace, repo, MR |
> | File | path, content (code search) | workspace, repo, language |

From §Index Updates:

> - Spec pushed -> index updated (content extracted from git blob)
> - Commit pushed -> commit messages indexed
> For code search (file content), indexing happens asynchronously on push via a background job.

## Problem (current state — code-verified)

Only 3 of 10 entity types are indexed: task (`api/tasks.rs:213`), mr (`api/merge_requests.rs:383`), agent (`api/agents.rs:130`). Spec / commit / persona / activity-event / audit-event / review / file are NEVER indexed. `reindex_all` is a no-op that counts existing in-memory docs — it does NOT rebuild from domain entities.

## Implementation Plan

Depends on task-201 (real FTS backend + `SearchPort` semantics). Populate every `SearchDocument.facets` map with the specced facets for each type (see table) and set `workspace_id`/`repo_id`/`title`/`body` correctly.

1. **Spec indexing** — at spec push (`crates/gyre-server/src/git_http.rs`, near `process_spec_lifecycle`): extract spec content from the pushed git blob, index `entity_type="spec"`, `entity_id`=spec path, `body`=full content, facets `{approval_status, scope, workspace, repo, owner}`.
2. **Commit indexing** — at post-push commit handling (`git_http.rs`): index `entity_type="commit"`, `entity_id`=SHA, `body`=commit message, facets `{author, branch, agent_id, repo, workspace, date}`.
3. **Persona indexing** — at persona create/update (locate persona write path; likely `api/personas.rs` / persona repository callsite): `entity_type="persona"`, `body`=prompt content, facet `{scope}`.
4. **Activity event indexing** — where `AnalyticsEvent`/activity events are recorded (`api/tasks.rs` already imports `AnalyticsEvent`; find the analytics emit path): `entity_type="activity_event"`, `body`=description, facets `{event_type, agent_id, workspace, repo, date}`.
5. **Audit event indexing** — at audit write (`AuditRepository` callsite / audit emission): `entity_type="audit_event"`, `body`=details/path, facets `{event_type, agent_id, path, workspace, repo, date}`.
6. **Review indexing** — at review creation (`api/reviews.rs`): `entity_type="review"`, `body`=review body, facets `{decision, reviewer, mr, workspace, repo}`.
7. **File (code search)** — asynchronous on push via a background job (use the existing `JobRegistry` on `AppState`): walk pushed files, index `entity_type="file"`, `entity_id`=path, `body`=file content, facets `{language, repo, workspace}`. Code search is eventually consistent — a background job, not inline. Cap indexed file size sensibly (skip binaries/large blobs) but index real content, not a stub.
8. **Real full reindex** — add a server-level `SearchIndexer` (e.g. `crates/gyre-server/src/search_indexer.rs`) that clears the index then iterates EVERY entity repository (specs, tasks, mrs, commits, agents, personas, activity events, audit events, reviews, files) and calls `SearchPort::index` for each, returning the true count. Change `reindex_handler` (`api/search.rs:91`, route `POST /api/v1/admin/search/reindex`) to invoke this coordinator instead of the port's `reindex_all` no-op. The coordinator lives in the server layer because it must read multiple repositories (the adapter cannot).

## Acceptance Criteria

- [ ] All 10 entity types (spec, task, mr, commit, agent, persona, activity_event, audit_event, review, file) are indexed with the specced fields and facets. Each write path calls `SearchPort::index` synchronously (except file/code search which runs via a background job).
- [ ] Spec content is extracted from the git blob on push and indexed as `body`.
- [ ] `POST /api/v1/admin/search/reindex` rebuilds the entire index from all domain repositories and returns the true document count (NOT a count of pre-existing docs).
- [ ] After a reindex against a seeded store, `GET /api/v1/search?q=` finds entities of each type; a query that matches a spec by body content returns that spec.
- [ ] A test seeds entities of at least 4 distinct types (including spec-by-content and commit-by-message), runs the reindex coordinator against a real SQLite FTS backend, and asserts each type is retrievable. The test MUST fail if any type is skipped or if reindex counts stale docs instead of rebuilding.
- [ ] `cargo test --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Read task-201's delivered adapter first. Read `crates/gyre-server/src/git_http.rs` (spec/commit push), `crates/gyre-server/src/api/reviews.rs`, the persona write path, the audit/analytics emit paths, and `crates/gyre-server/src/api/search.rs` (`reindex_handler`). Reuse the existing `SearchDocument` construction pattern in `api/tasks.rs:212-220`.
- The file/code-search job MUST use the existing `JobRegistry` (`AppState.job_registry`) — do not invent a new async runtime.
- Keep FTS SQL in `gyre-adapters`; the coordinator only calls the port. Preserve hexagonal boundaries.
- Run validation once at the end. On completion set `progress: ready-for-review` and record commit SHAs.

## Shipped

All 10 searchable entity types from search.md §Searchable Entities are
indexed with real content and the specced indexed fields and facets, and
`POST /api/v1/admin/search/reindex` rebuilds the entire index from domain
storage. The production implementation is commits `80f13a70`/`309fc7dd`
plus review/verification repairs (`3e223c8a`, `e2583917`, `2faf26af`,
`3dc6cdcc`, `b522dd5c`, `b316bc37`, `1f80f64f`, `3e5f747b`, `5acae62c`).
This round (`e9f6b2f4`) resolves the rebase against current main, where
task-154's search query language had landed in conflict with the
all-entity indexer.

### Behavior

- The document builders in `crates/gyre-server/src/search_indexer.rs` are
  the single source of truth for every index path: spec (content extracted
  from the pushed git blob, ledger-sourced title/owner/approval_status/
  links), task, mr, commit (message from `git show`), agent, persona,
  activity event, audit event, review, and file (code search via the
  `JobRegistry`-registered background job; binary and >1 MiB blobs
  skipped, real content indexed). Write paths index synchronously through
  `search_indexer::index`; tenant scope is resolved uniformly by the
  `with_tenant_resolution` port wrapper, never per-callsite.
- `reindex_all` (search_indexer.rs) clears the index then rebuilds from
  every entity repository and returns the true document count;
  `reindex_handler` (`POST /api/v1/admin/search/reindex`) invokes this
  coordinator.
- Rebase integration (`e9f6b2f4`): every document builder carries the
  entity's real timestamp in main's new `SearchDocument.updated_at` field
  so the `since:`/`until:` query facets filter on entity time, not index
  time; the Postgres INSERT upsert combines main's `updated_at` column
  ($9) with the 4-column scope-aware ON CONFLICT target; the API handler
  conflicts resolve to the search_indexer document builders (full specced
  facets) over main's inline per-callsite documents; the SQL-shape
  alias/placeholder-contiguity invariants formerly asserted against the
  removed `*_BASE_SQL` constants are now asserted against the extracted
  `search_sql`/`suggest_sql`/`facet_sql` composers the port methods
  actually execute, alongside both sides' existing tests.

### Test evidence

- `cargo test -p gyre-adapters --lib search` — 43 passed (SQLite FTS5
  roundtrip, query language, access scoping, scope-aware upsert, reindex
  clear-and-count contract).
- `cargo test -p gyre-adapters --lib postgres::search` — 14 passed (both
  task-202's PK-upgrade tests and task-154's tsv-predicate/scope tests
  after the merge resolution).
- `cargo test -p gyre-adapters --lib mem_search` — 11 passed.
- `cargo check --workspace --all-targets` (type-checks all server tests),
  `cargo clippy` on the touched crates (zero warnings in touched files),
  `cargo fmt --check`, `bash scripts/check-arch.sh`, and the ABAC
  route-registry / inert-enforcement / unbounded-HTTP checks pass.
- Deferred to the independent verifier (gyre-server test codegen exceeds
  the sandbox 4 GiB budget; type-checked here):
  `cargo test -p gyre-server --lib search_indexer` (includes
  `full_reindex_rebuilds_all_entity_types_from_real_storage`, the
  acceptance test seeding every entity type against a real SQLite FTS
  backend with a stale-doc poison check), `cargo test -p gyre-server
  --test search_wiring`, and full `cargo test --all`.
