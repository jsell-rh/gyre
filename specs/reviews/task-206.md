# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R1 (candidate 944bed72)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 047d652495dde3b1ae6ddbea4f8cf1b077a3c18f
**HEAD reviewed:** 944bed7233f930b4d45f199dd78c538eba40c100
**Status:** findings — not approved this round

---

## Verification performed

All probes on the exact candidate head; evidence retained under
`/tmp/stage/review-evidence/task-206-{web,cargo-check,kill-path,summary}.md`.

- `cargo check -p gyre-server --lib --tests` — clean (lib + full preview test
  suite type-check).
- `cargo test -p gyre-adapters --lib` — 574 passed, 0 failed (6 ignored:
  pre-existing SSH-daemon integration tests); focused `compute::ssh` run
  includes the `SshTarget::from_handle` round-trip/refusal tests.
- `cargo test -p gyre-common -p gyre-domain -p gyre-ports` — 563 passed.
- Web: `npm ci` then `vitest run src/__tests__/MetaSpecs.test.js
  src/__tests__/EditorSplit.test.js` — 161 passed, 0 failed.
- Mechanical gates all OK on this head (arch, abac-route-registry,
  mcp-write-tools, mem-port-contracts, in-memory-state-stores,
  scope-literal-defaults, fabricated-scope-defaults, forged-scope-fields,
  inert-enforcement, lossy-secret-conversion, byte-slice-truncation,
  forwarded-header-trust, unbounded-external-http, fail-open-ref-resolution,
  relative-path-defaults).
- `cargo test -p gyre-server --lib -- preview` remains **mandatory for the
  verifier** (server test binaries exceed this sandbox's build budget); the
  suite's green baseline is 1232 passed on pre-rebase head 4fc19fca.

## What is genuinely implemented (spot-verified in source)

The §5 loop is real, not an estimate: POST validates every target up front
(repo ABAC + tenant containment + spec_path on the default branch), provisions
real `Active` agent rows, `preview/{id}/{slug}` branches pinned to the
default-branch tip, real worktrees, and short-lived preview JWTs with
scope claims derived from the loaded workspace/repo rows; the draft travels
out-of-band (`GYRE_PREVIEW_ID`/`GYRE_META_SPEC_DRAFT_*`/`GYRE_TARGET_SPEC_PATH`,
`GYRE_TASK_ID` absent) and `agent-runner.mjs` builds the governing prompt from
it with task tools withheld; skip-ceremony is enforced server-side on both the
MCP dispatch and the REST surface (`complete`, `create_mr`, `create/update
task`, `merge-queue enqueue` all 403 for preview-agent callers — and the
orchestrator-tier MCP tools are unreachable because preview JWTs carry
`orchestrator_type: None`); completion routes every compute-target monitor
through `finish_preview_agent` (Stopped-never-Idle, worktree removed, token
revoked, slot released exactly once); status reports real per-agent state plus
the produced diff against the pinned base; DELETE tears down branches,
worktrees, tokens, kv records and closes dangling MRs; the GC job is
registered with a TTL boundary test. Budget: workspace budget by default with
up-front N-slot reservation, separate `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`
cap (429). Docs (api-reference, server-config, agent README) updated. Clean
cutover verified: no references to the removed workspace-scoped routes or
`compute_preview_blast_radius` remain.

## Findings

### F1 (blocking) — DELETE/GC cannot kill preview agents on the default SSH mode or Kubernetes targets

`crates/gyre-server/src/api/meta_specs.rs` `kill_preview_process` (:1902) kills
via `spawn::kill_process_handle` (spawn.rs:2405), which reconstructs the
target from the handle alone. But:

- The **default** SSH dispatch arm (`dispatch_agent_launch`, spawn.rs:1529,
  `container_mode` defaults to `true`) registers an `SshDockerTarget` handle
  whose `id` is the remote **container id** (ssh.rs:504 — deliberately not
  `user@host:pid`). `SshTarget::from_handle` refuses it (no `user@host:`
  structure) → the kill is refused with a warn log and the remote container
  keeps running. The `from_handle` fix only covers the legacy
  `container_mode: false` bare-ssh escape hatch.
- **Kubernetes** handles (`target_type: "kubernetes"`, `pid: None`) fall into
  `kill_process_handle`'s `else` arm → `LocalTarget::kill_process`, which is a
  **silent no-op when `pid` is `None`** (local.rs:42). The pod keeps running.

The correct mechanism already exists and is used by the admin kill path
(`admin.rs kill_agent` :379): kill through the `SpawnBackend` recorded in
`state.spawned_backends` at spawn time. `kill_preview_process` never consults
it. This violates the acceptance criterion "`DELETE …` kills agents" for two
of the five compute-target types the same POST endpoint spawns on, and the
shipped claim "remote kills reach the remote host" is true only for the
legacy bare-ssh mode. No test covers DELETE/GC against an ssh-docker or
kubernetes preview agent (the preview suite is local-target only), so the gap
is invisible to the suite. Reproduce: configure a workspace compute target of
type ssh (default container mode) or kubernetes, POST a preview, DELETE it —
the remote workload is still alive after the 204 (kill warn in the log for
ssh; silent no-op for k8s), while the agent row, branch, worktree, token and
budget slot are already torn down around it.

### F2 (blocking) — `spawned_backends` entry leaks on every preview DELETE/GC of a running agent

`kill_preview_process` (meta_specs.rs:1902) removes only the
`process_registry` entry. The exit monitor (`monitor_spawned_agent_exit`,
spawn.rs:2305) then observes the missing handle and `break`s **without**
removing the agent's `spawned_backends` entry (the `!alive` branch removes
both; the missing-handle branch removes neither). One `Arc<SpawnBackend>`
entry (agent id → backend holding its target config) is leaked per
DELETE/GC-torn-down running preview agent — unbounded growth over a
long-running server that uses the preview loop. The admin kill path removes
both entries; the preview path must too (naturally fixed together with F1 by
killing through the recorded backend).

### F3 (non-blocking observation) — `persist_preview` failure orphans provisioned agents, and the `non-atomic-creation:ok` annotation overstates coverage

`post_meta_spec_preview` (meta_specs.rs:966) propagates a `persist_preview`
error with `?` and no rollback, leaving N `Active` agent rows, live tokens,
held budget slots, branches and worktrees with **no** preview record and no
`preview_agents` index entries — invisible to the GC orphan pass (which
iterates `preview_agents`). The `// non-atomic-creation:ok` annotation on
`provision_preview_agent` (:1512) claims "every failure path after the first
create routes through rollback_preview_agent", which is false for this path
(trigger: transient kv failure on the `meta_spec_previews` write after the
per-agent `agent_tokens` writes succeeded). Low probability, but the same
compensation discipline the annotation claims should either cover it or the
annotation should not claim it.

## Verdict

Not approved this round: F1/F2 (one fix location — kill preview agents through
the recorded `SpawnBackend` and remove both registry entries, matching the
admin kill path). F3 should be addressed or the annotation corrected. The core
§5 implementation is otherwise real and well-tested; once the kill path is
closed and the server test suite is confirmed green in independent
verification, this task should be approvable.
