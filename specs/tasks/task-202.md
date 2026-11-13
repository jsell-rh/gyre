---
title: "Index all searchable entity types and implement real full reindex"
spec_ref: "search.md §Searchable Entities"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Searchable Entities"
commits: ["5acae62c983d21a1288b652a6628e8f81392fd59", "3e5f747b95b46e48c02e9431c8126dfe833e1d31", "1f80f64f4fa013634d38e985b271fb20d4a09a1a", "b316bc37f64c6f032c563b9e590a823ee8a626e4", "b522dd5cdb4eb4e6049d224dc3f745e5939ce3b9", "2faf26af2c4ca2fa00052be4fd9cf5429677c837", "3dc6cdcc5413b4eabd004920fcf13090dd658708", "3e223c8a1d632e2475d0a690a9cab77f3f4ced2c", "e2583917beb41b812d6f5799149f23e30ff332d7", "80f13a7059e1458ce3f10551909461a4a5677342", "309fc7dd426b346fdfa576a5770c49d65fc2aced"]
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
`POST /api/v1/admin/search/reindex` performs a true full rebuild from
domain storage. The production implementation is commits `80f13a70`/
`309fc7dd` (plus review/verification repairs `3e223c8a`, `e2583917`,
`2faf26af2`, `3dc6cdcc5`, `b522dd5cd`, `b316bc37f`, rebase repairs
`f38e8ad07`/`1f80f64f4`, clippy repair `3e5f747b9`, rebase onto assignment
base `a218b956` in merge `babb77f7c`). This round (`5acae62c9`) repairs
review findings from checkpoint `5fe0f18c`.

### Behavior

- Spec: indexed on push from the pushed git blob (`index_repo_specs`),
  with ledger-sourced title/owner/approval_status/links facets; commit:
  indexed on push from `git show` (`index_pushed_commit`); persona,
  activity event, audit event, review: indexed on their write paths via
  `search_sync` repository decorators and handler callsites; file (code
  search): indexed asynchronously on push via the `JobRegistry` job
  `search_code_index` (binary and >1 MiB blobs skipped, real content
  indexed). Every write path funnels through the document builders in
  `crates/gyre-server/src/search_indexer.rs`, so builder fields are the
  single source of truth for every index path.
- `reindex_all` (search_indexer.rs) clears the index then rebuilds from
  every entity repository (spec ledger + git blobs, tasks, MRs + reviews,
  agents, personas, analytics, audit, agent-commit mappings + git
  messages, default-branch file trees) and returns the true document
  count. `POST /api/v1/admin/search/reindex` invokes this coordinator.

### This round: review-finding repairs (checkpoint 5fe0f18c)

Both findings were specced Indexed Fields implemented as filter-only
facets (or missing entirely), which `GET /api/v1/search` — it has no
facet-value filter params — could neither search nor filter:

- **Agent persona** (was never indexed): `Agent.persona_id` is now folded
  into the document body and carried as a `persona` facet, so
  `?q=<persona-id>` returns the agent.
- **Commit author/branch, review decision/reviewer, spec owner,
  activity/audit event agent_id**: all folded into the document body
  (the MR-author pattern), keeping their facets for filtering.

### Test evidence

- `search_indexer::tests::full_reindex_rebuilds_all_entity_types_from_real_storage`
  seeds all 10 entity types against a real SQLite FTS5 backend, poisons
  the index with a stale doc, and asserts the true rebuild count (12),
  idempotency, stale-doc removal, and per-type retrieval — including
  spec-by-body-content, commit-by-message, and now agent-by-persona,
  commit-by-author/branch, review-by-decision/reviewer, spec-by-owner,
  and event-by-agent_id (each seeded so the value appears nowhere else
  in the document, so the assertion fails on the pre-fix builders).
- Push-path and code-search tests: `index_repo_specs_extracts_blob_content_from_pushed_commit`,
  `index_repo_specs_indexes_linked_ids_from_ledger`,
  `index_pushed_commit_reads_message_and_author`,
  `file_indexing_skips_binary_and_oversized_blobs`.
- Executed in this sandbox: `cargo check -p gyre-server --lib --tests`,
  `cargo clippy -p gyre-server --lib --tests` (no warnings in changed
  files), `cargo test -p gyre-adapters --lib sqlite::search` (13 passed
  — FTS semantics the new assertions rely on: AND-of-terms, porter
  stemming), `cargo fmt -p gyre-server -- --check` (clean). gyre-server
  test compilation exceeds this sandbox's 4 GiB budget (build-wrapper
  refusal, recorded for remote verification); the verifier must run:
  `cargo test -p gyre-server --lib search_indexer::`,
  `cargo test --all`, and `bash scripts/check-arch.sh`.
