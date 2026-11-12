---
title: "Index all searchable entity types and implement real full reindex"
spec_ref: "search.md §Searchable Entities"
depends_on: [task-201]
progress: not-started
coverage_sections:
  - "search.md §Searchable Entities"
commits: []
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
