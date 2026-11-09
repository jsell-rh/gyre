---
title: "Implement automatic jj rebase on target branch movement"
spec_ref: "source-control.md §4. Automatic Rebasing"
depends_on: []
progress: complete
review: specs/reviews/task-106.md
coverage_sections:
  - "source-control.md §4 Automatic Rebasing"
commits: ["f8be8730c81540598867316d46588b6e78cd265d", "6227b6e17e65659f805383fb9f528e49e310a0f3", "bb1099a10325fd4f58f63aed0b26c70ac295ce44", "bf2c2308bc21276dce979d37b10c78c16d0bb174", "e088a007daf767a8b1c818366057bb335a540124", "6d15a91fe5ee682660e5fd8216d5a3404d139586", "a5b167e33e4e768edafac1881a93e702d95861b2", "e6e393f7f8d610434378e6c96359fe62427fe688"]
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
Core behavior shipped in rounds 1–5; later rounds were review, CI,
verification-gate, base-merge, and completion-path repairs.

### Base-merge round (R24, commit `80772a600`)

Merged assignment base `774737638` (task-177 candidate). The auto-merge
left four content conflicts; resolution preserves BOTH sides' work:

- `merge_processor.rs`: the task-106 rebase test block was kept verbatim
  (byte-identical to pre-merge HEAD) and the base's new
  `require_current_spec_blocks_stale_spec_ref` test (spec-approval-ledger
  regression) was inserted after it, before the task-077 section. The
  criss-cross conflict split both tests around shared `.await.unwrap()`
  lines, so both were reconstructed whole rather than interleaved.
- `api/spawn.rs`: kept the base's new
  `secret_rotation::revoke_agent_task_secrets` teardown call (Platform
  Model §7); dropped the base's `jj_bookmark_create` block at the complete
  path because it is superseded by R20's F1 fix (`jj_bookmark_set` +
  `jj git export` BEFORE MR stats — reintroducing create would regress
  "Bookmark already exists" and export a stale spawn-time bookmark).
- `lib.rs`: kept both sides' tests — task-106's `configured_repos_path`
  F6 pins and the base's `seed_builtin_personas` suite (both were
  independent additions at the same spot).
- `.github/workflows/e2e.yml`: both sides pinned `runs-on: ubuntu-24.04`
  with equivalent rationale comments; kept one merged comment.

Evidence (logs in `/tmp/stage/review-evidence/task-106-r24/`):
`cargo test -p gyre-server --lib` → **1547 passed, 0 failed** (R22's 1505
plus the base's new suites; task-106 rebase tests, the base's
spec-approval-ledger and persona-seed tests, and the spawn/completion-path
tests all verified present and passing). `cargo test -p gyre-adapters
--lib` → 408 passed, 0 failed. `cargo check -p gyre-server --all-targets`
→ EXIT 0; `cargo fmt --check -p gyre-server` → EXIT 0;
`check-arch.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`
→ all EXIT 0.

### CI repair round (R23, commit `27b946cbc`)

Repaired the e2e CI failure at PR head `5a4318e9` (E2E run
42586542960): GitHub's `ubuntu-latest` now resolves to ubuntu26.04,
and the locked Playwright 1.58.2 has no `ubuntu26.04-x64` entry in its
browser registry — `npx playwright install --with-deps chromium`
fails with "ERROR: Playwright does not support chromium on
ubuntu26.04-x64". The e2e job is the only workflow job that installs
Playwright (all ci.yml jobs passed on the same image), so the fix pins
that one job's `runs-on` to `ubuntu-24.04`, a platform present in
1.58.2's registry. No product, test, or toolchain change — zero risk
to visual snapshots or assertion strength.

Evidence (logs in `/tmp/stage/review-evidence/task-106-r23/`):
- Root cause at source level: 1.58.2's
  `lib/server/registry/index.js` resolves the download path via
  `paths[hostPlatform] || paths["<unknown>"]` and throws exactly the
  logged error when empty; its supported-distro list is
  ubuntu 18.04–24.04 only.
- On an ubuntu24.04 host (this sandbox's OS = the pinned image's
  platform), `playwright install --dry-run chromium` with 1.58.2
  resolves valid download URLs (EXIT 0), and chromium-1208 — 1.58.2's
  pinned revision, preinstalled here — launches and renders (a
  `--no-zygote --disable-gpu --single-process` workaround was needed
  only for this container's seccomp limits; plain launch is blocked
  by "Operation not permitted" in chromium's zygote sandbox — an
  infrastructure restriction for remote verification, not a code
  defect).
- Alternative rejected: upgrading Playwright to 1.65.0 (latest; uses
  distro-agnostic `linux64` builds) would also fix the download, but
  it is a toolchain upgrade requiring e2e revalidation and is not
  needed to close this finding.

### Base-merge verification repair round (R22, commit `f8be8730c`)

