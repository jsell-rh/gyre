# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities (all 10 entity rows) + §Index Updates.
Candidate: `f7a06f46` (branch `pipeline/task-202/8259cad3c3f58848d0190eeca2bc-1`), base `7e9b3cfe`.
Verdict: **complete** — approved, no blocking findings.

## Probes run (this checkout, evidence in `/tmp/stage/review-evidence/`)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` (via `/tmp/stage/bin/cargo`, cold cache) → **exit 0** in 7m17s (`cargo-check-gyre-server-tests.log`). Type-checks the whole server including every `#[cfg(test)]` change (mem.rs `TestStateOverrides` repair round included).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib search` → **23 passed, 0 failed** (`cargo-test-gyre-adapters-search.log`), including the task-touched adapter behavior: `reindex_all_clears_and_reports_count` (SQLite FTS5 — proves the adapter clears, not counts), `upsert_replaces_and_deletes` (scope-aware upsert identity), and the five `postgres::search::pk_upgrade_*` unit tests (PK reconciliation for the widened `ON CONFLICT (entity_type, entity_id, workspace_id, repo_id)` target).
- `bash scripts/check-arch.sh` → OK (server coordinator imports only `gyre-ports`/`gyre-domain`; FTS SQL stays in `gyre-adapters`).
- `bash scripts/check-in-memory-state-stores.sh` → OK.
- `rustfmt --edition 2021 --check` on all five largest changed Rust files → clean.
- `git diff --stat 7e9b3cfe f7a06f46 -- scripts/ specs/coverage/ docs/ web/` → empty: no gate weakened, no exemption touched, no out-of-scope edits; diff surface is `crates/gyre-server`, `crates/gyre-adapters`, `specs/tasks/task-202.md` only.
- gyre-server test *execution* is wrapper-blocked in the review stage (recorded: wrapper rejects `cargo test -p gyre-server` with the 4 GiB codegen rationale) — see deferred checks.

## Verified against the spec table, row by row

Every §Searchable Entities row checked against the document builder in `crates/gyre-server/src/search_indexer.rs`:

