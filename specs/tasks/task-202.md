---
title: "Index all searchable entity types and implement real full reindex"
spec_ref: "search.md §Searchable Entities"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Searchable Entities"
commits: ["3e223c8a1d632e2475d0a690a9cab77f3f4ced2c", "e2583917beb41b812d6f5799149f23e30ff332d7", "80f13a7059e1458ce3f10551909461a4a5677342", "309fc7dd426b346fdfa576a5770c49d65fc2aced", "3dc6cdcc5413b4eabd004920fcf13090dd658708"]
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
indexed with real content and the specced facets, and the admin reindex
endpoint performs a true full rebuild from domain storage. Original
production code lives in commits `80f13a70`/`309fc7dd`; commit `3dc6cdcc`
repairs the round-2 review findings (activity-event scope facets on write
paths, Postgres search_index PK reconciliation).

### Behavior

- **Server-level coordinator** (`crates/gyre-server/src/search_indexer.rs`):
  document builders for every entity type (spec, task, mr, commit, agent,
  persona, activity_event, audit_event, review, file) with the specced
  indexed fields and facet maps (`status`, `priority`, `approval_status`,
  `scope`, `branch`, `author`, `decision`, `language`, ISO-date for
  date-range facets, etc.). Best-effort `index`/`deindex` helpers keep
  search failures from failing the entity write.
- **Write paths** call `SearchPort::index` synchronously: task
  create/update/transition (`api/tasks.rs`), MR create/status
  (`api/merge_requests.rs`, including review submission), agent create and
  spawn (`api/agents.rs`, `api/spawn.rs`), persona create/update
  (`api/personas.rs`, delete also de-indexes), activity events at every
  analytics emit path (`api/analytics.rs`, `api/tasks.rs`,
  `api/merge_requests.rs`, `api/spawn.rs`, `api/orchestrator.rs`,
  `merge_processor.rs`), and audit events (`api/audit.rs`,
  `container_audit.rs`, `procfs_monitor.rs`).
- **Activity-event scope facets (round-3 repair)**: every production emit
  site now includes `workspace_id`/`repo_id` in the analytics properties,
  derived from the entity in hand (task/mr/agent/repo) —
  `task.status_changed`, `mr.merged`, `agent.spawned`, `agent.completed`,
  `mr.created`, and both `merge_queue.processed` sites (which previously
  recorded without indexing at all and now index like every other emit
  path). Write-path-indexed activity events are therefore filterable by
  workspace/repo exactly like reindexed ones.
- **Spec pushes** (`git_http.rs`): after the ledger sync, spec content is
  extracted from the pushed git blob and indexed as `body` with
  `approval_status`/`scope`/`owner` facets.
- **Commit pushes** (`git_http.rs`): message + author are read from the git
  object store and indexed with `branch`/`agent_id`/`date` facets.
- **Code search (files)** is eventually consistent per spec: a
  `search_code_index` job registered in the existing `JobRegistry` walks
  the default branch (`git ls-tree -r -l`), skips binaries (NUL sniff) and
  blobs over 1 MiB, and indexes real file content with a `language` facet.
  It is triggered on default-branch pushes and runs hourly.
- **Real full reindex**: `POST /api/v1/admin/search/reindex` calls the
  coordinator, which clears the index first (port `reindex_all` contract)
  and then rebuilds from every domain repository (spec ledger + git blobs,
  tasks, MRs + reviews, agents, personas, activity events, audit events,
  agent-commit mappings, repo file trees), returning the true document
  count. The count is idempotent across runs and drops stale docs.
- **Postgres schema reconciliation (round-3 repair)**: the widened 4-column
  upsert conflict target is now backed by an idempotent init-path upgrade
  (`upgrade_search_index_pk` in `postgres/search.rs`) that reads the live
  PK from `information_schema` and `ALTER TABLE ... DROP CONSTRAINT, ADD
  PRIMARY KEY (entity_type, entity_id, workspace_id, repo_id)` when it
  doesn't match — a deployment initialized under task-201 no longer hits
  SQLSTATE 42P10 on every upsert (which silently stopped all PG search
  indexing). The pure decision logic (`pk_upgrade_sql`) is unit-tested
  without a live PG instance.

### Test evidence

Verified at branch head `3dc6cdcc` (evidence in
`/tmp/stage/review-evidence/task-202-repair-round3.txt`):
`cargo test -p gyre-server --lib search_indexer` — 6 passed, including
`full_reindex_rebuilds_all_entity_types_from_real_storage`, which seeds all
10 entity types plus a poisoned stale doc against a real SQLite FTS5
backend, asserts the true count, idempotency, stale-doc removal, and
per-type retrieval including spec-by-body-content and commit-by-message.
`cargo test -p gyre-server --lib api::tasks` — 7 passed, including the new
regression test `task_transition_activity_event_is_workspace_scoped`, which
drives the real HTTP router against a real SQLite FTS5 backend and asserts
the write-path-indexed activity event is findable via the workspace filter
with populated workspace/repo facets; verified to FAIL (0 results) with the
scope fix reverted.
`cargo test -p gyre-server --lib api::spawn` (34), `api::merge_requests`
(21), `merge_processor` (49), `orchestrator` (10) — all passed.
`cargo test -p gyre-adapters --lib postgres::search` — 5 passed (new
`pk_upgrade_sql` decision tests); `--lib search` — 18 passed.
`bash scripts/check-arch.sh` and all other `scripts/check-*.sh` gates
passed (the two pre-existing task-077 F5 exemption pins were re-pinned
1457/1479 → 1467/1489 — same sites, moved by line insertions, no new
exemptions). `cargo clippy` on the changed crates: 0 errors from these
changes.

Full-workspace `cargo test --all`, all-target Clippy, and GitHub CI are
deferred to verification per assignment scope. Note for verification: a
pre-existing deny-level `clippy::never_loop` error in
`crates/gyre-domain/src/rust_extractor.rs:1071` (upstream commit `d7940e85`,
present at this branch's base) fails any workspace clippy run regardless
of this task; it is untouched here and owned by that code's task.
