# Review: task-206 — Meta-spec preview mode (real agent preview runs)

**Reviewer:** Review agent (independent)
**Round:** R-current (fresh verdict on candidate da87710d, base 7a80e459)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 7a80e4597658a6b479adea183db2f3c8a2ef7614
**Candidate reviewed:** da87710d86323695c1abd5af6159e36cb7e840d2
**Status:** findings — not approved this round

---

## Verdict summary

The core §5 implementation is real and strong. `POST /api/v1/meta-specs/preview`
genuinely spawns one agent per target (real `Active` agent rows, real worktrees
on disk, real `preview/{preview_id}/{slug}` branches, real short-lived JWTs),
returns the exact 202 spec shape, injects the draft into the agent process env
(`GYRE_META_SPEC_DRAFT_KIND`/`GYRE_META_SPEC_DRAFT_CONTENT`/
`GYRE_TARGET_SPEC_PATH`, `GYRE_TASK_ID` deliberately absent) and into the LLM
prompt as governing context; skip-ceremony holds (no task, MR, gates, merge
queue, provenance, refs writes); completion lands Stopped-never-Idle with the
worktree removed and the branch kept; status reports real per-agent state plus
a merge-base-pinned diff; DELETE and the registered `meta_spec_preview_gc` job
(default 1 h interval, 24 h TTL) tear everything down; the hollow
blast-radius preview is fully removed; all six routes are ABAC-resolver
mapped; kv records are port-backed (SQLite/PG), not in-memory. The test suite
is hard: real bare git repos in tempdirs, real spawned processes with env
dumps, real minted tokens driven through the production router.

However, the preview-agent containment class that this task itself ships
(`caller_is_preview_agent` guards on create_mr, transition_mr_status,
approve_spec, task/agent status, stop/fail, compose, orchestrator spawn,
merge-queue cancel, MCP task/MR/complete tools) is **incompletely enforced**:
two mutation surfaces of exactly that class remain reachable by preview-agent
JWTs (findings F1, F2 below), and there are two further spec-conformance /
robustness defects (F3–F5). Verdict: not approved; findings below.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `bash scripts/check-arch.sh` → exit 0
- `bash scripts/check-abac-route-registry.sh` → exit 0 (exemption list shrank by 2)
- 15 further mechanical checks (mcp-write-tools, abac-exempt-handlers,
  migration-versions, dead-message-kinds, in-memory-state-stores,
  fail-open-ref-resolution, forged-scope-fields, lossy-secret-conversion,
  scope-literal-defaults, fabricated-scope-defaults, inert-enforcement,
  relative-path-defaults, unbounded-external-http, mem-port-contracts,
  migration-sql-portability, conditional-test-guards) → all exit 0
- `check-task-commit-attribution` fails only because the controller's
  working-tree reset emptied `commits:` in specs/tasks/task-206.md; at HEAD
  da87710d the list is complete and covers the flagged commits — not a
  candidate defect.
- `check-assertionless-tests` exits 2 identically on base 7a80e459
  (verified in a temp worktree) — pre-existing environment behavior, not a
  regression.
- `cd docker/gyre-agent && node --test agent-runner.test.mjs` → 12/12 pass,
  exit 0 (incl. the §5 preview test: draft reaches SDK prompt, gate marker
  never created, no `/complete` or `/mcp` POSTs).
- `cargo check -p gyre-server --lib --tests` → clean (9m36s).
- `npx vitest run src/__tests__/MetaSpecs.test.js src/__tests__/EditorSplit.test.js`
  → 178/178 pass.

## Findings (blocking)

### F1 — `submit_review` lacks the preview-agent refusal (missing enforcement)

`crates/gyre-server/src/api/merge_requests.rs:1167` (`submit_review`) has no
`caller_is_preview_agent` guard, while its siblings `create_mr`
(merge_requests.rs:321) and `transition_mr_status` (:976) do, with comments
naming "the same escape class", and `approve_spec` (api/specs.rs:456) is
guarded for exactly this reason: an agent-authored review can satisfy review
gates. A preview agent (attestation 0, running under an unvetted draft
persona) authenticates as an agent-token principal, so the review is bound to
its own id — and recorded on a **real** MR. Where a repo's gate constraints
are permissive (default), that review counts toward merge decisions on real
MRs. Reproduce: drive a preview-agent JWT (the test harness at
meta_specs.rs:6166ff shows how) against
`POST /api/v1/merge-requests/{id}/reviews` → review recorded (no 403).

### F2 — message send (REST + MCP) lets preview agents inject durable messages into real agents' inboxes

`crates/gyre-server/src/api/messages.rs:80` (`send_message`) and
`crates/gyre-server/src/mcp.rs:5166` (`gyre_message_send` dispatch; the
preview denial list at mcp.rs:5075-5081 covers only create/update task,
create_mr, agent_complete, conversation_upload) have no preview guard. A
preview agent can send Directed-tier (durable until acked) messages to any
real agent in its (real) workspace — a durable footprint plus an influence
channel into live production agents, against §5's scratch-work containment
("no refs, no provenance, no audit trail"). `gyre_message_ack`,
`gyre_agent_heartbeat` and `gyre_record_activity` are likewise unguarded
(heartbeat can keep a real agent alive against stale-reaping). Same class as
F1; the task's own commits ("close the preview-agent escape class",
"refuse preview agents on the five remaining REST escape routes") establish
this as in-scope.

