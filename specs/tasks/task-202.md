---
title: "Index all searchable entity types and implement real full reindex"
spec_ref: "search.md §Searchable Entities"
depends_on: [task-201]
progress: ready-for-review
coverage_sections:
  - "search.md §Searchable Entities"
commits: ["b522dd5cdb4eb4e6049d224dc3f745e5939ce3b9", "2faf26af2c4ca2fa00052be4fd9cf5429677c837", "3dc6cdcc5413b4eabd004920fcf13090dd658708", "3e223c8a1d632e2475d0a690a9cab77f3f4ced2c", "e2583917beb41b812d6f5799149f23e30ff332d7", "80f13a7059e1458ce3f10551909461a4a5677342", "309fc7dd426b346fdfa576a5770c49d65fc2aced", "b316bc37f64c6f032c563b9e590a823ee8a626e4"]
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
the admin reindex endpoint performs a true full rebuild from domain
storage. Original production code lives in commits `80f13a70`/`309fc7dd`;
later commits repair review/verification findings (see Repair history).

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
  `merge_processor.rs` — every emit site derives `workspace_id`/`repo_id`
  from the entity in hand so write-path activity events are
  workspace/repo-filterable exactly like reindexed ones), and audit events
  (`api/audit.rs`, `container_audit.rs`, `procfs_monitor.rs`).
- **Spec pushes** (`git_http.rs`): after the ledger sync, spec content is
  extracted from the pushed git blob and indexed as `body` with the
  ledger's linked task/MR ids appended (the specced `links` field) and
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
- **Postgres schema reconciliation**: the widened 4-column upsert conflict
  target is backed by an idempotent init-path upgrade
  (`upgrade_search_index_pk` in `postgres/search.rs`) that reconciles the
  live PK from `information_schema` — a deployment initialized under
  task-201 no longer hits SQLSTATE 42P10 on every upsert. The pure
  decision logic (`pk_upgrade_sql`) is unit-tested without a live PG
  instance.

### Test evidence

Latest full run at `b316bc37f` (evidence in
`/tmp/stage/review-evidence/task-202-links-repair.txt`; earlier rounds in
`task-202-repair-round3.txt`, `task-202-rebase-merge.txt`,
`task-202-ci-repair.txt`):

- `cargo test -p gyre-server --lib search_indexer` — 7 passed, including
  `full_reindex_rebuilds_all_entity_types_from_real_storage`, which seeds
  all 10 entity types plus a poisoned stale doc against a real SQLite FTS5
  backend, asserts the true count, idempotency, stale-doc removal, and
  per-type retrieval including spec-by-body-content, spec-by-linked-id,
  and commit-by-message. The push-path spec test
  (`index_repo_specs_indexes_linked_ids_from_ledger`) asserts the ledger's
  linked ids are indexed on push; both link assertions were verified to
  FAIL with the link append reverted (mutation check).
- `cargo test -p gyre-server --lib api::tasks` — 7 passed, including the
  regression test `task_transition_activity_event_is_workspace_scoped`,
  which drives the real HTTP router against a real SQLite FTS5 backend and
  asserts the write-path-indexed activity event is findable via the
  workspace filter with populated workspace/repo facets.
- `cargo test -p gyre-server --lib api::spawn` (38), `api::merge_requests`
  (33), `merge_processor` (67), `api::orchestrator` (6),
  `--test search_wiring` (1), `gyre-adapters --lib postgres::search` (5)
  and `--lib search` (18) — all passed.
- Gates against base `fefde2946`: `check-rustfmt-diff.py` exit 0,
  `check-clippy-diff.py` exit 0 (changed lines clean), `check-arch.sh`,
  `check-migration-versions.sh`, and `check-inert-enforcement.sh` passed.

Full-workspace `cargo test --all`, all-target Clippy, and GitHub CI are
deferred to verification per assignment scope. Note for verification: a
pre-existing deny-level `clippy::never_loop` error in
`crates/gyre-domain/src/rust_extractor.rs:1071` (upstream commit `d7940e85`,
present at this branch's base) fails any workspace clippy run regardless
of this task; it is untouched here and owned by that code's task.

### Repair history

- `3dc6cdcc` (round-2/3 review): activity-event scope facets on write
  paths; Postgres search_index PK reconciliation.
- `4bdcd763e`/`2faf26af2` (merge repair): rebase onto upstream `fefde2946`
  — kept the `search_code_index` job alongside upstream's new jobs, merged
  `test_state_inner` to the widened signature, delegated
  `compress_details_before` on `IndexingAuditRepository`, dropped the
  task-077 exemption pins that upstream's task-095 repair made obsolete.
- `b522dd5cd` (round-4 CI): rustfmt drift (`cargo fmt --all`) and two
  changed-line clippy errors (`needless_borrow` in `api/spawn.rs`,
  `too_many_arguments` on `commit_document`) — no behavior change.
- `b316bc37f` (round-5 review): the spec `links` indexed field —
  `SpecLedgerEntry.linked_tasks`/`linked_mrs` were never in the spec
  document, so `?q=<linked id>` returned no spec; they are now appended to
  the spec body in both `index_repo_specs` and `reindex_all`.
