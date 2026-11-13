# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities (all 10 entity types with
specced Indexed Fields and Facets) + §Index Updates (spec pushed → index
updated with content extracted from git blob; commit pushed → messages
indexed; file content asynchronously via background job).
Candidate under review: `8b7c00c9` (merge of the task-202 implementation with
main's task-154 query language, base `890a70de`). Scoping per `commits:`
frontmatter: `309fc7dd`, `80f13a70`, `3e223c8a`, `e2583917`, `3dc6cdcc`,
`2faf26af`, `b522dd5c`, `b316bc37`, `1f80f64f`, `3e5f747b`, `5acae62c`,
plus the merge/rebase-resolution commits (`e9f6b2f4` et al.) reviewed as the
diff against the assigned base.
Verdict: **complete** — no blocking findings.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-adapters --lib search` → **43 passed, 0 failed**
  (SQLite FTS5 roundtrip, query language, access scoping, scope-aware
  upsert, `reindex_all_clears_and_reports_count`).
- `cargo test -p gyre-adapters --lib mem_search` → **11 passed, 0 failed**.
- `cargo check -p gyre-server` → clean; `cargo check -p gyre-server --tests`
  → clean (type-checks the server test code including the acceptance test
  and `tests/search_wiring.rs`).
- `cargo clippy -p gyre-adapters --lib` → warnings only in pre-existing
  untouched files (`postgres/analytics.rs`, `postgres/audit.rs`,
  gyre-domain/gyre-ports baseline); zero in the touched
  `sqlite/search.rs` / `postgres/search.rs` / `mem_search.rs`.
- `cargo fmt --check` → clean.
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-inert-enforcement.sh`, `check-unbounded-external-http.sh`,
  `check-task-commit-attribution.sh` → all OK; `git diff base..candidate --
  scripts/` is empty (no gate weakened, no exemption file touched;
  `task-c....txt` still frozen at 2).
- gyre-server test **codegen is blocked by the executor's build wrapper**
  (4 GiB budget) — the following must be run by the independent verifier:
  - `cargo test -p gyre-server --lib search_indexer` (includes
    `full_reindex_rebuilds_all_entity_types_from_real_storage`, the
    acceptance test)
  - `cargo test -p gyre-server --test search_wiring`
  - full `cargo test --all`

## Verified working (no findings)

- **All 10 entity types indexed with the specced fields and facets.**
  `crates/gyre-server/src/search_indexer.rs` holds one document builder per
  type, checked row-by-row against the spec table: task (title/description/
  labels; status/priority/assigned_to/workspace/repo), mr (title/branches/
  author; status/has_conflicts/author/workspace/repo), commit (message/SHA/
  author/branch; author/branch/agent_id/date/workspace/repo), agent
  (name/persona/status; status/persona/workspace/repo), persona
  (name/prompt content; scope global/workspace/repo — tenant-wide mapped to
  "global"), activity event (description/event_type/agent_id; event_type/
  agent_id/date/workspace/repo), audit event (event_type/details/path;
  event_type/agent_id/path/date/workspace/repo), review (body/decision/
  reviewer; decision/reviewer/mr/workspace/repo), spec (title/path/content/
  owner/links; approval_status/scope/owner/workspace/repo), file (path/
  content; language/workspace/repo). Indexed Fields that are text are folded
  into the searchable body (commit author/branch, review decision/reviewer,
  spec owner and linked task/MR ids, agent persona id, event agent_id, task
  labels) — facet-only values would be unmatchable since `GET /api/v1/search`
  exposes no facet-filter params beyond the query language, which the
  round-5acae62c repair addressed.
- **Spec content really comes from the git blob.** `index_repo_specs` runs
  on default-branch push in `git_http.rs` after `sync_spec_ledger` (so
  approval_status/owner/links facets reflect the push), reading content via
  `git show <sha>:specs/<path>`; the reindex coordinator reads the blob
  pinned in the ledger (`entry.current_sha`). Title/owner/approval_status/
  links are ledger-sourced; unregistered specs index as `pending`.
- **Commit indexing reads real metadata.** `record_pushed_commits` calls
  `index_pushed_commit` per pushed SHA; author/timestamp/message come from
  `git show -s --format=%an%x1f%at%x1f%B`; branch facet is the normalized
  branch name on both the push path and the reindex path (identical
  documents).
- **Every write path indexes.** Beyond the explicit handler callsites
  (tasks, agents, MRs, reviews, personas create/update/approve/delete with
  de-index, analytics, audit), all seven entity repositories are wrapped at
  construction in `search_sync` decorators (`lib.rs build_state` via the
  `store!` macro, so mem **and** SQLite/Postgres modes alike) — every write
  from any handler, MCP tool, or background task re-indexes; delete paths
  de-index. This is stronger than per-callsite discipline and cannot miss a
  new write path.
- **File/code search is a real background job.** `search_code_index` is
  registered in the existing `JobRegistry` (hourly + triggered on
  default-branch push via `registry.trigger` inside `tokio::spawn`, plus
  `jobs::spawn_job` at startup), walks `git ls-tree -r -l` of the default
  branch, skips binaries (NUL sniff) and >1 MiB blobs, and indexes real
  blob content with a language facet. No new async runtime invented.
- **The full reindex is real.** `reindex_handler`
  (`POST /api/v1/admin/search/reindex`, ABAC-registered) invokes
  `search_indexer::reindex_all`, which first clears the index through the
  port (the SQLite and mem adapters' `reindex_all` now really clear — the
  old count-only no-op is gone, pinned by
  `reindex_all_clears_and_reports_count`), then rebuilds from every entity
  repository and returns the true document count. The coordinator lives in
  the server layer, calling only ports — hexagonal boundary preserved
  (`check-arch.sh` OK).
- **The task-154 merge resolution is correct, not a gutting.** All document
  builders carry the entity's real timestamp in `SearchDocument.updated_at`
  so `since:`/`until:` filter on entity time; the PG upsert combines the
  `updated_at` column ($9) with the 4-column scope-aware ON CONFLICT target;
  the SQL composers (`search_sql`/`suggest_sql`/`facet_sql`) keep the `s`
  alias and contiguity invariants the port methods execute, with tests on
  both sides retained. The PG `search_index` PK reconciliation
  (`upgrade_search_index_pk`) repairs task-201-era 2-column PKs that would
  make every 4-column ON CONFLICT fail silently (best-effort indexing →
  empty index); the old key is a subset of the new one so the ADD always
  applies; hostile constraint names are quoted; pure unit tests cover all
  four shapes.
- **Access scoping stays fail-closed.** Tenant resolution is centralized in
  the `with_tenant_resolution` port wrapper (never per-callsite); an
  unresolvable workspace leaves the tenant unset → `""`, never a fabricated
  "default" scope; the SQLite adapter derives tenant from the real
  workspace row and fail-closes to `""` on NotFound. The scope-aware upsert
  (entity + workspace + repo identity) correctly lets same-path specs/files
  in different repos coexist; `delete` removes all scope variants.
- **The acceptance test is hard, not self-confirming.**
  `full_reindex_rebuilds_all_entity_types_from_real_storage` seeds all 10
  types (spec by distinctive body content from a real temp git repo, commit
  by message with a real `git commit`, task/mr/review/agent/persona/
  activity/audit through the real repositories) against a **real SQLite
  FTS5 backend** (`test_state_with_search` wires `SqliteStorage` as the
  search port with production-shaped decorator wrapping), poisons the index
  with a ghost doc matching no entity, asserts the exact true count (12),
  idempotency on a second run, ghost-doc disappearance (kills any
  count-stale-docs regression), per-type retrieval, facet values, and
  indexed-field-only matches (author/branch/decision/reviewer/owner/
  agent_id/persona deliberately absent from the seeded bodies). Skipping
  any type fails its assertion. `tests/search_wiring.rs` additionally
  proves the HTTP surface end-to-end (FTS5 markers, bm25 scores, scoping).
- **No fakery indicators.** No `bail!` stubs, no hardcoded values standing
  in for lookups, no audit-only paths, no test-only branches in production
  code, no weakened gates or new exemption entries. Commit attribution
  verified: every task-labeled product-surface commit is in the frontmatter
  (merge commits and docs-only chore commits are out of the check's scope
  by design; `babb77f7` is a merge commit despite its subject).

## Non-blocking observations (recorded, not defects)

- Handlers that both call the decorated repository and then
  `search_indexer::index` inline index the same document twice — the
  scope-aware upsert makes this idempotent; purely cosmetic redundancy.
- `reindex_all` caps activity/audit events at 100 000 per type
  (`REINDEX_EVENT_LIMIT`); the spec sets no bound and the push-path
  indexing is uncapped, so eventual consistency is preserved.
- Deferred verifier probes listed above must pass before publication; the
  acceptance test is type-checked here but its execution belongs to the
  verifier's environment.

— Reviewer, task-202 round at candidate `8b7c00c9`
