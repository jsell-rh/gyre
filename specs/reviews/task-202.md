# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities + §Index Updates.
Candidate `d90877402effaa0430086b022151e10817ab73a5` vs base `a218b9565608f4bf0dc61477688013b0264c2edc`.
This round verifies the repair commit `5acae62c9` ("make specced Indexed Fields searchable") against the two findings from review checkpoint `5fe0f18c`, plus the full task contract.

Verdict: **approved** (findings: none).

## Prior-round findings — both fixed

1. **Agent `persona` never indexed** — `agent_document` (search_indexer.rs) now folds `persona_id` into both the body (`"\npersona {id}"`) and a `persona` facet. Test `full_reindex_rebuilds_all_entity_types_from_real_storage` asserts an agent with an attached persona is findable by `?q=persona-1` while the seeded agent name/body contains neither token — the assertion can only pass via the new body field.
2. **Specced Indexed Fields implemented as filter-only facets** (commit author/branch, review decision/reviewer, spec owner, activity/audit agent_id) — all now folded into the document body (MR-author pattern). Tests assert searchability for each (`Testcommitter`, `main`, `approved`, `agent-reviewer-9`, `user:test`, `agent-1`) with seeded bodies deliberately excluding those values, so each assertion is anchored to the fold, not to incidental content. `GET /api/v1/search` has no facet-value filter params, so body-folding is the correct interpretation of the spec's Indexed Fields column.

## Task contract verification (source + probes)