- **Spec** (`spec_document` + `index_repo_specs`): title/path/full content (extracted from the pushed git blob via `git show <sha>:specs/<path>` — real blob content, not a stub)/owner/links (ledger `linked_tasks`/`linked_mrs` appended to body, the same pattern as task labels); facets workspace, repo, approval_status, scope (first path segment), owner.
- **Task** (`task_document`): title/description/labels; facets workspace, repo, status, priority, assigned_to. `workspace_id` now set (was `None` at base — the workspace filter could never match).
- **MR** (`mr_document`): title/source_branch/target_branch/author; facets workspace, repo, status, has_conflicts, author.
- **Commit** (`commit_document` + `index_pushed_commit`): message (read from the git object store via `git show -s --format=%an%x1f%at%x1f%B`)/SHA/author/branch; facets workspace, repo, agent_id, date (ISO).
- **Agent** (`agent_document`): name/persona/status; facets workspace, repo, status.
- **Persona** (`persona_document`): name/prompt content (`system_prompt` as body); facet scope (Tenant→global, Workspace→workspace, Repo→repo — matches search.md line 125's `scope:global`/`scope:workspace` facet vocabulary).
- **Activity Event** (`activity_event_document`): description/event_type/agent_id; facets workspace, repo, event_type, date. Write-path emitters now carry workspace_id/repo_id in properties (tasks.rs, spawn.rs, merge_requests.rs, merge_processor.rs, orchestrator.rs) — pinned by `task_transition_activity_event_is_workspace_scoped` (real SQLite FTS5 + router oneshot).
- **Audit Event** (`audit_event_document`): event_type/details/path; facets workspace, repo, agent_id, event_type, path, date.
- **Review** (`review_document`): body/decision/reviewer joined to the MR for workspace/repo/mr facets.
- **File** (`file_document` + `index_repo_files`): path/content via `git ls-tree -r -l` + `cat-file blob`, NUL-sniff binary skip, 1 MiB cap; facet language (extension map). Runs as the `search_code_index` job in the existing `JobRegistry` (registered in `jobs.rs`, spawned in `main.rs`, triggered on default-branch push in `git_http.rs`) — async per §Index Updates, no new runtime invented.

## Write-path coverage (§Index Updates "synchronously on every write")

The candidate's strongest structural choice: `search_sync.rs` wraps the seven entity repositories (agents, tasks, MRs, reviews, personas, analytics, audit) in indexing decorators at `build_state`/`test_state_inner` construction time. Every write through the port — REST handlers, MCP tools, spawn/orchestrator/compose, merge processor, stale-agent detector, container auditor, procfs monitor, spec-lifecycle automation — re-indexes synchronously; deletes de-index. This closes the base-state gap the coverage matrix flagged ("update paths never reindex; status/priority facets freeze at create-time values") mechanically rather than by per-callsite discipline, and `update_status` re-fetches the entity so the status facet reflects the change. Handler-level manual `search_indexer::index` calls are redundant with the decorators (harmless — upsert), not a substitute for them.

## Real full reindex

`search_indexer::reindex_all(&state)` clears the index first (all three adapters' `reindex_all` now clear and report the dropped count — SQLite DELETE, PG DELETE, mem `docs.clear()`), then rebuilds from every domain repository: spec ledger + git blobs, tasks, MRs + reviews, agents, personas, analytics events, audit events, agent-commit mappings (message re-read from git), and repo file trees. `reindex_handler` (`api/search.rs`) now calls the coordinator — the base's no-op count is gone. Route unchanged (`POST /api/v1/admin/search/reindex`, pre-existing ABAC entry), so no new route-registry exposure.

## Test quality

`full_reindex_rebuilds_all_entity_types_from_real_storage` (search_indexer.rs) is genuinely anchored: seeds all 10 types against a **real SQLite FTS5 backend** (`test_state_with_search`), poisons the index with a stale doc (`zzzghoststaledoc`), asserts `count == 12` (kills any skipped type), asserts idempotence on a second run, asserts the stale doc is dropped (kills count-only regressions), and retrieves each type by distinctive content terms (spec-by-blob-content, commit-by-message, file-by-content, persona-by-prompt, etc.) with facet assertions. Push-path tests cover blob-content extraction, ledger-link indexing, commit message/author reading, and binary/oversized blob skipping. The coordinator logic and test were line-reviewed; type-check passes; the adapter half of the behavior ran green here.

## Findings

- (none blocking)

Non-blocking observations for the record:

1. `SearchPort::reindex_all`'s trait doc still reads "Rebuild the entire index from scratch. Returns number of documents indexed." while all three adapters now clear-and-report-dropped (the rebuild half moved to the server coordinator). Contract behavior is consistent and tested across adapters; the doc sentence is stale and worth a one-line fix in a later pass.
2. `index_pushed_commit` falls back to indexing an empty-message doc when git metadata read fails rather than skipping — cosmetic.
3. Activity-event search-doc scope comes from event `properties`; on the REST analytics endpoint those properties are caller-supplied. This inherits the §Access Scoping gap explicitly assigned to task-203 (search filtering itself is client-param scoped there) — not a regression introduced here (activity events were not indexed at all at base).
4. `specs/coverage/system/search.md` rows 3/6 were not updated by the candidate — consistent with this repo's process (rows 4/5 for delivered task-201 are likewise still `task-assigned` on main; the auditor flips rows).

## Deferred to the independent verifier (wrapper-blocked here)

- `cargo test -p gyre-server --lib search_indexer` — must show `full_reindex_rebuilds_all_entity_types_from_real_storage`, `index_repo_specs_extracts_blob_content_from_pushed_commit`, `index_repo_specs_indexes_linked_ids_from_ledger`, `index_pushed_commit_reads_message_and_author`, `file_indexing_skips_binary_and_oversized_blobs`, `iso_date_formats_epoch_days`, `language_for_maps_extensions` passing.
- `cargo test -p gyre-server --test search_wiring` (FTS5 port wiring through the real router).
- `cargo test -p gyre-server --lib task_transition_activity_event_is_workspace_scoped` (in api::tasks).
- `cargo test --all`, the clippy diff gate (the last repair round `3e5f747b9` restructured `test_state_inner` to 5 args precisely to clear `too_many_arguments`; `cargo check --tests` here confirms it compiles), and `bash scripts/check-arch.sh` in the full battery.

— Review, 2026-11-12
