# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities + §Index Updates.
Candidate under review: `870c2295b325eb5e4ea737aa4835f5b5f435c9d6` (branch `pipeline/task-202/25851ec9e047454a99c3ba8a265fca6b-2`, base `d81b0fe4`). Scoping per `commits:` frontmatter: `3e223c8a`, `e2583917`, `80f13a70`, `309fc7dd`.
Verdict: **needs revision** (2 findings — the core delivery is real and independently verified; both findings are scoped gaps, not fakery).

## Probes run (evidence: `/tmp/stage/review-evidence/task-202-*.txt`)

- `cargo test -p gyre-server --lib search_indexer` → **6 passed, 0 failed** (incl. `full_reindex_rebuilds_all_entity_types_from_real_storage`: seeds all 10 types + a poisoned stale doc against a real SQLite FTS5 backend, asserts true count 12, idempotency, stale-doc removal, and per-type retrieval incl. spec-by-body and commit-by-message).
- `cargo test -p gyre-adapters --lib search` → **13 passed, 0 failed** (FTS5 schema, porter stemming, upsert/delete, entity/workspace filters, `reindex_all_clears_and_reports_count`).
- `bash scripts/check-arch.sh` → **passed** (hexagonal boundaries intact; FTS SQL stayed in `gyre-adapters`, coordinator in `gyre-server`).
- Independent write-path sync probe (temporary unit test, file restored byte-identical after — `git status` clean): created task/MR/review/persona/activity/audit/agent through the decorated repositories against a real SQLite FTS5 backend and asserted each is searchable **without any reindex**, that `update_status` refreshes the `status` facet, and that task/persona/agent deletes de-index. **All assertions passed.** This kills the "decorators are dead code" hypothesis — the `search_sync` wrappers are wired in every mode (`build_state` wraps whatever `store!` resolves: PG, SQLite, or mem; `mem::test_state_inner` mirrors it).

## Verified real (no findings)

- **Reindex coordinator is a true rebuild**: clears via the port `reindex_all` contract (all three adapters now clear-and-report, incl. the mem adapter's former no-op), then iterates spec ledger + git blobs, tasks, MRs + reviews, agents, personas, analytics events, audit events, agent-commit mappings, and repo file trees. `POST /api/v1/admin/search/reindex` (`api/search.rs`) invokes it and returns the true count. ABAC mapping present (`abac_middleware.rs:378`, admin/write).
- **Spec pushes**: `git_http.rs` indexes after the ledger sync, inside the default-branch filter, content read from the pushed git blob (`git show {sha}:specs/{path}`); unledgered specs default to `approval_status=pending`. Manifest path convention matches `spec_registry::read_manifest_paths`.
- **Commit pushes**: every pushed commit (all pushes go through `record_pushed_commits`) is indexed with message/author read from the git object store, branch normalized off `refs/heads/`, plus `agent_id`/`date` facets.
- **Code search is a real background job**: `search_code_index` registered in the existing `JobRegistry` (hourly + spawned in `main.rs` + triggered on default-branch pushes), walks `git ls-tree -r -l`, skips binaries (NUL sniff) and blobs > 1 MiB, indexes real content with a `language` facet. Verified by `file_indexing_skips_binary_and_oversized_blobs`.
- **Scope-aware upsert identity** (type+id+workspace+repo) in SQLite/PG/mem adapters, so same-path specs/files in different repos coexist.
- Tests are hard, not self-confirming: distinctive probe terms, exact counts, stale-doc poisoning, idempotency, per-type retrieval — a skipped type or a stale-count reindex fails them.

## Findings (details + reproduction in `/tmp/stage/review-evidence/task-202-findings-evidence.txt`)

1. **Activity-event workspace/repo facets are hollow on 6 of 7 emit paths** (`api/tasks.rs:335`, `api/merge_requests.rs:633`, `api/spawn.rs:1109/1341/1357`, `merge_processor.rs:757/1528`). `activity_event_document` reads scope only from `event.properties`, and those sites don't include `workspace_id`/`repo_id` (the scope is in hand at each site); only `api/orchestrator.rs:136` does. Those events are stored with `workspace_id=''` and empty workspace/repo facets, so workspace-scoped search (`?workspace_id=`) excludes them and client-side facet filtering sees empty values — against search.md's Activity Event facet row (workspace, repo) and the task plan item 4. Trivial fix: add the scope keys to the analytics `json!` at the listed sites.
2. **PG schema change has no upgrade path** (`postgres/search.rs:45,96`). The `search_index` PK was widened to (entity_type, entity_id, workspace_id, repo_id) and the upsert conflict target changed to match, but the table is `CREATE TABLE IF NOT EXISTS` on the PG-only init path with no ALTER/migration. An existing PG deployment (task-201 schema, 2-col PK) makes every `ON CONFLICT (4 cols)` fail with 42P10; best-effort indexing then silently stops all PG search indexing, and the admin reindex reports a false count (the coordinator counts entities regardless of per-doc index failures). No CI job exercises PG, so green suites cannot catch it. Fix: idempotent constraint reconciliation on the PG init path.

## Notes (not findings)

- Redundant double-indexing: several handlers (e.g. `api/audit.rs`, `api/analytics.rs`, `api/tasks.rs`, `procfs_monitor.rs`) call `search_indexer::index` explicitly *and* write through the now-decorated repositories, indexing the same document twice per write. Upsert semantics make this harmless; worth cleaning up eventually.
- `SearchPort::delete` remains scoped by (entity_type, entity_id) only while upsert identity is 4-column; no current de-index callsite uses a non-unique entity_id (spec/file paths), so no live bug — but a future spec/file de-index would cross-delete same-path docs in other repos.

— Reviewer, independent review round (candidate `870c2295`)
