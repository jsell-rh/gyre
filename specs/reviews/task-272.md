# Review — task-272 (Publish reviewed spec backlog for specs/development/agent-workflow.md)

Candidate: `a43b041696d9425562f12ff1a98c5a2a132e52b3` (branch
`pipeline/task-272/976aaf7ed8d543688cc17f1338699aff-1`), base `ee03c299`.
Verdict: **approved** — no findings. Evidence:
`/tmp/stage/review-evidence/task-272-review.md`.

## What was reviewed

Document-only backlog publication: task-268 (pre-pr-local-verification),
task-269 (terminal-worktree-cleanup, depends_on task-258), task-270
(pr-protection-and-hygiene), task-271 (interrupt-ledger-hardening-gate), the
publication document task-272 itself, and the four section bindings +
spec-gap binding keys in `specs/task-spec-inputs.json`.

## Probes (all at the checked-out candidate)

- `git diff --stat ee03c299..a43b0416` → exactly 6 files, 653 insertions,
  0 deletions, all under `specs/` (task docs + registry). No product code,
  scripts, specs, coverage files, or gates touched; canonical
  `specs/development/agent-workflow.md` byte-identical to base (sha256
  `3131f954...` at both revisions).
- Programmatic plan-fidelity comparison (`compare-plan.py`/`compare-pub.py`,
  retained in the evidence dir): every plan field — title, depends_on,
  required_behavior, acceptance list, negative probe, covered section ids —
  reproduced **verbatim** in task-268..271; task-272 embeds the entire
  reviewed plan JSON verbatim (parsed and deep-compared equal).
- Registry: `jq` parse OK; four new backlog section entries with
  file+headings exactly matching the spec's actual headings; four new
  bindings each 1:1 (reverse lookup confirms no duplicate binding); the four
  section ids and the spec path appear nowhere else in `specs/tasks/` — no
  duplicate active assignment.
- Dependency: task-269 → task-258 exists (`agent-session-commit-tracking`,
  not-started); ordering rationale (session end-state capture before worktree
  teardown) is documented in the plan. No missing dependencies.
- All four new tasks `progress: not-started`.

## Audit challenge (spot-checked every gap claim against the candidate tree)

- **Immediate Feedback gap is real:** `agent-runner.mjs` reads only
  `GYRE_TASK_PROMPT` (line 263) and `GYRE_META_SPEC_PROMPT` (line 295) —
  never `GYRE_PROTOCOL_PROMPT`, though spawn.rs:1985 injects it; the
  built-in task prompt says "Just implement, commit, and push" (line 281)
  with no local verification; the error path explicitly force-completes
  ("Still try to complete" → POST /complete, lines 414-429).
- **Worktree-cleanup gap is real:** `remove_worktree` appears in server code
  only in admin.rs, gate_executor.rs, stale_agents.rs (plus adapters/test
  mocks) — not on the completion path; `ralph_loop.rs:684` ("agent has no
  worktree to re-spawn") confirms the respawn-reuse placement constraint
  task-269's required behavior is built around.
- **PR-protection gap is real:** `pre_accept.rs` catalog is exactly
  conventional-commit / task-ref / no-em-dash — no default-branch push
  rejection; `abandoned_branch.rs` is daily, spec-edit/*-only, and
  notification-only.
- **Interrupt-ledger gap is real:** case-insensitive grep for
  "interrupt.ledger" over crates/, web/, scripts/, docker/ → zero matches.

The audit's gap decisions, the four-way decomposition (parent heading
correctly recorded as context to avoid double-counting), and the dependency
analysis are all sound. The task-270 scoping decision — agent-principal-only
default-branch push rejection, preserving the human/system push path that
`specs/system/spec-lifecycle.md` normatively requires — is a reasoned
reconciliation of a genuine inter-spec conflict, stated transparently in the
task's required behavior with companion negative probes guarding both
directions; the publication itself amends neither spec, so no spec
lifecycle step is bypassed.

## Reconciliation round

Merge `249facf1` (candidate `2459036a` + main `ee03c299`) resolved the
purely-additive registry conflicts as the union: main's task-273/274
documents and admin-panel bindings are preserved (final tree vs base: 0
deletions), and this branch's four sections/bindings are intact. The
shipped notes in task-272.md accurately describe the resolution; the
canonical spec is byte-identical to both merge parents.

## Test-inflation / fakery check

Not applicable — no tests or production code shipped, and none claimed: the
shipped section states "Task definitions and explicit bindings only; no
product implementation claimed." The publication preserves every existing
task, spec, registry entry, and coverage status (zero deletions).

## Verifier notes

Document-only change; no focused compilation or runtime probes are
meaningful (no production source changed). Verifier should run the standard
repository-wide gates on the merged tree as usual.
