# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `search.md` §Searchable Entities (all 10 entity types with specced indexed
fields + facets) and §Index Updates (spec pushed → index updated from git blob;
commit pushed → messages indexed; file/code search asynchronous via background
job). Candidate `39e49f00` on base `0cb71015`; production commits `309fc7dd`,
`80f13a70`, `e2583917`, `3e223c8a`, repair commit `3dc6cdcc` (this round's
subject), plus process-recording commits.

Verdict (round 4): **complete** — both durable round-2 findings are repaired
with real production code, the round-1 write-path-coverage findings stay fixed
via a structural decorator, and all focused probes pass, including a mutation
probe proving the new regression test kills the defect it targets.

## Round-2 finding 1 — activity-event scope facets on write paths: FIXED

Every production analytics emit site now derives `workspace_id`/`repo_id` from
the entity in hand and puts them in the event properties, which
`activity_event_document` (search_indexer.rs) reads for the specced
workspace/repo facets and the adapter-level workspace filter:

- `task.status_changed` (api/tasks.rs:335 area) — from `task.workspace_id`/`task.repo_id`
- `mr.merged` (api/merge_requests.rs:633 area) — from `mr`
- `agent.spawned` (api/spawn.rs:1109 area) — from `repo`
- `agent.completed` + `mr.created` (api/spawn.rs:1347/1369 area) — from `mr`
- `agent.spawned` orchestrator path (api/orchestrator.rs:136) — already scoped
- both `merge_queue.processed` sites (merge_processor.rs:757, 1538) — from
  `updated_mr`; these previously recorded without indexing at all and now
  index like every other emit path.

Regression test `task_transition_activity_event_is_workspace_scoped`
(api/tasks.rs) drives the real HTTP router (`POST /api/v1/tasks` then
`PUT .../status`) against a real SQLite FTS5 backend and asserts the
write-path-indexed event is findable via `workspace_id=ws-scope-1` with
populated workspace/repo facets. Mutation probe (this review, restored and
verified clean afterwards): replacing the `workspace_id`/`repo_id` property
values with empty strings at the tasks.rs emit site makes the test FAIL
(0 results vs 1 expected) — the test is genuinely anchored to the production
behavior, not self-confirming.

## Round-2 finding 2 — Postgres search_index PK reconciliation: FIXED

`postgres/search.rs` now runs `upgrade_search_index_pk` on the PG-only init
path right after the CREATE TABLE: it reads the live PK columns from
`information_schema.table_constraints`/`key_column_usage` (ordered by
ordinal_position) and, when they don't equal
`(entity_type, entity_id, workspace_id, repo_id)`, executes
`ALTER TABLE search_index DROP CONSTRAINT <quoted name>, ADD PRIMARY KEY
(entity_type, entity_id, workspace_id, repo_id)` — so a deployment initialized
under task-201's 2-column PK no longer hits SQLSTATE 42P10 on every upsert
(which, being best-effort, silently emptied the index). The decision function
`pk_upgrade_sql` is pure and unit-tested (5 tests: no-op when matching, drop
task-201 2-col key, hostile constraint-name quoting, add-when-missing,
reconcile any non-matching key). This is init-path reconciliation, matching
the SQLite side's situation (FTS5 rowid table has no PK constraint, so the
4-column upsert delete-scope needed no migration there).

## Round-1 findings (write-path coverage) — fixed structurally, stayed fixed

Instead of per-callsite discipline, `search_sync.rs` (new) wraps the entity
repositories on `AppState` at construction time in both wiring paths —
production (`lib.rs` store! wiring) and test (`mem.rs test_state_inner`) —
so **every** write through the port re-indexes: `IndexingAgentRepository`
(create/update/update_status — the latter re-fetches to rebuild the full doc
so the `status` facet reflects the change — and de-index on delete),
`IndexingTaskRepository`, `IndexingMrRepository`, `IndexingReviewRepository`
(submit_review joins the MR for title/scope facets),
`IndexingPersonaRepository`, `IndexingAnalyticsRepository`,
`IndexingAuditRepository`. Reads pass through untouched, so the reindex
coordinator lists without side effects. Verified that the previously-flagged
paths all write through the wrapped `state.agents`/`state.tasks`:
spawn_agent_core (spawn.rs), orchestrator/compose registration,
complete_agent, stale_agents.rs, admin kill, MCP agent status updates,
MCP `gyre_update_task`, and spec-lifecycle task cancellation (api/specs.rs).
This is a stronger fix than the originally requested per-callsite calls — a
new write path cannot forget to index.

