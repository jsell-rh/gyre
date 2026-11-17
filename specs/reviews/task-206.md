# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Review agent
**Round:** R1 (candidate 31d42f9be535d1c34b9f3bdf12a1adfbd631f5be)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 64d0b86582b0d09c2a689ffff75c0585b6ee9f56
**Status:** needs-revision — five preview-agent ceremony-escape routes remain open

---

## What is real (verified)

The core preview loop is a genuine implementation, not the hollow estimate the
task replaced:

- **Spawn** — `POST /api/v1/meta-specs/preview` (Admin/Developer, global route
  per spec) validates every target up front (repo, ABAC, tenant containment,
  spec_path on the default branch), then provisions per target a real `Active`
  agent row, a `preview/{preview_id}/{slug}` branch pinned to the default-branch
  tip, a real git worktree, and a short-lived JWT. 202 shape matches the spec
  exactly. All-or-nothing provisioning with rollback on provision/persist
  failure; budget enforced per workspace plus an optional
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap (429).
- **Draft delivery** — `GYRE_META_SPEC_DRAFT_KIND/CONTENT`,
  `GYRE_TARGET_SPEC_PATH`, `GYRE_PREVIEW_ID` injected on every compute target
  (local/container/docker/ssh/ssh-docker/k8s verified); agent-runner.mjs embeds
  the draft verbatim in the LLM prompt as governing "how to build" context over
  the target spec and withholds task-mutating tools.
- **Completion** — every compute-target exit funnels through the single
  monitor → `on_agent_process_exit` → `finish_preview_agent`: token revoked,
  worktree removed, budget slot released, row `Stopped` — never `Idle`.
- **Status + diff** — per-agent state from the real agent rows; diff of the
  preview branch against the pinned base sha via the same git2 plumbing as MR
  diffs.
- **Cleanup** — DELETE kills processes through the recorded backend+handle
  pair, removes worktrees/branches/records (404 unknown id); the
  `meta_spec_preview_gc` job (interval + TTL configurable, defaults 1h/24h)
  expires old runs and reclaims orphaned agent entries.
- **Tests** — unusually strong: real git repos in tempdirs, real spawned
  processes (env-dump script proves the draft reached the process), the real
  monitor-driven completion path, error-text-pinned authz denials with positive
  controls, failure injection at port boundaries. No self-confirming stubs
  found on the spawn/status/DELETE/GC paths.
- `cargo check -p gyre-server` passes; check-arch, ABAC route-registry,
  ABAC exempt-handlers, MCP write-tools, inert-enforcement scripts all pass
  (logs in /tmp/stage/review-evidence). The ABAC exemption file shrank by the
  two removed workspace-scoped preview routes (no growth).

## Findings (blocking) — the preview-agent ceremony escape class is incompletely closed

The candidate's own repair history (commits 7cad1261, 2f10e8d1 — "close the
preview-agent escape class") and shipped summary establish server-side refusal
of ceremony mutations by preview-agent callers as the contract. Five routes of
that class remain open. All are reachable by a preview agent's JWT because the
builtin `agent-scoped-access` policy grants the Agent role `write` on `*`, and
a preview agent is same-tenant/same-repo-ABAC-valid for its own target by
construction:

1. **`POST /api/v1/repos/:id/orchestrator/spawn`** (orchestrator.rs:1117) — no
   role gate, no preview refusal. A preview agent spawns a repo orchestrator
   and receives an orchestrator JWT (task decomposition + worker spawning).
   The workspace-tier twin got exactly this guard in this diff; the repo tier
   was missed.
2. **`POST /api/v1/agents`** (agents.rs:127 `create_agent`) — no auth
   extractor; mints a new agent row plus an opaque `agent_tokens` credential
   whose identity is not in the `preview_agents` index, so
   `caller_is_preview_agent` returns false for it. One call launders a fresh
   identity past every refusal guard this task added.
3. **`POST /api/v1/agents/:id/complete`** (spawn.rs:2909) — the preview guard
   (line 2926) is keyed on the path *target*, not the caller (the only
   target-keyed guard in the diff), and the handler has no auth extractor. A
   preview agent completing another agent mints a real MR, moves a task to
   Review, writes refs snapshots. The shipped claim "the REST surface refuses
   preview-agent callers on agent complete" is false as implemented.
4. **`POST /api/v1/repos/:id/specs/save`** (specs_assist.rs:897) — auto-creates
   an MR plus an approval notification; no preview refusal. MR-creation
   escape.
5. **`POST /api/v1/repos/:id/commits/record`** (agent_tracking.rs:90) —
   provenance write with caller-supplied `agent_id`, no auth extractor, no
   preview refusal, although the MCP side refused `conversation_upload` for
   exactly this reason.

Consequence: the task acceptance criterion "Preview agents skip gates, MR,
merge queue, provenance, and task assignment" does not hold against an
adversarial (or merely hallucinating — the runner keeps `Bash` and a valid
`GYRE_AUTH_TOKEN`) preview agent, and specs/coverage row 9's "Skip-ceremony
enforced … REST surface refuse task/MR/provenance mutations … for preview
agents" overstates the shipped enforcement.

## Non-blocking observations

- `monitor_poll_once` treats an `is_alive` **Err** (SSH blip, daemon hiccup) as
  exit; on remote targets a transient probe failure tears down a live preview
  agent's records while the remote container/Pod keeps running, with both
  registry entries already removed so nothing can ever kill it
  (spawn.rs:2376-2380, ssh.rs:533-541).
- `release_preview_slot` (meta_specs.rs:2117) released-flag guard is
  check-then-act on a stale copy; concurrent exit + DELETE can
  double-decrement the budget counter.
- The meta-spec branch of `web/src/lib/EditorSplit.svelte` is a second,
  production-unreachable preview loop (only `context="spec"` is mounted) that
  has already diverged from MetaSpecs.svelte's implementation — wire it up or
  remove it.
- No end-to-end launch-failure test: the POST → launch error →
  `mark_preview_agent_failed` → 500/rollback chain is only tested via direct
  calls with hand-built state (meta_specs.rs:3710).
- Env-knob parsing (`preview_*_from_env`) and the lib.rs env→AppState wiring
  are untested.
- Cosmetic: MetaSpecs.svelte:894 classifies `+++`/`---` diff headers as
  add/remove lines (EditorSplit does it correctly).

## Verifier instructions

Run (deferred from review per the 4 GiB budget rule):
`cargo test -p gyre-server preview_`, the mcp.rs preview-refusal tests, and
`cd web && npm ci && npx vitest run src/__tests__/MetaSpecs.test.js
src/__tests__/EditorSplit.test.js`. Evidence and command logs:
/tmp/stage/review-evidence/.