- **All 10 entity types** have document builders in `crates/gyre-server/src/search_indexer.rs` with the specced indexed fields and facets (checked row-by-row against search.md's table): spec (title/path/content/owner/links + approval_status/scope facets, scope = first path segment), task, mr (author in body + facet), commit (sha=entity_id, message=body, author/branch in body + facets, date facet via ISO `iso_date`), agent (persona in body + facet), persona (prompt content=body, scope facet from `PersonaScope`, tenant→"global"), activity_event (description from properties, event_type/agent_id in body + facets, date), audit_event (details/path body, event_type/agent_id/path facets), review (body/decision/reviewer, mr facet, scope joined from the MR), file (content=body, language facet from extension).
- **Write paths** all reach `SearchPort::index`: REST handlers (tasks, agents, MRs, reviews, personas, analytics, audit) call `search_indexer::index` explicitly, and *every* entity repository on `AppState` is additionally wrapped in `search_sync` decorators at construction (lib.rs `build_state` for both the SQLite/PG `store!` path and the mem fallback; mirrored in `mem.rs::test_state_inner`). Status transitions (`update_status`, `transition_mr_status`, `update_task`, persona approve/update) re-index; delete paths de-index (agent/task/mr/persona). `container_audit`, `procfs_monitor`, `audit_simulator`, `merge_processor`, `spawn`, `orchestrator` emit paths all index.
- **Spec push**: `git_http.rs` calls `index_repo_specs` after ledger sync — content read via `git show {head_sha}:specs/{path}` from the pushed blob; ledger-sourced title/owner/approval_status/linked ids facets. `sync_spec_ledger` stores the per-spec **blob** SHA in `current_sha` (verified via `get_blob_sha` in spec_registry.rs), so `reindex_all`'s `git cat-file blob current_sha` reads real content in production, not just in the test seeding.
- **Commit push**: `record_pushed_commits` calls `index_pushed_commit` — author/timestamp/message from `git show -s --format=%an%x1f%at%x1f%B`; branch normalized off `refs/heads/` identically on push and reindex paths.
- **File/code search**: `search_code_index` job registered in the existing `JobRegistry` (jobs.rs), spawned hourly in main.rs and triggered on default-branch pushes via `job_registry.trigger` (async, not inline). `index_repo_files` walks `git ls-tree -r -l`, skips non-blobs, >1 MiB, and NUL-sniffed binaries; indexes real UTF-8 content. Test proves binary/oversized skipping.
- **Real full reindex**: `reindex_all` (server coordinator) first calls the port's `reindex_all`, whose adapter contract is now "clear and report dropped count" in all three adapters (mem_search.rs clears; SQLite `DELETE FROM search_index` — proven by adapters test `reindex_all_clears_and_reports_count`). It then iterates spec ledger (+git blobs), tasks, MRs+reviews, agents, personas, analytics, audit (bounded at 100k), agent-commit mappings (+git messages), and default-branch file trees, returning the true count. `reindex_handler` (`POST /api/v1/admin/search/reindex`) invokes the coordinator.
- **Scope handling**: every index path funnels through `search_scope::with_tenant_resolution` (port-level decorator), so documents carry tenant resolved from the workspace scope row — never a fabricated `"default"`. Task/agent docs now carry real `workspace_id` (the coverage row-6 bug where they were indexed with `workspace_id: None` is gone). Adapter upsert/delete identity widened to (entity_type, entity_id, workspace_id, repo_id) so same-path specs/files in different repos coexist; the PG adapter reconciles a task-201-era 2-column PK on init (otherwise every 4-col `ON CONFLICT` would silently fail — well-reasoned, unit-tested).
- **Test quality** (acceptance criterion 5): the coordinator test seeds all 9 repo-backed types plus a real temp git repo (spec by content, file by content, commit by message), poisons the index with a ghost doc, asserts `count == 12`, idempotence (`count2 == 12`), ghost-doc absence (rebuild-not-count proof), and per-type retrievability with distinctive terms (`quokkafederation`, `wombatintegration`, …) against a real SQLite FTS5 backend (`SqliteStorage` via `test_state_with_search`). Seeding goes through the decorated repositories, but the ghost assertion plus exact-count assertion make it fail if any type is skipped in the rebuild or if stale docs are counted.

## Probes run (evidence in /tmp/stage/review-evidence)

- `cargo check -p gyre-server` → **EXIT=0** (7m26s cold; web assets built).
- `cargo check -p gyre-server --lib --tests` → **EXIT=0** (3m09s; test code type-checks clean, no warnings).
- `cargo test -p gyre-adapters --lib search` → **30 passed, 0 failed** (incl. `reindex_all_clears_and_reports_count`, `upsert_replaces_and_deletes`, `access_scoping_filters_inside_the_index_query`, PG `pk_upgrade_*`).
- `bash scripts/check-arch.sh` → OK (hexagonal boundaries preserved; FTS SQL stayed in gyre-adapters, coordinator only calls the port).
- `bash scripts/check-forged-scope-fields.sh` → OK.
- `git diff --check` on the candidate range → clean; `scripts/` and `*-exemptions.txt` untouched by the diff (no gate weakening).

## Verifier must run (budget rule: gyre-server test compilation deferred)

- `cargo test -p gyre-server --lib search_indexer` — the acceptance test battery (`full_reindex_rebuilds_all_entity_types_from_real_storage`, `index_repo_specs_extracts_blob_content_from_pushed_commit`, `index_repo_specs_indexes_linked_ids_from_ledger`, `index_pushed_commit_reads_message_and_author`, `file_indexing_skips_binary_and_oversized_blobs`).
- `cargo test --all` (task acceptance criterion).
- Full static-check battery per CI.

## Non-blocking observations (advisory, not findings)

1. **Activity-event REST scope attribution**: `activity_event_document` derives workspace/repo facets from `event.properties`. All server-internal emit paths set these from real records (task/mr/repo), but `POST /api/v1/analytics/events` (`api/analytics.rs::record_event`) forwards caller-supplied JSON, so a caller permitted to record analytics can scope their (own-authored) event's search document to an arbitrary workspace — a search-result pollution vector, not data exfiltration. Underlying design is pre-existing: the analytics event stream has no workspace scoping anywhere at base (`GET /api/v1/analytics/events` returns all events unscoped), and `AnalyticsEvent` carries no server-known scope field. `check-forged-scope-fields.sh` passes because the field flows through the properties JSON rather than `req.<field>`. Recommend a follow-up (derive from the authenticated caller's workspace, or validate `properties.workspace_id` against the caller's accessible set).
2. Several REST handlers index explicitly *and* through the decorator (double upsert per write). Idempotent by the adapter's delete-then-insert upsert; harmless redundancy, kept because the explicit calls document the write-path contract.
3. `specs/coverage/system/search.md` row 3 still reads `task-assigned` — coverage rows are updated by the audit process in this repo (task-198's merge likewise left its row to the auditor), not by implementation commits; noting so the auditor re-audits the row against this delivery.

— Review, 2026-11-12 (candidate d9087740)
