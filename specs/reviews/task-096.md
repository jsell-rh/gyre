# Review — task-096 (Platform Model §6 Circuit Breaker + CLI/UI)

Spec: `platform-model.md` §6 "Circuit Breaker", "CLI", "UI" (coverage rows 36/37/38).
Candidate: `c6c5281b0d8f4423441151c83c46d6469805e38b` (work commit `109ec4d0b` "feat(task-096)" + process note commit), base `7e693c20e`.
Diff scope verified: 11 files, +928/−14 — `api/recovery.rs`, `api.js`, `RepoMode.svelte`, `MergeQueueGraph.svelte`, `WorkspaceHome.svelte`, `en.json`, 4 test files, `specs/tasks/task-096.md`. No routes, migrations, scripts, verifiers, or `web/dist` touched.

## Round 1 — approved

### Scope reconciliation (verified at candidate, not re-implemented)

The task's declared surface (circuit breaker, REST, CLI) was already delivered by the task-095 lineage merged at base `7e693c20e` — confirmed at candidate HEAD: `increment_revert_count`/`trip_circuit_breaker` (`merge_processor.rs:51/95`, tripped at `:2222`/`:2484` incl. the manual-revert path), CLI `RepoCommands::{Status,Revert,Queue}` + `QueueCommands::{Pause,Resume}` (`gyre-cli/src/main.rs:213-290`, dispatch `:1597+`, client `client.rs:1472-1531`), and the R3-F1/F3 fixes (merge-commit-sha-scoped revert, fail-closed `main_green` — `check-fail-open-ref-resolution.sh` still reports 0 exemptions). This commit ships the one genuinely open gap (row 38, §6 UI) plus one additive server capability, and does not duplicate or rewrite the delivered surface. Correct decomposition per the §6 2nd-cycle audit's PM notes.

### What was verified in this commit

- **Server (`api/recovery.rs`)** — `GET /repos/:id/status?gates=false` skips post-merge gate execution (`main_green: null`). Additive: no new route (ABAC registry unchanged, `check-abac-route-registry.sh` green), default behavior and the CLI call unchanged. Parsing is fail-safe in direction: only the exact values `false`/`0` (case-insensitive "false") skip; `true`, absent, or garbage all run gates. The task-095 R3-F3 fail-closed `resolve_ref` semantics are untouched (skipped gates report `None`, never green). New test `repo_status_gates_false_skips_gate_execution` uses a gate whose command is `false` and proves non-execution via `main_green: null`.
- **Queue-scoping join fix (`RepoMode.svelte`)** — verified against the server DTO: `QueueEntryResponse` (`api/merge_queue.rs:24-31`) carries no `repository_id`/`repo_id`, so the previous client filter (`e.repository_id === repoId`) was dead code and the repo-mode merge queue section never rendered. The new join (queue entries ∩ repo's MRs) is correct; reverted MRs badge `danger`.
- **UI (§6 bullets)** — green/red/unknown main-health badge in the repo header (from `main_green`); MRs-tab queue-state banner with main health, paused state **with reason** (`role=status`), and Pause/Resume controls wired to the Admin-gated endpoints (health reading preserved across pause/resume since those responses don't re-run gates); `MergeQueueGraph` paused banner (visible even when empty); `WorkspaceHome` paused chip (reason in title) + DAG banner, fetched with `withGates: false` so pause-state display never executes gate commands; `MrReverted` activity label + danger variant. Health is fetched on MRs-tab visit — documented rationale (`GET /repos/:id/status` runs minute-long gate commands server-side); the CLI's `gyre repo status` remains the always-real reading.

### Probe evidence (saved under /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib api::recovery` → **7/7 pass** (incl. fail-closed HEAD test and the new gates-skip test).
- `npx vitest run` on the 4 new/extended test files → **56/56 pass**.
- Full `npm test` in `web/` → **75 files, 1778 passed / 0 failed, 41 pre-existing env-gated skips** — matches the shipped claim; no regression in sibling suites (RepoMode.test.js, WorkspaceHome.test.js, api.test.js all green). Working tree clean afterwards (no `web/dist` drift).
- Mechanical checks green: `check-arch`, `check-fail-open-ref-resolution` (0 exemptions), `check-abac-route-registry`, `check-relative-path-defaults`, `check-forged-scope-fields`, `check-inert-enforcement`, `check-task-commit-attribution`.
- **Mutation probes (tests are not self-confirming):**
  - Broke the queue-entry join (re-introduced a never-matching filter) → `RepoModeHealth.test.js` "scopes merge queue entries to this repo via the MR join" **FAILS**. Restored.
  - Disabled the `gates=false` skip server-side → `repo_status_gates_false_skips_gate_execution` **FAILS**. Restored; re-ran full recovery suite 7/7 green; `git status` clean.

### Observations (non-blocking, recorded for the PM / next audit)

1. **Stale health badge on repo switch.** `repoHealth` is not cleared when `repo` changes (only `repoHealthLoaded` resets, `RepoMode.svelte:438`), so switching from repo A to repo B without visiting B's MRs tab leaves A's badge displayed in B's header. Same lazy-reset convention as the sibling per-tab state in the file (`mrsLoaded`/`mergeQueueLoaded`/`allAgentsLoaded`, lines 198/401/123) — cosmetic staleness, no enforcement or scoping impact.
2. **Workspace pause-chip coverage limit.** `loadMergeQueue` only resolves MR details for the first 20 queue entries (pre-existing `mrIds.slice(0, 20)` enrichment limit, `WorkspaceHome.svelte:943`), so with >20 queued MRs a paused repo beyond the first 20 gets no chip. Pre-existing limit, inherited by the new pause-state join.
3. **"Failure output" in the activity feed.** The sidebar feed renders label + entity link + time for every event type and no free-text descriptions; the MrReverted reason travels in the notification message (`merge_processor.rs:2311-2320`) and is reachable via the MR link, but is not rendered inline. Disclosed by the implementer; the §6 2nd-cycle audit had already deemed the activity-feed revert bullet satisfied at base, and this is a feed-wide design choice, not a revert-specific gap.
4. **Coverage rows 36–38** remain `task-assigned task-096` in `specs/coverage/system/platform-model.md` — PM-owned status per that file's own notes; reconciliation (including the R3-F2 resubmission-stable-key reachability question) belongs to the PM/audit cycle, not this commit.

### Verdict

**Approved.** Independent evidence supports the task contract: the §6 UI gap is genuinely closed with real API consumption and DOM behavior, the additive server capability is fail-safe and covered by a test that kills a real mutation, all declared test runs reproduce, and no mechanical invariant is violated. `cargo test --all` / GitHub CI are owned by verification/publication.