Repaired the verification-gate failure after merging assignment base
`fefde2946` (merge commit `8c0110150`): the base added a sixth
`kv_store` parameter to `test_state_inner` in mem.rs, but the two
task-106 test helpers (`test_state_with_jj_ops`,
`test_state_with_jj_and_git_ops`) still passed five arguments, so
`cargo test -p gyre-server --lib` failed to compile with two E0061
errors. Both helpers now pass `None` (default `MemKvStore`), matching
every other helper — task-106's merge-processor rebase state reads
`state.kv_store`, which `test_state_inner` already populates. No
behavior change; test-infrastructure signature repair only.

### Spawn-response repair round (R21, commit `6227b6e17`)

Repaired the single finding of review `de1eec5d`:

- **F1 (minor) — response names a nonexistent directory**: the spawn
  response's `worktree_path` always cited
  `{repo_root}/worktrees/{branch_slug}`, but when jj provisioning
  succeeds — the designed-for common case — the plain-git worktree is
  NOT created and the agent's working directory is the jj workspace
  `{repo_root}/workspaces/{agent_id}` (the value persisted to the DB
  worktree record and listed by `GET /repos/:id/worktrees`). The REST
  spawn response, docs/agent-protocol.md, and the MCP
  `gyre_spawn_worker` result (which serializes the same response) all
  reported a path that does not exist, and the API was internally
  inconsistent. The response now returns the effective path —
  identical to the worktree record (jj workspace when provisioned,
  plain-git worktree otherwise); interrogation agents keep the
  historical placeholder (they have no worktree at all).
  `docs/agent-protocol.md` documents the semantics. New test
  `spawn_response_worktree_path_matches_recorded_worktree` covers BOTH
  provisioning outcomes (`ConfigurableJjOps` gained a
  `main_checkout_init_fails` knob for the fallback case) and asserts
  response == recorded path; the F9 spawn test now also pins the
  response value. Mutation-verified: reverting the response to the
  bare worktrees/ path makes both tests FAIL with the reviewer's
  exact repro; the fix makes them PASS (transcripts in
  `/tmp/stage/review-evidence/task-106-r21/`).

### Completion-path repair round (R20, commit `bb1099a10`)

Repaired the three findings of review `a442cdb`:

- **F1 (major) — bookmark set, not create**: `complete_agent` ran `jj
  bookmark create <source> -r @`, which fails with "Bookmark already
  exists" in exactly the layout this task ships (spawn's `create_branch`
  writes `refs/heads/<branch>` into the bare repo; jj's git import
  materializes it as a local bookmark), so the following `jj git export`
  exported the bookmark's stale spawn-time position and the agent's
  work at `@` never reached the bare repo — the MR diffed and merged an
  unchanged branch. New port method `JjOpsPort::jj_bookmark_set`
  (`jj bookmark set <name> -r <rev> --allow-backwards`, create-or-move)
  now backs the completion path (workspace AND legacy-colocated
  branches); the public `POST /repos/:id/jj/bookmark` endpoint keeps
  create semantics. Fixture test
  `jj_bookmark_set_moves_imported_bookmark_to_agent_work` pins the
  reviewer repro against real jj (create refuses on the imported
  bookmark; set moves it; export carries the agent's edit into
  `refs/heads/<branch>`) — guard-skipped locally (no jj binary), runs
  in CI against pinned jj-vcs 0.39.0 with `GYRE_JJ_TESTS_MUST_RUN=1`.
- **F2 — export before MR stats**: the bookmark-set + `jj git export`
  block moved BEFORE the MR's `diff_stats`/`has_conflicts` are
  computed from git refs, so a fresh MR carries the agent's actual
  completed work instead of pre-export (empty/stale) stats.
- **F3 — stale conflict flag cleared**: the rebase success arm now
  resets `has_conflicts` to `Some(false)` on the author's MR when an
  earlier conflicted rebase flagged it — the conflicts-as-state surface
  no longer reports state jj no longer has.

Both new server tests were mutation-verified: reintroducing each
pre-fix behavior (create-regression, export-after-MR-create,
no-flag-clear) makes them FAIL; the fix makes them PASS (transcripts in
`/tmp/stage/review-evidence/task-106-r20/`).

### Prior rounds

- **R19 base merge (`b99537ec`)**: merged assignment base `98a5299b9`
  (task-088/120 upstream work); no content conflicts on task-106 files.
- **R18 (`a5db6ecd`)**: merged base `72ad2a128` (task-095/096/077);
  took the base's real fixes for the two attestation sites
  (exemptions file byte-identical to base), kept both test suites in
  merge_processor.rs and both sides' spawn.rs improvements.
- **R16–R17 (`e088a007`, `bf2c2308`)**: CI installs pinned jj-vcs 0.39.0
  and runs `cargo nextest run --all` with `GYRE_JJ_TESTS_MUST_RUN=1`
  (guard panics on missing jj); R17's inline-marker workaround was
  superseded by R18.