### F3 — UI: duplicate keyed-each keys when two repos share a spec path

`web/src/components/MetaSpecs.svelte:839, :857, :882` key their `{#each}`
blocks on `spec_path` alone, but targets are `{repo_id, spec_path}` pairs and
`targetSpecs` is built from all workspace specs with a repo_id
(MetaSpecs.svelte:410-415). Two workspace repos both exposing e.g.
`specs/system/design.md` produce duplicate keys (Svelte keyed-each error /
misrendering) and indistinguishable tabs (labels are the spec slug only). The
server explicitly disambiguates this input class
(`preview_branch_name`, meta_specs.rs:1676-1706) — the UI does not. Tests only
use unique paths.

### F4 — UI: diff viewer does not update as each preview agent completes (spec §5 iteration step 4)

Spec: "The diff viewer updates as each preview agent completes; multiple
targets show as tabs." In `MetaSpecs.svelte` the running panel
(:850-875) renders only status labels — the per-agent `diff` already present
in mid-run status polls is discarded — and the diff viewer with tabs
(:876-940) mounts only when `previewState === 'complete'`, which the server
sets only when **all** agents are terminal. A slow second agent hides a
finished first agent's diff for the whole remainder of the run. Same behavior
in `EditorSplit.svelte` (`codeDiffFiles` set only in `applyPreviewResult`).

### F5 — transient liveness-probe failure destroys a live preview run

`crates/gyre-server/src/api/spawn.rs:2727` (`monitor_poll_once`) maps an
`is_alive` error to "not alive" (`.unwrap_or(false)`), and
`SshDockerTarget::is_alive` (`crates/gyre-adapters/src/compute/ssh.rs:537-543`)
maps SSH/inspect failure to `false` (`.unwrap_or_default()`). One failed SSH
probe therefore runs the full exit path → `finish_preview_agent` → destructive
teardown (token revoked, worktree force-removed, slot released, row Stopped)
while the remote process still runs and its work is lost. For normal agents
the same transient merely parks them Idle (recoverable); for previews it is
unrecoverable. The exit hook should distinguish "confirmed dead" from "probe
failed", or preview teardown should be re-confirmed before destroying the
worktree.

## Findings (low, non-blocking but should be fixed)

- `meta_specs.rs:1557-1560` — `require_preview_scope` skips deleted repos
  (`None => continue`): a run whose every target repo was deleted performs
  zero scope checks, so an Admin/Developer in any tenant can read/tear down
  that run's record (metadata only — agent ids, branches).
- `meta_specs.rs:1349-1354` — `is_preview_agent` returns `false` on a KV
  error: during a store outage every REST/MCP preview refusal silently stops
  firing (fail open). Contrast `finish_preview_agent`, which handles `Err`
  explicitly.
- GC test (`meta_specs.rs:5933`): the retained case is a fresh run (age ≈ 0),
  not just-under-TTL, so a threshold bug anywhere below TTL would pass; and
  for the aged run only kv-record deletion is asserted (no kill / branch /
  worktree effect).
- `kill_process` implementations swallow non-zero kill status and send only
  SIGTERM (no KILL escalation) — teardown proceeds with the process possibly
  alive.
- `release_preview_slot`/`mark_preview_slot_released` are non-CAS
  read-modify-writes; concurrent terminal paths can double-decrement.
- Dead drifting duplicate preview loop in `EditorSplit.svelte` (never calls
  `deleteMetaSpecPreview`; fails the whole preview on first poll error);
  stale locale keys (`sim_banner`, `preview_unavailable_label`,
  `no_arch_changes`); duplicated `console.error` banner in
  `docker/gyre-agent/agent-runner.mjs:773-774`.

## Verified non-findings (checked and dismissed)

- **Workspace budget bypass when `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` is set**
  — intentional and documented (docs/server-config.md:87: "Preview runs use
  this budget instead of the normal workspace budget"), consistent with the
  spec sentence "separate preview budget (configurable, defaults to workspace
  budget)". Unset → workspace budget applies (tested).
- **Preview agent can push to any branch via git-receive-pack** — pre-existing
  systemic property for all repo-write agents (normal workers equally
  unrestricted); not a task-206 regression.
- **Workspace-scoped preview routes retained** (plan step 6 said remove) —
  they are real, resolver-enforced, containment-tested twins of the §5
  machinery, documented; the spec-conformant global routes exist and the
  hollow implementation is gone. Deviation from plan, not from spec.
- Generated `web/dist/` matches upstream — owned by the verifier, noted only.

## Required verifier tests

- `cargo test -p gyre-server --lib -- preview_` (full preview suite; run with
  `RUST_TEST_THREADS=1` — the env-dump tests mutate process-global env under
  `ENV_LOCK`, and the pipeline scripts set that variable)
- `cd docker/gyre-agent && node --test agent-runner.test.mjs`
- `cargo test --all`, `cd web && npm ci && npm test`
- `bash scripts/check-arch.sh`, `bash scripts/check-abac-route-registry.sh`