## Coordinator and remaining acceptance criteria — verified

- `reindex_all` (search_indexer.rs) clears via the port's `reindex_all`
  contract, then rebuilds from every domain repository: spec ledger (content
  via `read_blob(repo_path, entry.current_sha)` — production
  `sync_spec_ledger` stores the **blob** SHA, confirmed in
  spec_registry.rs:388/417, so this reads real content), tasks, MRs +
  reviews, agents, personas, analytics events (limit 100k), audit events,
  agent-commit mappings (message/author via `git show`), and repo file trees.
- `POST /api/v1/admin/search/reindex` (api/search.rs reindex_handler) invokes
  the coordinator, not the port no-op.
- Spec pushes: `index_repo_specs` runs on default-branch pushes after the
  ledger sync (git_http.rs), extracting content via
  `git show <sha>:specs/<path>`.
- Commit pushes: `index_pushed_commit` inside `record_pushed_commits`, reading
  `%an/%at/%B` from the object store.
- Code search: `search_code_index` job registered in the existing
  `JobRegistry` (interval 3600), spawned via `jobs::spawn_job` in main.rs, and
  triggered on default-branch pushes via `job_registry.trigger`; walks
  `git ls-tree -r -l` on the default branch, skips NUL-sniffed binaries and
  blobs over 1 MiB, indexes real content with a `language` facet.
- SQLite upsert delete is now scope-scoped (entity_type, entity_id,
  workspace_id, repo_id) so same-path specs/files in different repos coexist;
  MemSearchAdapter identity and `reindex_all` (clear + count dropped) match
  the same contract.

## Test runs (this checkout, candidate `39e49f00`)

- `cargo test -p gyre-server --lib search_indexer` → **6 passed, 0 failed**,
  including `full_reindex_rebuilds_all_entity_types_from_real_storage`: seeds
  all 10 types + a poisoned stale doc against a real SQLite FTS5 backend,
  asserts true count (12), idempotency, stale-doc removal, and per-type
  retrieval including spec-by-body-content and commit-by-message.
- `cargo test -p gyre-server --lib api::tasks` → **7 passed, 0 failed**
  (incl. the workspace-scope regression test).
- `cargo test -p gyre-server --lib` (full) → **1267 passed, 0 failed**.
- `cargo test -p gyre-adapters --lib` (full) → **352 passed, 0 failed**
  (12 ignored — live-Postgres-dependent).
- `cargo test -p gyre-adapters --lib postgres::search` → **5 passed**
  (pk_upgrade_sql decision tests); `sqlite::search` included in the full run.
- `bash scripts/check-arch.sh` → OK; `bash scripts/check-inert-enforcement.sh`
  → OK (the two task-077 F5 exemption pins re-pinned 1457/1479 → 1467/1489,
  same sites moved by line insertions above them — no new exemptions).
- Mutation probe (restored, `git status --porcelain` clean): scope-fix
  revert at the tasks.rs emit site → regression test fails. Evidence:
  `/tmp/stage/review-evidence/task-202-r10-*.txt`.

Minor observations (not defects): several emit sites index twice (once via
the `search_sync` wrapper on `state.analytics.record`, once via an explicit
`search_indexer::index` call) — redundant but idempotent upserts; the
adapter-level `delete(entity_type, entity_id)` is not scope-scoped, but it is
only ever invoked for UUID-identified entities (persona/task/mr/agent), never
for path-identified specs/files.

Findings:

- (none)

— Reviewer, round 4 (candidate `39e49f00`)
