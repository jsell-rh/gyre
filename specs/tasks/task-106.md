---
title: "Implement automatic jj rebase on target branch movement"
spec_ref: "source-control.md §4. Automatic Rebasing"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-106.md
coverage_sections:
  - "source-control.md §4 Automatic Rebasing"
commits: ["6d15a91fe5ee682660e5fd8216d5a3404d139586", "a5b167e33e4e768edafac1881a93e702d95861b2", "e6e393f7f8d610434378e6c96359fe62427fe688", "e088a007daf767a8b1c818366057bb335a540124"]
---

## Spec Excerpt

From `source-control.md` §4:

> When the target branch moves (because another agent's MR merged), jj automatically rebases the agent's in-progress work. With git, the agent must manually `git fetch && git rebase`, handle conflicts, and continue. In a system with 20+ agents pushing concurrently, the baseline moves constantly. jj handles this transparently.

Current state: The speculative merge system (`speculative_merge.rs`) detects conflicts when the target branch moves, but it does not trigger an automatic `jj rebase` on the agent's working branch. Conflict detection is passive — the agent must manually handle the rebase.

## Implementation Plan

1. **Rebase trigger in merge processor:**
   - After a successful merge (MR lands on default branch), identify all other in-flight agents targeting the same branch
   - For each affected agent, trigger a `jj rebase` operation on their working change
   - This should run in `gyre-server/src/merge_processor.rs` after the merge commit

2. **jj rebase adapter (`gyre-adapters/src/jj_ops.rs`):**
   - Add `jj_rebase(repo_path, revision, destination)` function
   - Execute `jj rebase -r <revision> -d <destination>` in the agent's worktree
   - Capture and return rebase result (success, conflict, or error)

3. **Conflict handling:**
   - If rebase produces conflicts, emit a `SpeculativeConflict` event
   - Set the conflict state on the agent's MergeQueueEntry
   - Notify the agent via WebSocket message (MessageKind::Escalation or similar)
   - Agent can continue working on non-conflicting files (jj conflict-as-state model)

4. **Agent notification:**
   - After successful rebase, send a WebSocket notification to the agent informing it the baseline moved
   - Include the new base commit SHA in the notification
   - After conflict rebase, send conflict details with affected file list

5. **Safeguards:**
   - Only rebase agents that are in Active status (not Dead/Completed)
   - Skip rebase if the agent's worktree path doesn't exist
   - Log all rebase operations for audit trail
   - Rate-limit rebases: if target moves multiple times in quick succession, batch into one rebase

## Acceptance Criteria

- [x] `jj_rebase` function added to jj_ops adapter
- [x] After MR merge, in-flight agents on same target branch are automatically rebased
- [x] Conflicts from rebase are surfaced as state (not errors)
- [x] Agents notified via WebSocket of baseline movement
- [x] Rebase skipped for dead/completed agents
- [x] Rebase operations logged
- [x] `cargo test --all` passes

## Spec-vs-Plan Divergence (review F5)

The plan said `jj rebase -r <revision> -d <destination>`; the spec (§4) says
the agent's "in-progress work" — the whole stack — moves onto the new base.
`-r @` would rebase only the working-copy commit and abandon its descendants
onto the old parent; the adapter therefore runs `jj rebase -b @ -d <dest>`,
which rebases the branch containing `@` (the entire in-flight stack).
Verified empirically against jj 0.39.0 (see
`jj_rebase_clean_after_target_moves` in `crates/gyre-adapters/src/jj_ops.rs`).

## Agent Instructions

Read `specs/system/source-control.md` §4 for the full spec. The jj adapter is in `gyre-adapters/src/jj_ops.rs` — add `jj_rebase` alongside existing `jj_new`, `jj_undo`, etc. The merge processor is in `gyre-server/src/merge_processor.rs` — the post-merge hook is where rebase should be triggered. The speculative merge system is in `gyre-server/src/speculative_merge.rs` for conflict detection patterns. WebSocket messaging is in `gyre-server/src/ws.rs`. Agent worktree paths are tracked in `agent_tracking.rs`.

## Shipped

Automatic jj rebase on target branch movement (source-control.md §4).
Production behavior is unchanged since round 5; rounds 6–14 were review,
compile, and gate repairs; round 16 repaired the CI execution gap for the
jj recorded-fixture tests.

### CI repair round (R16, commit `e088a007`)

Review finding F-CI (major): the CI test job installed no jj, so all 13
jj recorded-fixture tests in `jj_ops.rs` guard-skipped with zero
assertions executed on every CI run — and
`jj_workspace_forget_absent_workspace_is_ok` had never run against a
real jj binary in any recorded round. The exemption-file comment
claiming the fixture tests "now run in CI" was false. Repaired with no
production-behavior change:

- **ci.yml test job**: installs pinned jj-vcs 0.39.0
  (x86_64-unknown-linux-musl release tarball; asset name and archive-root
  layout verified against the release workflow at that tag via the
  GitHub API; version asserted with a `jj --version` grep) and runs
  `cargo nextest run --all` with `GYRE_JJ_TESTS_MUST_RUN=1`.
- **Fail-on-guard-skip** (`jj_ops.rs`): `jj_available()` is replaced by
  `jj_fixture_guard()` — without the env var a missing jj still skips
  quietly (developer machines unchanged), but under
  `GYRE_JJ_TESTS_MUST_RUN` a missing jj PANICS with an explanatory
  message, so a broken CI install step fails the leg loudly instead of
  silently degrading it to zero-assertion passes. Two new unit tests pin
  the guard contract itself
  (`fixture_guard_panics_on_missing_binary_when_must_run`,
  `fixture_guard_skips_quietly_without_must_run`).
- The false exemption-file comment is rewritten to state the actual
  mechanism, which this change makes true.

Evidence: `/tmp/stage/review-evidence/task-106-r16/` — skip path
`cargo test -p gyre-adapters jj` EXIT 0 (13 fixture tests skip here, no
jj installable in this sandbox); must-run path
`GYRE_JJ_TESTS_MUST_RUN=1 cargo test -p gyre-adapters jj` EXIT 101 with
all 13 fixture tests failing loudly at the guard (the fail-on-skip
behavior proven on a machine where jj is genuinely absent). The
real-binary 15/15 run — including the never-executed forget test — is
now the CI test job's behavior on every run; exact-head GitHub checks
confirm it during verification (this sandbox cannot fetch the jj
release CDN or crates.io; see the round's 00-infra-restrictions.md).

### Gate-repair round (R13/R14, commit `6d15a91f`)

The executor gates flagged two failures at candidate `c3d83fdb` vs the
assigned base; both are repaired with no behavior change:

- **rustfmt-diff**: the R12-F1 test
  `rebase_skips_agent_with_only_other_repo_worktree` had an unformatted
  multi-line `if` that fits on one line per default rustfmt style.
  `rustfmt --edition 2021` on the candidate-owned `merge_processor.rs`
  changed exactly those three lines (verified: single-hunk diff).
- **check-inert-enforcement**: the R12 production comment hunks drifted
  the two pre-existing task-077-owned `evaluate_attestation_abac`
  exemption pins from 2014/2036 to actual 2022/2044; both entries
  re-pinned (entry count unchanged at 4 repo-wide, sites byte-identical).

Earlier gate repairs (R10/R11, commit `cf800612`): rustfmt on the six
candidate-owned Rust files, `#[allow(clippy::double_must_use)]` on
`JjOpsPort` (async_trait `#[must_use]` futures; same annotation as
`StoragePort`), `#[allow(clippy::too_many_arguments)]` on
`record_rebase_audit` (repo-standard), and the same two exemption
re-pins after the +523-line insertion.

### Behavior

- **Rebase trigger**: after a merge lands and post-merge gates pass,
  `rebase_inflight_agents` (merge_processor.rs) rebases every other
  in-flight agent targeting the same branch — same target branch, MR
  Open/Approved, agent Active, worktree present; merged authors excluded.
  Runs as the LAST step in both the single-entry and atomic-group paths
  (F3: never onto a base that gate-failure recovery would revert).
- **jj layout**: per-repo shared jj main checkout at `{repo.path}/jj-main`
  (standalone, backed by the bare git repo — jj 0.39.0 refuses colocation
  inside git worktrees) with per-agent workspaces at
  `{repo.path}/workspaces/{agent_id}`; plain-git worktree fallback at
  `{repo.path}/worktrees/{branch}` when jj provisioning fails.
  `jj_workspace_add` runs `jj git import` before resolving `-r <branch>`
  (F7), and on failure forgets the workspace and removes the
  half-created dir so the git fallback stays reachable.
- **`jj_rebase`** (port `JjOpsPort::jj_rebase`, adapter jj_ops.rs):
  `jj git import`, then `jj rebase -b @ -d <moved branch>` (whole
  in-flight stack, per the F5 spec-vs-plan divergence). Conflict
  detection via the `jj resolve --list -r @` exit-code contract
  (0 = conflicted with files on stdout, 2 = clean) — no text scraping
  for the decision (F1).
- **Conflict-as-state**: on conflict the author's MR gets
  `has_conflicts = Some(true)`; `SpeculativeConflict` to the workspace
  and `Escalation` to the agent carry the conflicting-file list. On
  success the agent gets `baseline_moved` (Custom) with `new_base_sha`
  and `rebased_count`. All via `emit_event` (signed, persisted, WS).
- **Deferral semantics** (plan item 5 / F4 / F8): a movement inside the
  60s backoff window sets a `jj_rebase_pending` marker (newest base
  wins — batch semantics); `replay_pending_rebases` on processor idle
  cycles retries after expiry; the backoff timestamp is written only
  after a terminal outcome; an infrastructure Err keeps the pending
  marker on BOTH trigger and replay paths.
- **Absolute paths + audit** (F6): `configured_repos_path()` anchors
  `GYRE_REPOS_PATH` at the process cwd at server start;
  `spawn_agent_core` absolutizes legacy relative `repo.path` rows before
  any jj/git child call. All rebase attempts (success, conflict, error)
  are recorded as audit events (`jj_rebase` / `jj_rebase_conflict`).

### Test evidence

At HEAD `e088a007` (R16 CI repair on top of the R14 repair). This
round's probe logs + exit codes in
`/tmp/stage/review-evidence/task-106-r16/`:

- `cargo test -p gyre-adapters jj` → **EXIT 0**, 17/17 (13 fixture tests
  skip — no jj installable from this sandbox: the jj release CDN and
  crates.io are both transport-blocked; round-5 real-jj evidence 14/14
  against jj 0.39.0 stands, and the CI test job now installs that exact
  version and runs all 13 for real on every push).
- `GYRE_JJ_TESTS_MUST_RUN=1 cargo test -p gyre-adapters jj` → **EXIT
  101**, 4 passed / 13 failed — every fixture test panics at the guard
  with "GYRE_JJ_TESTS_MUST_RUN is set but `jj --version` failed" — the
  fail-on-guard-skip contract, proven on a machine where jj is genuinely
  absent.
- `python3 scripts/check-rustfmt-diff.py 88bd81bf` → **EXIT 0**;
  `bash scripts/check-ignored-tool-tests.sh` → **EXIT 0**;
  `bash scripts/check-inert-enforcement.sh` → **EXIT 0**.
- The R11 full-suite evidence stands: `cargo test -p gyre-server --lib`
  → 1293/1293 EXIT 0 (includes all eight task-106 rebase tests and the
  F9 test).

All builds used `SKIP_WEB_BUILD=1` (documented Rust-only path, committed
`web/dist/`): npm registry tarball fetches are blocked from this sandbox
(EACCES), so build.rs `npm ci` cannot run here — a transport restriction
for remote verification, not a code defect.
