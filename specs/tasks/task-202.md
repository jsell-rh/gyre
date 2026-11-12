---
title: "Index all searchable entity types and implement real full reindex"
spec_ref: "search.md §Searchable Entities"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Searchable Entities"
commits: ["f38e8ad07f5081342ffcc2dc6ee30e956b72bce9", "b316bc37f64c6f032c563b9e590a823ee8a626e4", "b522dd5cdb4eb4e6049d224dc3f745e5939ce3b9", "2faf26af2c4ca2fa00052be4fd9cf5429677c837", "3dc6cdcc5413b4eabd004920fcf13090dd658708", "3e223c8a1d632e2475d0a690a9cab77f3f4ced2c", "e2583917beb41b812d6f5799149f23e30ff332d7", "80f13a7059e1458ce3f10551909461a4a5677342", "309fc7dd426b346fdfa576a5770c49d65fc2aced"]
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
domain storage. The original production implementation lives in commits
`80f13a70`/`309fc7dd` (plus review/verification repairs `3e223c8a`,
`e2583917`, `2faf26af2`, `3dc6cdcc5`, `b522dd5cd`, `b316bc37f`). This
round is a rebase repair: merge commit `f38e8ad07` integrates the branch
with updated main (3af80bd21) whose spawn-path refactor and shared
workspace-store test wiring conflicted with the task-202 changes; every
conflict was resolved keeping both sides.

### Behavior

- **Server-level coordinator** (`crates/gyre-server/src/search_indexer.rs`):
  document builders for every entity type (spec, task, mr, commit, agent,
  persona, activity_event, audit_event, review, file) with the specced
  indexed fields and facet maps (`status`, `priority`, `approval_status`,
  `scope`, `branch`, `author`, `decision`, `language`, ISO-date for
  date-range facets, etc.). Best-effort `index`/`deindex` helpers keep
  search failures from failing the entity write.
- **Write paths** call `SearchPort::index` synchronously: task
  create/update/transition (`api/tasks.rs`), MR create/status and review
  submission (`api/merge_requests.rs`), agent create/spawn/status
  (`api/agents.rs`, `api/spawn.rs`), persona create/update
  (`api/personas.rs`, delete also de-indexes), activity events at every
  analytics emit path (`api/analytics.rs`, `api/tasks.rs`,
  `api/merge_requests.rs`, `api/spawn.rs`, `api/orchestrator.rs`,
  `merge_processor.rs` — each emit site derives `workspace_id`/`repo_id`
  from the entity in hand), and audit events (`api/audit.rs`,
  `container_audit.rs`, `procfs_monitor.rs`).
- **Spec pushes** (`git_http.rs`): after the ledger sync, spec content is
  extracted from the pushed git blob and indexed as `body` with the
  ledger's linked task/MR ids appended (the specced `links` field) and
  `approval_status`/`scope`/`owner` facets.
- **Commit pushes** (`git_http.rs`): message + author are read from the
  git object store and indexed with `branch`/`agent_id`/`date` facets.
- **Code search (files)** is eventually consistent per spec: the
  `search_code_index` job registered in the existing `JobRegistry` walks
  the default branch (`git ls-tree -r -l`), skips binaries (NUL sniff)
  and blobs over 1 MiB, and indexes real file content with a `language`
  facet. It is triggered on default-branch pushes and runs hourly
  (alongside main's new `meta_spec_stale_pin_check` job — both are
  registered and spawned).
- **Real full reindex**: `POST /api/v1/admin/search/reindex` calls the
  coordinator, which clears the index first and then rebuilds from every
  domain repository (spec ledger + git blobs, tasks, MRs + reviews,
  agents, personas, activity events, audit events, agent-commit
  mappings, repo file trees), returning the true document count.

### Rebase-repair specifics (f38e8ad07)

- `api/spawn.rs`: main split the monolithic spawn path into
  `launch_agent_process` + `register_spawned_agent`/`SpawnBackend`;
  task-202's activity-event scope facets and search indexing were ported
  into the refactored `spawn_agent_core` (the analytics emit after the
  launch), and `monitor_spawned_agent_exit`'s `AuditCtx` construction
  gained the `search` field task-202 added to `container_audit`.
- `mem.rs`: unified `test_state_inner` on main's `ws_store` param plus
  task-202's `search` override param (8 args); all test-state helpers
  (`test_state`, `test_state_with_jj_ops`, `test_state_with_search`,
  `test_state_failing_trust`, `test_state_with_jj_and_git_ops`) aligned;
  test states keep the search-sync decorated repositories mirroring
  production wiring.
- `api/agents.rs`, `api/personas.rs`, `api/search.rs`, `jobs.rs`,
  `main.rs`: both sides' features preserved (secret revocation,
  persona-approval notifications, suggest/facets endpoints, both
  background jobs).

### Test evidence

- This round (sandbox, build-wrapper constrained):
  - `cargo check --workspace --all-targets` — clean (no errors/warnings).
  - `cargo test -p gyre-adapters search` — 23/23 passed (SQLite FTS5,
    mem, and Postgres PK-upgrade adapter suites).
  - `bash scripts/check-arch.sh` — passed.
  - Evidence: `/tmp/stage/review-evidence/task-202-rebase-repair.txt`.
- Deferred to the independent verifier (gyre-server codegen exceeds the
  sandbox's 4 GiB model+build budget):
  - `cargo test -p gyre-server --lib search_indexer` — includes
    `full_reindex_rebuilds_all_entity_types_from_real_storage` (seeds
    spec-by-blob-content, commit-by-message, task, MR, review, agent,
    persona, activity/audit events, and file docs against a real SQLite
    FTS5 backend; poisons the index with a stale doc; asserts every type
    is retrievable and the stale doc is dropped).
  - `cargo test -p gyre-server --test search_wiring` — FTS5 port wiring
    through the real router (`/api/v1/search`, `/suggest`, `/facets`,
    admin reindex).
  - `cargo test --all` and remaining CI gates.
