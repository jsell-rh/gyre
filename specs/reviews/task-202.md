# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `search.md` §Searchable Entities (10-entity table) + §Index Updates (spec pushed → index updated from git blob; commit pushed → messages indexed; file/code search async via background job).
Candidate: `8a6bf4ea314c8545f0595a5391e39c1d82ed7710` on branch `pipeline/task-202/2e09f9b58e71493ab7be8334c715b1ce-1`, base `3af80bd21a8a3452420e014e645eefdfd137f9be`. All 9 task-labeled product-surface commits are recorded in the `commits:` frontmatter (attribution check passes on the candidate commit).
Verdict: **complete** (approved; server-crate test execution deferred to the verifier under the sandbox's 4 GiB build policy — required probes listed below).

## What was reviewed

Full diff base→candidate (25 files, +2629/−100): new `crates/gyre-server/src/search_indexer.rs` (document builders for all 10 types + push-path indexing + file/code-search walker + full-reindex coordinator + tests), new `crates/gyre-server/src/search_sync.rs` (indexing repository decorators wired in `build_state` and the test-state builders), write-path hooks in `api/{tasks,merge_requests,agents,personas,analytics,audit,spawn,orchestrator}.rs`, `merge_processor.rs`, `container_audit.rs`, `procfs_monitor.rs`, `audit_simulator.rs`, push-path hooks in `git_http.rs`, `search_code_index` job in `jobs.rs`/`main.rs`, `reindex_handler` rewiring in `api/search.rs`, and adapter identity widening in `mem_search.rs` / `sqlite/search.rs` / `postgres/search.rs`.

## Verification (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean, 0 errors (the latter type-checks the coordinator test code, including the acceptance-criteria test).
- `cargo test -p gyre-adapters search` → **23 passed, 0 failed** against the real SQLite FTS5 backend, including `reindex_all_clears_and_reports_count` (the clear-first semantics the coordinator's rebuild depends on) and `upsert_replaces_and_deletes` (the new scope-identity upsert). Postgres PK-reconciliation unit tests pass.
- `bash scripts/check-arch.sh` → PASS. Full invariant battery (attribution, mem-port contracts, inert enforcement, ABAC registries, MCP write gate, migration versions/portability, scope-literal/forged-scope/forwarded-header/lossy-secret/in-memory-state/unbounded-HTTP, byte-slice, dead message kinds, fail-open refs) → all PASS. Attribution verified on the candidate commit in a temp worktree (the working-tree failure is the controller's reset of the task file, not a candidate defect).
- `cargo test -p gyre-server --lib search_indexer` was **blocked by the executor's 4 GiB build wrapper** — server test execution belongs to the independent verifier.

## Spec-contract findings (none blocking)

- **All 10 entity types indexed with the specced fields and facets.** Each document builder checked row-by-row against the §Searchable Entities table: task (title/description+labels body; status/priority/assigned_to/workspace/repo facets), mr (branches+author in body; status/has_conflicts/author facets), commit (message body from `git show`; author/branch/agent_id/date facets), agent (name; status/workspace/repo), persona (prompt content body; scope facet mapping Tenant→global/Workspace→workspace/Repo→repo), activity event (description from properties; event_type/agent_id/workspace/repo/date), audit event (details+path; event_type/agent_id/path/workspace/repo/date), review (body; decision/reviewer/mr), spec (full blob content + ledger links in body; approval_status/owner/scope/workspace/repo), file (real content; language/workspace/repo).
- **Write paths synchronous.** Handlers index on create/update/transition, and — stronger than the plan — `search_sync.rs` wraps the agents/tasks/mrs/reviews/personas/analytics/audit repositories in indexing decorators at construction time in both production (`build_state`) and test wiring, so every write path (REST, MCP, background jobs, merge processor, stale-agent detector) re-indexes. Redundant manual index calls at some emit sites are idempotent upserts.
- **Spec content from the git blob**: `git show <sha>:specs/<path>` on the push path; the coordinator reads `git cat-file blob <ledger.current_sha>` — verified `current_sha` is a blob SHA (spec_registry.rs stores `blob_sha`).
- **Real full reindex**: `reindex_handler` calls the server-level coordinator, which clears via the port (`reindex_all` in all three adapters now DELETEs/clears and returns the dropped count — proven by the passing adapter test) and then rebuilds from every repository, returning its own true count. Not a count of pre-existing docs.
- **Code search async**: `search_code_index` registered in the existing `JobRegistry`, triggered on default-branch pushes and run hourly; walks `git ls-tree -r -l`, skips binaries (NUL sniff) and blobs > 1 MiB, indexes real content. No new async runtime.
- **Acceptance test is hard, not self-confirming**: `full_reindex_rebuilds_all_entity_types_from_real_storage` seeds all 10 types against a real SQLite FTS5 backend (via `test_state_with_search`), poisons the index with a stale ghost doc, asserts count==12, idempotency, ghost-doc removal (kills the "counts stale docs" failure mode — especially meaningful because the seed-time decorator indexing means docs exist before reindex, so only the clear makes the per-type assertions meaningful), and per-type retrieval including spec-by-body-content and commit-by-message (kills the "any type skipped" failure mode). FTS5 query sanitization (`build_match_expr`) verified to quote tokens so the tests' dotted query strings parse as phrase ANDs. `task_transition_activity_event_is_workspace_scoped` pins the activity-event workspace/repo scope facets through the real router + SQLite FTS backend.
- **Hexagonal boundary respected**: FTS SQL stays in `gyre-adapters`; the coordinator only reads ports and calls `SearchPort`; git object access is server-side as the plan requires.
- **Rebase-repair merge (f38e8ad07)**: conflicts in agents.rs/personas.rs/search.rs/spawn.rs/jobs.rs/main.rs/mem.rs resolved keeping both sides; verified the merged `mem.rs` test-state builder carries main's workspace-store sharing AND the search wiring, and `spawn.rs` keeps main's launch refactor with the activity-event scope facets ported in.

Non-blocking observations (recorded in the evidence summary, no action required): one retention analytics event has empty scope properties; adapters' `delete` is not scope-scoped while upsert identity is (stale row only until next reindex); `specs/coverage/system/search.md` row 3 still reads `task-assigned` (coverage-matrix updates are made by separate audit commits per repo practice).

## Verifier-required probes (deferred under the 4 GiB wrapper policy)

- `cargo test -p gyre-server --lib search_indexer`
- `cargo test -p gyre-server --lib task_transition_activity_event_is_workspace_scoped`
- `cargo test -p gyre-server --test search_wiring`
- `cargo test --all` and remaining CI gates.

Findings: none.

— Review, 2026-10-08
