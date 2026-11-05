# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities (10-type table) + §Index Updates
("The search index is updated synchronously on every write: Task created/updated; Spec
pushed (content extracted from git blob); MR created/reviewed; Commit pushed → commit
messages indexed; Agent registered/status changed"; code search async via background job).
Candidate under review: `2c757877701b328fd11bc7e4418d9dc4ca36b3f2` (base
`d81b0fe4ab93abff06089432e4a8fca5be6e5d31`). Production commits: `309fc7dd`, `80f13a70`
(+ `e2583917` bookkeeping).
Verdict: **needs-revision** (core deliverable is real and well-tested; the agent-entity
write paths are not covered and the Shipped notes overstate them).

## Probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-server --lib search_indexer` → **6 passed** (`task-202-server-search-indexer.txt`),
  including `full_reindex_rebuilds_all_entity_types_from_real_storage`, which seeds all
  10 entity types plus a poisoned stale doc against a real SQLite FTS5 backend and
  asserts true count (12), idempotency, stale-doc removal, and per-type retrieval
  (spec-by-body-content, commit-by-message, review-by-body, persona-by-prompt, etc.).
- `cargo test -p gyre-adapters --lib search` → **13 passed** (`task-202-adapters-search.txt`),
  incl. FTS5 schema, porter stemming, scope-scoped upsert/delete, workspace filter at
  index level, `reindex_all_clears_and_reports_count`.
- `bash scripts/check-arch.sh` → OK; `bash scripts/check-inert-enforcement.sh` → OK
  (orchestrator.rs edits kept line-count-neutral around the pinned `validate_persona`
  sites at :187/:283, exactly as the exemption baseline requires).
- **Mutation probes** (source restored; `git status --short` clean, HEAD `2c757877`
  re-verified green afterwards):
  - Mutation 1 — coordinator skips the persona repository loop →
    `full_reindex...` **FAILS** (`assert_eq!(count, 12)` → left: 11)
    (`task-202-mutation1-skip-persona.txt`).
  - Mutation 2 — remove the clear-before-rebuild (`state.search.reindex_all()`) →
    `full_reindex...` **FAILS** ("stale document survived the reindex")
    (`task-202-mutation2-no-clear.txt`).
  Both acceptance-criterion test properties ("MUST fail if any type is skipped or if
  reindex counts stale docs instead of rebuilding") are genuinely anchored.

## Verified working (no findings)

- **All 10 document builders** (`search_indexer.rs`) carry the specced fields/facets:
  task (title/description/labels → status/priority/assigned_to), mr
  (source/target/author → status/has_conflicts), commit (message/SHA/author/branch →
  agent_id/date ISO), agent (name/status), persona (prompt content → scope
  global/workspace/repo), activity event (description/event_type/agent_id → date),
  audit event (event_type/details/path → agent_id/date), review (body/decision/reviewer
  → mr), spec (path/full content/owner → approval_status/scope), file
  (path/content → language). `workspace_id`/`repo_id` set from real scope ids, not
  literals.
- **Spec push** (git_http.rs, after ledger sync, default-branch ref): content read via
  `git show <sha>:specs/<path>` from the pushed blob — proven by
  `index_repo_specs_extracts_blob_content_from_pushed_commit`.
- **Commit push** (record_pushed_commits): author/timestamp/message read from the git
  object store (`git show -s --format=%an%x1f%at%x1f%B`) — proven by
  `index_pushed_commit_reads_message_and_author`.
- **Real full reindex**: `reindex_handler` (POST /api/v1/admin/search/reindex, route +
  ABAC mapping pre-existing) calls the server-layer coordinator, which clears via the
  port contract then rebuilds from spec ledger + git blobs, tasks, MRs + reviews,
  agents, personas, analytics, audit, agent-commit mappings, and repo file trees.
  Adapter `reindex_all` now actually clears (sqlite/mem/postgres) instead of counting.
- **Code search**: `search_code_index` job registered in the existing `JobRegistry`,
  spawned hourly in main.rs and triggered (tokio::spawn, non-blocking) on
  default-branch pushes; walks `git ls-tree -r -l`, NUL-sniffs binaries, skips >1 MiB
  blobs, indexes real content — proven by `file_indexing_skips_binary_and_oversized_blobs`.
- **Scope-scoped upsert identity** (sqlite/pg/mem): same spec/file path in different
  repos coexists instead of clobbering; port-level `delete` remains type+id-wide so
  persona de-index still works.
- **Write paths wired**: task create/update/transition, MR create/transition,
  review submission, agent REST create/status, persona create/update/approve/delete
  (delete de-indexes), activity events at every analytics emit path (analytics.rs,
  tasks.rs, merge_requests.rs, spawn.rs, orchestrator.rs), audit events at every audit
  write (audit.rs, container_audit.rs via AuditCtx, procfs_monitor.rs, audit_simulator.rs).
- Task bookkeeping is honest: `specs/tasks/task-202.md` records the three commits and
  keeps notes under `## Shipped`; no exemption files or scripts touched; hexagonal
  boundaries hold (coordinator in gyre-server only calls the port; FTS SQL stays in
  gyre-adapters).

## Findings

1. **Agent registration paths never index** (`crates/gyre-server/src/api/spawn.rs:420`,
   same shape in `api/orchestrator.rs:108` and `api/compose.rs:116`). Spec §Index
   Updates: "Agent registered/status changed -> index updated"; acceptance criterion 1:
   "Each write path calls `SearchPort::index` synchronously". `spawn_agent_core`
   creates the agent (`state.agents.create(&agent)`) with no `search_indexer::index`
   call — `grep -rn agent_document crates/gyre-server/src | grep -v search_indexer`
   yields only `api/agents.rs:126` and `:268`. The Shipped notes explicitly claim
   "agent create and spawn (api/agents.rs, api/spawn.rs)" — that is false for
   spawn.rs. Reproduce: spawn an agent via POST /api/v1/spawn, then query
   `GET /api/v1/search?q=<name>` with `entity_type=agent` — not found until an admin
   reindex or a REST status PATCH. The spawn path is the primary agent-creation path
   in production, so most agents are invisible to search.
2. **Agent status lifecycle changes bypass re-indexing** (`crates/gyre-server/src/api/spawn.rs:1284`
   complete→Idle; `stale_agents.rs:38/116`; `api/admin.rs:285` kill;
   `mcp.rs:1344/1425`). Only the REST `PATCH /agents/{id}/status` handler re-indexes
   (`api/agents.rs:266-268`); the internal lifecycle transitions update the entity
   without touching the index, so the `status` facet freezes at its last REST value —
   directly against "Agent registered/status changed -> index updated".
3. **Task updates via the MCP tool and spec-lifecycle automation bypass re-indexing**
   (`crates/gyre-server/src/mcp.rs:1054` — the `gyre_update_task` tool agents actually
   use; `api/specs.rs:917` — spec-lifecycle task updates). Spec: "Task created/updated
   -> index updated". REST update/transition re-index, but an agent-side status/priority
   change through MCP leaves the search facets stale until the next admin reindex.

## Notes (not blocking)

- The spec's Agent indexed fields include "persona", but the `Agent` domain type has no
  persona attribute, so there is no source field to index; resolving that needs a spec
  amendment or domain change, not this task. `agent_document` indexes name + status,
  which is everything the domain carries.
- MR merge/revert via `merge_processor.rs` (4 `merge_requests.update` sites) also skips
  re-indexing; the spec's literal Index Updates list for MRs says "created/reviewed"
  (both covered), so this is weaker than findings 1–3 but the same one-line pattern
  would fix it.
- Full-workspace `cargo test --all` deferred to verification per review scope; focused
  suites, both mutation probes, and the mechanical checks all pass at the candidate.

## Required revision

Wire `search_indexer::index(&state, agent_document(&agent))` into the agent
spawn/orchestrator/compose creation sites and the status-lifecycle update sites
(complete, stale detector, admin kill, MCP), and the task-update sites in
`mcp.rs`/`api/specs.rs` (task documents). Correct the Shipped notes to match. The
coordinator, builders, adapters, and tests need no rework — the mutation-probed test
suite already covers them.
