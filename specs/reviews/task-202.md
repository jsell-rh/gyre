# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities (10-entity table) + §Index Updates (spec pushed → index updated with content from git blob; commit pushed → messages indexed; file/code search async via background job).
Candidate under review: `da459c919f41550ed7567d9d2dbc654d48c35e6c` (branch `pipeline/task-202/8cee59baabf7496eae9df8ea6df62178-1`), diffed against assigned base `fefde2946`. All six `commits:` frontmatter entries are in the candidate's lineage.
Verdict: **approved** (round 4; this review independently re-probed the final rebased state).

## What shipped (verified in the diff, not from labels)

- `crates/gyre-server/src/search_indexer.rs` (new, 1265 lines): document builders for all 10 entity types with the specced indexed fields and facet maps (task status/priority/assigned_to; mr status/has_conflicts/author; agent status; persona scope; activity event event_type/agent_id/date; audit event event_type/agent_id/path/date; commit author/branch/agent_id/date; spec approval_status/scope/owner; file language; workspace/repo facets everywhere). Git object-store helpers (`git show`/`cat-file`/`ls-tree -r -l`) for spec blobs, commit metadata, and file trees. Real full-reindex coordinator: clears the index via the port contract, then rebuilds from every domain repository (spec ledger + git blobs, tasks, MRs + reviews, agents, personas, analytics events, audit events, agent-commit mappings, repo file trees) and returns the true count.
- `crates/gyre-server/src/search_sync.rs` (new): repository decorators wrapping agent/task/mr/review/persona/analytics/audit repositories so **every** write path — REST, MCP, background jobs, merge processor, spec lifecycle — re-indexes/de-indexes synchronously, best-effort. Wired identically in production `build_state` (lib.rs) and test `test_state_inner` (mem.rs). This is stronger than per-handler calls: it closes the fan-out the task plan worried about.
- `git_http.rs`: spec content indexed from the pushed git blob after ledger sync; commit messages+author indexed in `record_pushed_commits`; `search_code_index` job triggered on default-branch pushes (async, per spec).
- `jobs.rs`/`main.rs`: `search_code_index` registered in the existing `JobRegistry` (hourly interval) — no new async runtime.
- Write-path handlers (`api/tasks.rs`, `agents.rs`, `personas.rs`, `merge_requests.rs`, `spawn.rs`, `orchestrator.rs`, `analytics.rs`, `audit.rs`, `merge_processor.rs`, `container_audit.rs`, `procfs_monitor.rs`, `audit_simulator.rs`): explicit re-index calls plus workspace/repo scope facets on every activity-event emit site (`task.status_changed`, `mr.merged`, `agent.spawned`, `agent.completed`, `mr.created`, `merge_queue.processed` ×2).
- Adapters: `mem_search.rs` `reindex_all` now actually clears (contract match); SQLite upsert delete scoped by the 4-column identity (type, id, workspace, repo) so same-path docs in different repos coexist; Postgres 4-column PK backed by an idempotent init-path upgrade (`upgrade_search_index_pk`) with pure decision-logic unit tests — repairs the 42P10 silent-indexing failure a task-201-initialized deployment would otherwise hit.
- `api/search.rs` `reindex_handler` now calls the coordinator, not the port no-op. Route `POST /api/v1/admin/search/reindex` pre-existing.

## Test runs (this checkout at `da459c919`; evidence in `/tmp/stage/review-evidence/task202-probe-*.txt`)

- `cargo test -p gyre-server --lib search_indexer` → **6 passed** (incl. `full_reindex_rebuilds_all_entity_types_from_real_storage`: seeds all 10 types + a poisoned stale doc against a real SQLite FTS5 backend, asserts true count 12, idempotency, stale-doc removal, and per-type retrieval incl. spec-by-body-content and commit-by-message).
- `cargo test -p gyre-server --lib` focused: `api::tasks` 7, `api::personas` 8, `api::analytics` 9, `api::audit` 12, `api::orchestrator` 6, `api::spawn` 38, `api::merge_requests` 33, `merge_processor` 67 — all passed.
- `cargo test -p gyre-adapters --lib search` → **18 passed** (SQLite FTS5 schema/roundtrip/filters/reindex-clears; Postgres `pk_upgrade_sql` decision tests).
- `cargo test -p gyre-server --test search_wiring` → **1 passed** (sqlite URL wires the FTS5 port).
- `bash scripts/check-arch.sh` OK; `check-inert-enforcement.sh` OK; `check-mem-port-contracts.sh` OK; `check-forged-scope-fields.sh` OK; `check-in-memory-state-stores.sh` OK; `check-scope-literal-defaults.sh` OK; `check-fail-open-ref-resolution.sh` OK.

## Mutation probes (each restored; `git status --short` clean at candidate SHA afterwards)

1. **Skip the personas repository in `reindex_all`** → `full_reindex_rebuilds_all_entity_types_from_real_storage` **FAILS** (`left: 11, right: 12`). The test catches any skipped entity type.
2. **Disable the clear-before-rebuild** (the exact "counts stale docs instead of rebuilding" failure mode) → **FAILS** ("stale document survived the reindex"). The rebuild semantics are genuinely asserted, not assumed.
3. **Drop the workspace/repo scope facets from the task-transition analytics emit** → `task_transition_activity_event_is_workspace_scoped` **FAILS** (0 results via the workspace filter). The round-2/3 regression test is anchored to the production behavior, not to the test's own construction.

## Acceptance criteria check

- All 10 types indexed with specced fields/facets, write paths synchronous (file via background job): **yes** — builders + decorators + handler calls; decorator wrapping means even MCP/background write paths sync.
- Spec content from git blob on push indexed as body: **yes** — `git show {head}:specs/{path}`, test asserts retrieval by body content term.
- Reindex endpoint rebuilds and returns true count: **yes** — coordinator clears then rebuilds; mutation 2 proves it.
- Search finds each type after reindex; spec-by-body query returns the spec: **yes** — asserted per-type in the coordinator test.
- Test with ≥4 types incl. spec-by-content and commit-by-message that fails on skip/stale-count: **yes** — 10 types; mutations 1 and 2 prove the failure modes are caught.
- `cargo test --all` + `check-arch.sh`: check-arch passes; all focused suites green; full-workspace suite deferred to verification per review scope. Note for verification: a pre-existing deny-level `clippy::never_loop` in `crates/gyre-domain/src/rust_extractor.rs:1071` exists at this branch's base (upstream `d7940e85`), untouched by this diff — not a task-202 defect.

## Non-blocking notes

- The spec `scope` facet is interpreted as the spec's top-level section (first path segment, e.g. `system`), matching the task plan's facet list `{approval_status, scope, workspace, repo, owner}`; search.md's "(global/workspace/repo)" annotation attaches to the Persona row, so this reading is defensible.
- Handlers and decorators both index on some write paths (double upsert) — idempotent and harmless.
- Coverage-matrix row updates (`specs/coverage/system/search.md` still shows `task-assigned`) belong to the verification/audit stage, consistent with how task-201's approval left them.
