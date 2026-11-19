# Review: task-272 — Publish reviewed spec backlog for specs/development/agent-workflow.md

Candidate: `4cc2415e8c4f14eba97fdd54fc49d8418541a850` (base `25497b9e3cc16c7e405c641870ff3c3b1905cf6b`)

## Scope of change

Exactly six files added, nothing modified or removed:
`specs/tasks/task-268.md` … `task-272.md` and additive entries in `specs/task-spec-inputs.json`
(4 `sections` entries + 4 `bindings` entries; flat JSON diff confirms 12 additions, 0 changes,
0 removals). No production code, scripts, canonical specs, coverage matrices, or gates touched.

## Evidence

1. **Byte-exact plan fidelity.** Each of the five task documents was compared against a
   reconstruction from the assigned reviewed plan using the pipeline's own `reconcile()`
   template (`scripts/pipeline/spec_backlog.py`): frontmatter, required_behavior, acceptance
   items, negative_probe, discovery-evidence gap JSON, and the publication body all match
   byte-for-byte. The embedded plan's recomputed `plan_id`
   (`b3acf94c…`) matches, so the published plan is the reviewed one, unmodified.
   (`/tmp/stage/review-evidence/body-diff.txt`)

2. **Canonical identities, not invented labels.** The spec ref
   `spec-4c39ca6ea3e71ce8a53398f3` equals the pipeline derivation
   `'spec-' + sha256(json("specs/development/agent-workflow.md"))[:24]`, and each of the four
   section ids (`d5679654…`, `346ce862…`, `4a3b61a0…`, `5d7c43ad…`) equals
   `digest([path, heading_path])[:24]` for the four child headings of the canonical spec —
   headings verified against `specs/development/agent-workflow.md` (title-only parent is
   correctly classified `context`; no double counting).

3. **Pipeline's own validator passes.** `snapshot(body, contract, require_registry=True)`
   (the exact check `reconcile()` runs before publishing) returns zero errors for all five
   documents; each task resolves to exactly its assigned backlog section, and the GOAL
   binding for the publication task is intact.
   (`/tmp/stage/review-evidence/snapshot-validation.txt`)

4. **Audit challenged against code — all claims hold.** Spot-checks on the candidate tree:
   `remove_worktree`/`worktrees.delete` absent from the completion path (only stale-agent
   Dead, admin kill, manual DELETE route, post-merge gate worktree); "interrupt ledger"
   absent from all production source; `agent-runner.mjs` reads only `GYRE_TASK_PROMPT` /
   `GYRE_META_SPEC_PROMPT` and never `GYRE_PROTOCOL_PROMPT`, and the server never sets
   `GYRE_TASK_PROMPT`; `ralph_loop.rs:684` "agent has no worktree to re-spawn" (respawn
   reuses the worktree — the placement constraint behind task-269's terminal-state scoping);
   `prompts.rs:157/163/167` quotes accurate; `abandoned_branch.rs` daily spec-edit-only
   notification scope accurate; latest migration is `000066`; `git_http.rs` processes
   default-branch pushes as accepted events; `specs/system/spec-lifecycle.md:11,110`
   contains the human-push-preservation text task-270 reconciles against;
   `docs/agent-protocol.md:54` carries the stale "removes the git worktree" claim.
   All nine `source_evidence` sha256s match the candidate tree — the audit is fresh.
   (`/tmp/stage/review-evidence/audit-probes.txt`, `source-evidence-check.txt`)

5. **No duplicate assignment or missing dependency.** No pre-existing task references
   agent-workflow or its sections; the four backlog section ids are new and no existing
   binding was retargeted. `task-269`'s dependency `task-258` exists
   (agent-session-commit-tracking, not-started) and the ordering rationale (session
   end-state capture must run before worktree removal) is real.

6. **Acceptance criteria.** New tasks are `progress: not-started`; the publication task is
   `ready-for-review`; no product behavior, coverage status, or enforcement changed.

## Verdict

Approved. The candidate publishes exactly the independently reviewed plan: byte-exact task
contracts, correctly derived canonical section bindings, additive-only registry changes, and
a code-verified gap audit. Task creation only — no implementation claimed.