- **R10–R14**: rustfmt, clippy `#[allow]` annotations matching repo
  precedent.

### Behavior

- **Rebase trigger**: after a merge lands and post-merge gates pass,
  `rebase_inflight_agents` (merge_processor.rs) rebases every other
  in-flight agent targeting the same branch — same target branch, MR
  Open/Approved, agent Active, worktree present; merged authors excluded.
  Runs as the LAST step in both the single-entry and atomic-group paths.
- **jj layout**: per-repo shared jj main checkout at `{repo.path}/jj-main`
  (standalone, backed by the bare git repo) with per-agent workspaces at
  `{repo.path}/workspaces/{agent_id}`; plain-git worktree fallback at
  `{repo.path}/worktrees/{branch}` when jj provisioning fails.
  `jj_workspace_add` runs `jj git import` before resolving `-r <branch>`
  (F7), and on failure forgets the workspace and removes the
  half-created dir so the git fallback stays reachable.
- **`jj_rebase`** (port `JjOpsPort::jj_rebase`, adapter jj_ops.rs):
  `jj git import`, then `jj rebase -b @ -d <moved branch>` (whole
  in-flight stack, per the F5 spec-vs-plan divergence). Conflict
  detection via the `jj resolve --list -r @` exit-code contract
  (0 = conflicted with files on stdout, 2 = clean).
- **Completion path** (R20): on complete, a jj-workspace agent's source
  bookmark is SET onto `@` and exported to the bare repo BEFORE the MR's
  stats are computed, so diff/can_merge/merge see the actual work.
- **Conflict-as-state**: on conflict the author's MR gets
  `has_conflicts = Some(true)` (cleared again by a later clean rebase,
  R20 F3); `SpeculativeConflict` to the workspace and `Escalation` to
  the agent carry the conflicting-file list. On success the agent gets
  `baseline_moved` (Custom) with `new_base_sha` and `rebased_count`.
  All via `emit_event` (signed, persisted, WS).
- **Deferral semantics** (plan item 5 / F4 / F8): a movement inside the
  60s backoff window sets a `jj_rebase_pending` marker (newest base
  wins — batch semantics); `replay_pending_rebases` retries after
  expiry on every processor cycle; the backoff timestamp is written
  only after a terminal outcome; an infrastructure Err keeps the
  pending marker on BOTH trigger and replay paths.
- **Absolute paths + audit** (F6): `configured_repos_path()` anchors
  `GYRE_REPOS_PATH` at the process cwd at server start;
  `spawn_agent_core` absolutizes legacy relative `repo.path` rows before
  any jj/git child call. All rebase attempts (success, conflict, error)
  are recorded as audit events (`jj_rebase` / `jj_rebase_conflict`).

### Test evidence

At HEAD `f8be8730c` (R22). Probe logs + exit codes in
`/tmp/stage/review-evidence/task-106-r22/` (R21's in `…/task-106-r21/`, R20's
in `…/task-106-r20/`):

- R22 (post-base-merge repair): `SKIP_WEB_BUILD=1 cargo test -p
  gyre-server --lib` → **1505 passed, 0 failed** (the pre-merge 1439 plus
  the merged base's upstream suites; this was the exact compile-failing
  probe in verification). `cargo check --workspace --all-targets` → EXIT 0.
  `cargo fmt --check -p gyre-server` → EXIT 0. `check-arch.sh`,
  `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-task-commit-attribution.sh` → all EXIT 0.
- R21 evidence: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1439
  passed, 0 failed** (R21's new spawn-response test + all prior task-106
  tests, the base's task-077/095/096 suites, and the R20 completion-path
  tests).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` → **391 passed,
  0 failed** (new fixture test guard-skips here: no `jj` binary in this
  sandbox; CI's R16 leg runs it against pinned jj-vcs 0.39.0 with
  `GYRE_JJ_TESTS_MUST_RUN=1`) — an installation restriction for remote
  verification, not a code defect.
- `cargo check -p gyre-server --all-targets` → EXIT 0.
- `cargo fmt --check` on the 5 changed files → EXIT 0.
- Clippy `--all-targets` on ports/adapters/server: warning-location
  diff vs the pre-change baseline shows only line-number shifts of
  pre-existing warnings — zero new findings.
- `scripts/check-mem-port-contracts.sh`, `check-arch.sh`,
  `check-inert-enforcement.sh`, `check-byte-slice-truncation.sh`,
  `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`
  → all EXIT 0.
- Mutation verification: all three pre-fix behaviors reintroduced →
  both new tests FAIL; fix restored → both PASS (transcripts in the
  evidence dir).

All builds used `SKIP_WEB_BUILD=1` (documented Rust-only path, committed
`web/dist/`): npm registry tarball fetches are blocked from this sandbox
(EACCES), so build.rs `npm ci` cannot run here — a transport restriction
for remote verification, not a code defect.
