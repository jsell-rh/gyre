# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities, §Index Updates.
Comparison base: `3b734b1af744f594ec5d903be242b41ec0f287f2` → candidate `0a1970a343bdfed2f4e8acabc3109ef13d860473`.
Diff: 26 files, +2733/−163 — new `search_indexer.rs` (1449 lines) and `search_sync.rs` (494 lines) in gyre-server; adapter upsert/reindex semantics fixes; wiring in `lib.rs`/`mem.rs`/`main.rs`/`jobs.rs`/`git_http.rs` and every entity write path.

Verdict: **needs-revision** (2 findings).

## What was verified (independently, this sandbox)

- **All 10 entity types are genuinely wired.** Spec content is extracted from the pushed git blob on the post-receive path and from the ledger-pinned blob on reindex; commit message/author come from `git show` in `record_pushed_commits`; file/code search runs as the `search_code_index` JobRegistry job (registered in `jobs.rs`, triggered on default-branch push, hourly loop in `main.rs`) walking `git ls-tree -r -l` with a NUL binary sniff and a 1 MiB cap; task/mr/agent/persona/review/activity/audit are indexed on every repository write via `search_sync` decorators wrapping the `store!`-selected adapters (so DB-backed deployments get SQLite/Postgres inner repositories) plus explicit handler/emit-path calls (analytics/audit emit sites, merge processor, container audit `AuditCtx`, procfs monitor).
- **The reindex is a real rebuild.** `reindex_handler` now calls the server-level coordinator, which clears the index through the port (the mem adapter's count-only no-op was fixed to clear; SQLite/Postgres already cleared) and rebuilds every type from the domain repositories and git object store. Route `/api/v1/admin/search/reindex` is registered and ABAC-mapped (admin/write).
- **The acceptance test is a hard test.** `full_reindex_rebuilds_all_entity_types_from_real_storage` seeds all 10 types against a real SQLite FTS5 backend and real temp git repos, poisons the index with a stale doc, asserts the true count (12), idempotency, stale-doc removal, and per-type retrieval including spec-by-body-content and commit-by-message. It fails if any type is skipped or stale docs are counted.
- **Adapters.** The scope-aware upsert identity (type, id, workspace, repo) is applied consistently in mem and SQLite; the Postgres PK was widened to 4 columns with an init-path reconciliation for task-201-era deployments (would otherwise fail every upsert with SQLSTATE 42P10, silently, since indexing is best-effort) — with unit tests for the reconciliation logic.
- **Probes run:** `cargo check -p gyre-server` (exit 0), `cargo test -p gyre-adapters --lib search` (30/30 pass), `scripts/check-arch.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-in-memory-state-stores.sh`, `check-lossy-secret-conversion.sh`, `check-byte-slice-truncation.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-relative-path-defaults.sh` — all pass. All 10 commits listed in the task frontmatter are ancestors of the candidate. Evidence: `/tmp/stage/review-evidence/`.
- **Deferred to verifier (4 GiB build rule):** `cargo test -p gyre-server --lib search_indexer search_scope search_sync`, `--lib api::tasks`, `--test search_wiring`, `--test git_integration`, then full `cargo test --all`.

## Findings (blocking)

1. **Agent documents never index `persona`.** search.md's Agent row lists Indexed Fields "name, persona, status", but `agent_document` (search_indexer.rs:219-247) puts only name/status in the body and {status, workspace, repo} in the facets — `Agent.persona_id` appears nowhere in the document, on any index path (decorators, handlers, reindex). A query matching an agent's persona can never return that agent.
2. **Several specced Indexed Fields are filter-only facets, not searchable text** (same root cause — the task plan merged the spec's two columns): Commit author/branch, Review decision/reviewer, Spec owner, Activity/Audit `agent_id` are absent from document bodies. Since GET /api/v1/search exposes no facet-value filters either, these values are currently neither searchable nor client-filterable. The MR builder shows the intended pattern (author is in the body). Fix by folding them into the body, or amend the spec via the spec lifecycle if facet-only treatment was intended.

## Non-blocking observations

- Retention deletions (`analytics`/`audit.delete_older_than`) do not de-index removed events; stale event docs persist until the next full reindex (documented backstop).
- Double indexing on REST write paths (decorator + explicit handler call) is redundant but idempotent under the scope-keyed upsert.
- `index_repo_specs` resolves ledger entries by path only (no repo key) — pre-existing ledger semantics; same-path specs in multiple repos can share facets.

Generated `web/dist/` observations: none — the diff touches no generated assets; the verifier-owned rebuild is unchanged by this candidate.
