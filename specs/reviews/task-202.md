# Review — task-202 (Index all searchable entity types and implement real full reindex)

Spec: `specs/system/search.md` §Searchable Entities + §Index Updates.
Candidate: `a82d16bb` (base `4dc0eb9b`), branch
`pipeline/task-202/c3991fbc40ae4f07b85b6a30078aea28-1`. Scoped to the 11
commits in the task's `commits:` frontmatter (incl. rebase-repair merges
`9921ca85`/`4fb23cd9` against main's task-119/task-139 AppState conflicts —
resolution keeps both sides; verified in the lib.rs/mem.rs hunks).
Verdict: **complete** — approved, no blocking findings.

## Probes run here (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` —
  pass (lib + cfg(test) code type-checks).
- `cargo test -p gyre-adapters search` — **43 passed, 0 failed** against the
  real SQLite FTS5 backend (reindex clears-and-reports, scope-aware upsert,
  access scoping inside the index query, facets, phrases, porter stemming;
  new PG pk_upgrade SQL-shape suite).
- `cargo clippy -p gyre-server -p gyre-adapters` (+ `--tests`) — 0 errors.
- `cargo fmt --check` — clean.
- `bash scripts/check-arch.sh` + all 17 mechanical invariant scripts — pass
  at the candidate commit (run in a detached worktree of `a82d16bb`; the
  working tree's task-202.md had been reset to `not-started`/`commits: []`
  by the controller, which also makes check-task-commit-attribution fail
  against the working tree only — it passes at HEAD).
- gyre-server test *execution* is deferred to the independent verifier per
  the stage build budget (wrapper rejects server codegen). Required runs:
  `cargo test -p gyre-server --lib search_indexer`,
  `cargo test -p gyre-server --lib api::tasks`, `cargo test --all`.

## Verified against the acceptance criteria

- **All 10 entity types** have document builders in
  `crates/gyre-server/src/search_indexer.rs` with the specced indexed fields
  and facets: spec (blob content, ledger title/owner/approval_status, linked
  task/MR ids, scope = first path segment), task (labels folded into body),
  mr (source/target/author), commit (message from `git show`, author, branch,
  SHA = entity_id), agent (name/persona/status), persona (prompt content,
  scope facet with Tenant→global mapping), activity_event (description,
  event_type, agent_id), audit_event (event_type/details/path), review
  (body/decision/reviewer joined to the MR for workspace/repo/mr facets),
  file (path/content/language, binary + >1 MiB skipped, real content via
  `git cat-file`).
- **Write paths index synchronously**: REST handlers plus a stronger
  belt-and-braces layer — every entity repository on `AppState`
  (agents/tasks/mrs/reviews/personas/analytics/audit) is wrapped at
  construction (`search_sync.rs`) so REST, MCP tools, background jobs, and
  the merge processor all re-index on write; deletes de-index. Update paths
  (update_task, transitions, persona update/approve, agent status) re-index
  with current facet values, fixing the create-path-only freeze noted in the
  coverage audit. Spec/commit indexing hooks live in `git_http.rs` on the
  push path (spec after ledger sync on default-branch pushes; commits for
  every pushed SHA).
- **File/code search is a background job** via the existing `JobRegistry`
  (`search_code_index`, registered in `jobs.rs`, triggered on default-branch
  push via `job_registry.trigger` inside `tokio::spawn`, hourly loop in
  `main.rs`) — eventually consistent per spec, no new async runtime.
- **Real full reindex**: `search_indexer::reindex_all` clears the index
  (adapter `reindex_all` now really deletes — mem + SQLite verified by
  adapter tests) then rebuilds from every repository (spec ledger + git
  blobs, tasks, MRs+reviews, agents, personas, analytics, audit,
  agent-commit mappings with `git show` messages, repo file trees) and
  returns the true count. `reindex_handler`
  (`POST /api/v1/admin/search/reindex`, ABAC-registered route) invokes the
  coordinator.
- **The mandated test is hard**: `full_reindex_rebuilds_all_entity_types_from_real_storage`
  seeds all 10 types (real temp git repo, real SQLite FTS5 via
  `test_state_with_search`), poisons the index with a ghost doc, asserts
  count == 12 exactly, idempotency on a second run, ghost-doc removal (kills
  any count-stale-docs regression), and per-type retrieval with distinctive
  terms including spec-by-body-content, commit-by-message/author/branch,
  spec-by-owner and by-linked-id, review-by-decision/reviewer,
  agent-by-persona, activity/audit-by-agent_id — the facet-only-value
  regressions from earlier rounds. Write-path scope coverage is pinned by
  `task_transition_activity_event_is_workspace_scoped` (activity events now
  carry workspace/repo on every internal emit site: tasks, spawn, complete,
  merge processor, orchestrator).
- **Hexagonal boundaries preserved**: FTS SQL stays in gyre-adapters; the
  coordinator only calls ports; check-arch passes.

## Findings

- (none blocking)

Non-blocking observations recorded in
/tmp/stage/review-evidence/summary.md: (1) PG's tsvector covers only
title+body, so SHA/path full-text matches work on SQLite (entity_id is an
indexed FTS column) but not on Postgres — pre-existing task-201 schema, PG
runtime verification deferred to the verifier; (2) the pre-existing
unscoped REST analytics ingest now feeds the search scope from
caller-supplied properties (no new authority — the analytics query endpoint
was already unscoped); (3) harmless double-indexing on some paths
(decorator + manual call) is idempotent under the scope-aware upsert.

— Reviewer, task-202 review round (candidate a82d16bb)
