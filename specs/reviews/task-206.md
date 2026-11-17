# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 9f446d0eb33ecb24b04c821e47007d8884882b63
**HEAD reviewed:** 109c9594840b43e2368759e6be47ac43cba8077e (branch pipeline/task-206/0ec93701728a4e9aa98be79150fd1036-1)
**Status:** approved
**Evidence:** /tmp/stage/review-evidence/ (summary.md + raw logs)

---

## Verdict

Approved. The hollow structural-impact preview is fully replaced by a real
agent-run path that satisfies every acceptance criterion, the skip-ceremony
invariant is enforced server-side (not just skipped client-side), and the
test suite is hard — real bare git repositories, real spawned processes,
production-router probes with live agent JWTs, failure-injection rollbacks —
not self-confirming stubs. Focused probes all pass; gyre-server test
*execution* is deferred to the independent verifier per the stage build
wrapper (required probes listed in the evidence summary).

## What was verified (source + probes)

- **Spawn is real** (`api/meta_specs.rs` `post_meta_spec_preview`): global
  `POST /api/v1/meta-specs/preview` (Admin/Developer role gate with RBAC
  denial audit), per-target validation before any creation (repo 404, ABAC +
  tenant containment 403, spec_path existence on the default branch 400),
  all-or-nothing provisioning of `Active` agent rows, `preview/{id}/{slug}`
  branches pinned to the default-branch tip (fail-closed `resolve_ref`, no
  `unwrap_or_default`), real worktrees, and short-lived preview JWTs
  (`GYRE_PREVIEW_JWT_TTL_SECS`). 202 body is exactly the spec shape.
- **Draft delivery**: `GYRE_PREVIEW_ID` / `GYRE_META_SPEC_DRAFT_KIND` /
  `GYRE_META_SPEC_DRAFT_CONTENT` / `GYRE_TARGET_SPEC_PATH` injected via the
  shared dispatch (merged before server-owned vars so they cannot shadow
  identity/credentials); `GYRE_TASK_ID` deliberately absent.
  `docker/gyre-agent/agent-runner.mjs` builds the governing prompt from the
  draft, withholds task-mutating MCP tools, never calls
  `gyre_agent_complete`, uploads no provenance.
- **Skip-ceremony enforced on every surface**: MCP dispatch refuses
  `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/
  `conversation_upload` for preview agents; REST guards on
  `POST /agents/{id}/complete` (preview target), `POST /merge-requests`,
  `POST /merge-queue/enqueue`, `POST/PUT /tasks` (preview caller, via
  `caller_is_preview_agent` — agent-scope JWT + preview membership). No
  task/MR/provenance/`refs/{agents,tasks}/*` writes; budget enforced
  (workspace default with up-front N-slot reservation via the new
  `check_spawn_budget_for`, plus `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`).
- **Completion**: every compute-target exit monitor routes through
  `spawn::on_agent_process_exit` → `finish_preview_agent` — token revoked,
  worktree removed, budget slot released (idempotent `released` flag), agent
  row `Stopped`, never `Idle`; branch survives for diffing. Launch failure is
  judged by the dispatch's own `launched` flag (not handle shape —
  SshDocker/Kubernetes handles are pid-less by design) and kills through the
  recorded handle/backend pair.
- **Status + diff**: `GET /api/v1/meta-specs/preview/{id}` reports per-agent
  state derived from the real agent rows plus the produced diff
  (preview branch vs pinned base sha, same git plumbing as MR diffs).
- **Cleanup**: `DELETE` kills agents through `state.spawned_backends`
  (recorded backend, never handle-shape reconstruction — the ssh/kubernetes
  kill-routing regressions are pinned with real processes), force-removes
  worktrees (new `force_remove_worktree` port op), closes open MRs on
  preview branches, deletes branches, revokes tokens, drops kv records;
  404 on unknown id. Background `meta_spec_preview_gc` job (registered in
  `jobs.rs`, spawned in `main.rs`) expires runs at/past the TTL
  (`GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h) and reclaims orphaned
  agent index entries.
- **Clean cutover**: workspace-scoped preview routes and
  `compute_preview_blast_radius`/`StructuralImpact` fully removed; ABAC
  exemption file shrank by exactly the two removed routes (frozen baseline
  honored); new global routes registered in the ABAC resolver. Docs updated
  (api-reference.md §Meta-Spec Preview Mode, server-config.md knobs,
  docker/gyre-agent/README.md env vars). Web UI drives the §5 API
  (per-agent status tabs, diff rendering, Clean Up), covered by 164 passing
  vitest cases.

## Probe results

- `cargo check -p gyre-server --tests` — exit 0 (all test code type-checks).
- `cargo test -p gyre-adapters` — 576 passed (incl. new
  `SshTarget::from_handle` round-trip/refusal tests).
- `npm ci` + `npx vitest run MetaSpecs.test.js EditorSplit.test.js` — 164
  passed.
- `cargo fmt --check` (server/adapters/ports) — clean.
- `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-forged-scope-fields.sh`,
  `check-in-memory-state-stores.sh`, and all other invariant check scripts —
  pass.
- Deferred to verifier (stage wrapper blocks gyre-server test codegen):
  `cargo test -p gyre-server --lib -- preview_`, `-- merge_queue`,
  `-- uninstrumented_role_guards`, `-- mcp_preview_agent_denied_ceremony_tools`,
  and the `monitor_`/`kill_process_handle` spawn tests. Four test-quality
  grep scripts (assertionless / comparative-claims / dead-test-code /
  dead-test-variables) exit 2 only because this sandbox ships mawk, which
  cannot parse the scripts' gawk 3-arg `match()` — an environment limitation
  identical at base, not a candidate defect.

## Non-blocking observations

1. **Budget double-release race (minor, bounded).** `release_preview_slot`
   guards on the in-memory/record `released` flag read before the
   read-modify-write in `mutate_preview`; DELETE racing a process exit on the
   same agent can decrement `active_agents` twice. The decrement goes through
   the shared saturating budget port, so the worst case is a slightly
   under-counted workspace occupancy — an accounting drift, not a scope or
   safety escape. Worth a follow-up compare-and-set if budget precision
   matters operationally.
2. **`complete_agent` guards on the path id, not the caller.** The preview
   guard added there fires when the *completed* agent is a preview agent; a
   preview agent completing a *normal* agent via REST is not blocked by this
   task's guard. That cross-agent completion exposure is pre-existing at the
   base (the endpoint had no caller check and still has none for non-preview
   targets) and is out of task-206's scope; the MCP-side guard is
   caller-based and covers the tool surface.
3. **Generated output.** `web/dist/` is unchanged in the candidate diff and
   owned by the deterministic verifier (see evidence summary for an
   incidental in-session rebuild that was fully reverted).
