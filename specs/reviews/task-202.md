# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities (10-row entity table) + §Index Updates.
Commits under review (scoping per `commits:` frontmatter): `309fc7dd`/`80f13a70` (original implementation), `e2583917` (bookkeeping), `3e223c8a` (checkpoint), `3dc6cdcc` (round-2/3 repair: activity-event scope facets + PG PK reconciliation), `2faf26af2` (rebase-merge repair onto base `fefde2946`), `b522dd5c` (CI repair: rustfmt/clippy), `b316bc37` (round-5 repair: spec `links` indexed field). Candidate reviewed: `542206ac4` on `pipeline/task-202/742848a403934941a26593fec03d2b49-1`, diffed against assigned base `fefde2946`.
Verdict: **complete** — approved with no findings.

## Round 6 (this review)

Test runs (this checkout at candidate `542206ac4`, cold build, evidence in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib search_indexer` → **7 passed, 0 failed** against a real SQLite FTS5 backend (`test_state_with_search` wires `SqliteStorage` as the `SearchPort`), including `full_reindex_rebuilds_all_entity_types_from_real_storage` (seeds all 10 types + a poisoned stale doc, asserts true count 12, idempotency, stale-doc removal, per-type retrieval incl. spec-by-body-content, spec-by-linked-id, commit-by-message).
- `cargo test -p gyre-server --lib api::tasks` → **7 passed**, incl. `task_transition_activity_event_is_workspace_scoped` (real HTTP router + SQLite FTS5; write-path activity event findable via the workspace filter).
- `cargo test -p gyre-server --test search_wiring` → **1 passed**.
- `cargo test -p gyre-adapters --lib search` → **18 passed** (SQLite FTS5 semantics, mem scope-keyed upsert, PG `pk_upgrade_sql` decision logic).
- Gates: `check-arch.sh`, `check-migration-versions.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh` → all exit 0.
- Full-workspace `cargo test --all` deferred to verification per assignment scope.

Mutation probes (source restored after each; `git status --short` clean at end, baseline re-run green):

1. **Spec-links repair reverted** (`spec_document` appends disabled) → `index_repo_specs_indexes_linked_ids_from_ledger` and `full_reindex_rebuilds_all_entity_types_from_real_storage` both **FAIL**. The round-5 durable finding (ledger `linked_tasks`/`linked_mrs` never indexed, so `?q=<linked id>` returned no spec) is genuinely repaired in both `index_repo_specs` and `reindex_all`, and the tests are anchored to it.
2. **Persona type skipped in `reindex_all`** → count assertion **FAILS** (12 ≠ 11). The test fails if any type is skipped, as the acceptance criterion requires.
3. **Index clear removed from `reindex_all`** → `stale document survived the reindex` **FAILS**. The coordinator truly rebuilds rather than counting pre-existing docs.

Verified working (no findings):

- **All 10 entity types** have document builders (`crates/gyre-server/src/search_indexer.rs`) cross-checked row-by-row against the spec table: spec (title/path/content/owner/links; facets approval_status, scope, workspace, repo), task (title/description/labels; status, priority, assigned_to, workspace, repo), mr (title/branches/author; status, has_conflicts), commit (message/SHA/author/branch; agent_id, date), agent (name/persona/status), persona (name/prompt; scope global/workspace/repo — tenant-scoped mapped to "global"), activity_event (description/event_type/agent_id; workspace, repo, date), audit_event (event_type/details/path; agent_id, workspace, repo, date), review (body/decision/reviewer; mr, workspace, repo), file (path/content; language, workspace, repo). Date facets are ISO `YYYY-MM-DD` via a correct `civil_from_days` implementation (unit-tested at epoch and 2027).
- **Write-path coverage is structural, not per-callsite discipline**: entity repositories on `AppState` are wrapped in `search_sync` decorators at construction (`lib.rs` `build_state`, via the `store!` macro so DB-backed SQLite/PG adapters are wrapped, and mirrored in `test_state_inner`). Every write from any handler, job, or background task re-indexes; delete paths de-index; `update_status` re-fetches to keep facets fresh. Some REST handlers also index explicitly — redundant but idempotent under the scope-keyed upsert.
- **Spec push indexing** (`git_http.rs`): runs after the ledger sync; content extracted from the pushed git blob (`git show <sha>:specs/<path>`), ledger metadata (title/owner/approval_status) and linked ids folded in. Ledger `current_sha` is a blob SHA (spec_registry.rs), so `reindex_all`'s `read_blob(path, current_sha)` reads the right object.
- **Commit push indexing**: message/author/timestamp read from the git object store (`git show -s --format=%an%x1f%at%x1f%B`); branch facet normalized from the refname identically on both the push path and reindex.
- **Code search is eventually consistent per spec**: `search_code_index` job registered in the existing `JobRegistry` (hourly) plus `spawn_job` in `main.rs`, and triggered via `tokio::spawn` only on default-branch pushes (never inline in the push path). Walks `git ls-tree -r -l`, skips binaries (NUL sniff, first 8000 bytes) and blobs over 1 MiB, indexes real blob content with a language facet (test asserts binary and oversized files are skipped).
- **Real full reindex**: `reindex_handler` (`POST /api/v1/admin/search/reindex`, Admin-only, route registered in api/mod.rs:500) calls the server-layer coordinator, which clears the index via the port `reindex_all` contract then rebuilds from spec ledger + git blobs, tasks, MRs + reviews, agents, personas, analytics, audit (both with a 100k event cap), agent-commit mappings, and repo file trees, returning the true count.
- **Adapter changes are sound**: SQLite FTS5 upsert delete-then-insert scoped by (entity_type, entity_id, workspace_id, repo_id) so same-path docs in different repos coexist; mem adapter mirrors the contract (and its `reindex_all` now clears instead of counting); the Postgres 4-column conflict target is backed by an idempotent init-path PK reconciliation (`upgrade_search_index_pk`) with pure, unit-tested decision logic — the SQLSTATE 42P10 trap for task-201-initialized deployments is genuinely closed.
- **Hexagonal boundaries preserved**: FTS SQL stays in `gyre-adapters`; the coordinator only reads repositories and calls the port; `check-arch.sh` passes. No scripts, exemption files, or unrelated tasks touched (diff stat is code + task file only); worktree clean at review end.

Scope notes (not findings): faceted query syntax (`status:approved`) is task-154, access scoping is task-203 — both outside this task's coverage section; `index_repo_specs` counts a spec even when blob extraction fails (best-effort, empty body still searchable by title/path).

Findings:

- (none)
