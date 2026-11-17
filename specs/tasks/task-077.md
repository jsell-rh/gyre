---
title: "HSI Trust Gradient — Trust Levels, Enforcement & Mechanical Implementation"
spec_ref: "human-system-interface.md §9–13"
depends_on: []
progress: complete
review: specs/reviews/task-077.md
coverage_sections:
  - "human-system-interface.md §9 2. Trust Gradient"
  - "human-system-interface.md §10 The Problem"
  - "human-system-interface.md §11 Trust Levels"
  - "human-system-interface.md §12 What Each Level Controls"
  - "human-system-interface.md §13 Mechanical Implementation"
commits: ["2fea93f03006238a8f85f3e8f0b80cf2fa781603", "71b202e6386a1ee211f9f265e4d531afdf0d411c", "099f08c43f2afdbc53319850d57b6be1445a7ce7", "09d5266952f214c2e38911c4bcd548c4c79c0f68", "f4f32a01a8891b67010600b6af3857c9a4be695d", "accffacfbfd7cab9f6913964c0fe8da7e667748d", "dddcb3a4e4e36417e99cee451670d72ce5e72a45", "4e17ffa5c94b84b1b8c9f2890a94997424932a58", "5b5e7c65957c79b12111ae987d71f7752abac702", "3372f2c3e498d1968bd3ef97cd05853280807ccb", "1b07c83be8904c63ad97e8b5d59abf50b8f59878", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0"]
---

## Spec Excerpt

Trust is a **workspace-level setting** (`trust_level: TrustLevel` enum: `Supervised`, `Guided`, `Autonomous`, `Custom`). Changing trust level is a `PUT /api/v1/workspaces/:id` update (existing endpoint).

**What each level controls:**
| Aspect | Supervised | Guided | Autonomous |
|---|---|---|---|
| MR merge | Human approval required | Autonomous if gates pass | Autonomous if gates pass |
| Spec approval | Human required (always) | Human required (always) | Human required (always) |
| Notifications | Every state change | Failures and approvals | Exceptions only |

**Mechanical implementation:** Each trust preset maps to ABAC policies:
- **Supervised:** Creates `trust:require-human-mr-review` (Deny merge by system)
- **Guided:** Empty trust policy set (relies on built-in policies only)
- **Autonomous:** Removes notification policies, keeps `builtin:require-human-spec-approval`

**Policy naming:** `trust:` prefix for preset-managed (priority 100-199), `builtin:` for immutable server-seeded (priority per table), no prefix for user-created (priority 200-299). ABAC CRUD endpoint rejects `trust:` and `builtin:` prefix creation (400 error).

**Trust transitions** (workspace `trust_level` update + policy delete/create) are performed in a **single database transaction**. On rollback: 409 Conflict. ABAC cache invalidated after commit.

**`builtin:require-human-spec-approval`** is `immutable: true` at priority 999. Immutable Deny policies are evaluated FIRST, before any priority-based evaluation. This amends `abac-policy-engine.md` §Policy Composition.

## Implementation Plan

1. **Add `TrustLevel` enum** to `gyre-common`:
   ```rust
   pub enum TrustLevel { Supervised, Guided, Autonomous, Custom }
   ```

2. **Add `trust_level` field** to the Workspace entity in `gyre-common` and the workspaces DB table (migration). Default: `Supervised`.

3. **Add `immutable` field** to the Policy entity (amending the ABAC policy engine). Migration to add `immutable BOOLEAN NOT NULL DEFAULT FALSE` to the policies table.

4. **Implement trust preset policy sets** in `gyre-domain`:
   - `fn trust_policies(level: TrustLevel) -> Vec<PolicyTemplate>` returning the preset policies for each level
   - `fn apply_trust_transition(old: TrustLevel, new: TrustLevel, workspace_id: &Id) -> Result<()>` that deletes old `trust:` policies and creates new ones in a single transaction

5. **Amend the ABAC evaluation engine** in `gyre-domain`:
   - Process immutable Deny policies FIRST, before priority-based evaluation
   - Immutable Deny cannot be overridden by any Allow regardless of priority

6. **Update `PUT /api/v1/workspaces/:id`** handler to:
   - Accept `trust_level` in the update payload
   - Call `apply_trust_transition` in a single DB transaction
   - Invalidate ABAC policy cache after commit
   - Return 409 on transaction failure

7. **Seed `builtin:require-human-spec-approval`** at server startup (alongside existing built-in policies).

8. **Guard `trust:` and `builtin:` prefixes** in the ABAC policy CRUD endpoint — reject creation with 400.

9. **Add `trust_level` to Workspace API responses** so the UI can display it.

## Acceptance Criteria

- [ ] `TrustLevel` enum exists in `gyre-common`
- [ ] Workspace entity has `trust_level` field, default `Supervised`
- [ ] DB migration adds `trust_level` to workspaces, `immutable` to policies
- [ ] Trust preset policy sets defined for Supervised, Guided, Autonomous
- [ ] Trust transitions run in a single DB transaction (atomic)
- [ ] ABAC engine evaluates immutable Deny policies first
- [ ] `builtin:require-human-spec-approval` seeded at startup
- [ ] Policy CRUD rejects `trust:` and `builtin:` prefixes (400)
- [ ] `PUT /api/v1/workspaces/:id` accepts `trust_level`, applies transition
- [ ] 409 returned on failed trust transition
- [ ] ABAC cache invalidated after trust transition commit
- [ ] Unit tests for trust policy generation and transition logic
- [ ] Integration test: change trust level → verify policies created/deleted
- [ ] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/human-system-interface.md` §9–13 (Trust Gradient) carefully — the mechanical implementation section has precise details about policy naming, priorities, and transaction behavior. Also read `specs/system/abac-policy-engine.md` for the existing ABAC engine design. The key amendment is adding `immutable` flag support to the ABAC evaluation engine. Check `crates/gyre-domain/src/` for existing ABAC evaluation code and `crates/gyre-adapters/migrations/` for migration numbering (currently at 000046+). The workspace entity is in `gyre-common` — grep for `Workspace` struct.

## Shipped

### Review-repair round — HSI trust-gradient control rows enforced (commit `71b202e6`)

Review repair (the seven open findings in `specs/reviews/task-077.md`): every row of the HSI "What Each Level Controls" table that was previously documented-only is now mechanically enforced.

- **Notifications / Inbox / Inbox-priorities rows** — `TrustLevel` gains a notification classifier (`delivers_notification`, `suppressed_notification_types`, `inbox_excluded_types` in `gyre-domain/src/workspace.rs`): Guided suppresses state-change chatter (MrMerged, GatePassed, AgentCompleted, TrustSuggestion, ...); Autonomous additionally suppresses agent-owned failure types (GateFailure, AgentFailed, SpecAssertionFailure, ...) while action-required types (InvitationReceived, MembershipChanged) always stay visible — so the exclusion is a type set derived from the rank->10 band minus always-delivered, not a scalar `max_priority=9` cap. Applied server-side in the inbox and unread-count handlers (`api/users.rs`, workspace scope = that workspace's level, tenant scope = intersection, fail-open on lookup errors), in realtime push (`BroadcastingNotificationRepo` persists but skips SSE/WS broadcast for suppressed types), and in outbound channels (`dispatch_to_channels` skips email/webhook/slack likewise). `Inbox.svelte` additionally caps its request at `max_priority=9` for Guided/Autonomous workspaces.
- **Briefing detail / Agent completion summaries rows** — `assemble_briefing` (`api/graph.rs`) varies detail by the workspace trust level: per-MR completed rows plus the full agent decision log at Supervised/Custom; per-spec rollups ("N MRs merged") with per-spec task progress ("x of y tasks complete, z active") at Guided; terse counts ("N task(s) in progress") and low-confidence-only agent decisions at Autonomous. `Briefing.svelte` renders the completed-agents section (decision log with confidence badges, uncertainties highlighted at Guided/Autonomous), new i18n key `briefing.section_completed_agents`.
- **Admin row (workspace role, not tenant role)** — `PUT /api/v1/workspaces/:id` with `trust_level` in the body now requires an accepted Admin/Owner *membership in that workspace* (403 otherwise); the server's system token is exempt. `trust_level_change_requires_admin_workspace_role` covers Developer-no-membership, Developer-with-membership, non-trust-field updates, pending memberships, accepted Admin, and the system token.
- **Policy naming conventions (priority band)** — user-created (no reserved prefix) policy priorities must be 200-299 on both create and update (400 outside; boundary probes tested at 199/200/299/300/900). Existing test fixtures moved in-band; `PolicyEditor.svelte` defaults to 250 with the band documented in the UI.
- **Spec amendment** — `abac-policy-engine.md` Policy Composition rule 2 now records that immutable Deny policies are evaluated before all priority-based evaluation.
- **Non-atomic creation finding** — new port method `PersonaRepository::apply_approval_reset_with_task` persists a persona approval reset and its human-approval task in ONE transaction (SQLite and PostgreSQL adapters wrap both upserts in a single diesel transaction via extracted `upsert_task_txn`/`upsert_persona_txn` helpers; the mem adapter enforces the same contract in code with a `set_fail_task_writes` test hook; the search-indexing wrapper forwards the method and indexes both documents). `process_persona_changes` (`git_http.rs`) uses it, so a push can no longer leave a reset persona stranded without its approval task — `scripts/check-non-atomic-creation.sh` passes.

Test evidence (this round): `cargo test -p gyre-domain --lib` — 582 passed, 0 failed (includes the classifier suite); `cargo test -p gyre-adapters --lib` — 574 passed, 0 failed (includes `persona_approval_reset_with_task_persists_both_rows`); `cargo fmt --all -- --check` clean; `cargo check -p gyre-server --lib --tests` and `--tests` clean (server test *execution* is the independent verifier's gate — this sandbox's cargo wrapper rejects the gyre-server test target over the 4 GiB limit; required probes for the verifier: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` covering `inbox_trust_filter_*`, `trust_level_change_requires_admin_workspace_role`, `create/update_policy_rejects_out_of_band_priority`, `briefing_detail_and_completion_summaries_vary_by_trust_level`, `create_notification_suppresses_realtime_push_by_trust_level`, `dispatch_to_channels_suppresses_by_workspace_trust_level`, `mem_persona_approval_reset_with_task_is_atomic`, plus `--test e2e_ralph_loop --test git_integration`); web vitest for the touched surfaces (Inbox 76, Briefing 34, PolicyManager/ABAC and dependent view suites 250) all green after `npm ci`; every mechanical check script either passes (arch, non-atomic-creation, task-commit-attribution, mem-port-contracts, abac-*, migration-*, in-memory-state-stores, ...) or produces output byte-identical to the assignment base (advisory scripts with pre-existing failures, compared sorted base-worktree vs HEAD).

Also this round: restored the inherited task-labeled commit `099f08c4` to this file's `commits:` frontmatter — the checkpoint recovery diff had dropped it, which `check-task-commit-attribution.sh` flagged (commit `e155c490`).

### Test-suite repair round - merge-expecting integration tests honor the trust gradient (commit `09d52669`)

Review repair (round-23 findings): the F5 merge-time supervised-trust hold correctly holds MRs for human approval on Supervised workspaces, but `e2e_ralph_loop::full_ralph_loop_via_gyre` and `git_integration::merge_queue_auto_merges_mr_and_commit_on_main` created their workspaces without `trust_level` (default Supervised) while still expecting autonomous queue merges, leaving the tree red.

- Both tests exercise loop/queue mechanics, not the trust gradient, so their workspaces are now created with `trust_level: "Autonomous"` (e2e workspace body; git_integration `create_repo` helper), which seeds no trust hold policies and restores the intended autonomous-merge expectation. No other integration test expects a completed merge (verified by grep over `tests/`); trust-hold behavior itself stays covered by the merge_processor lib suite.
- A merge of the assignment base `1d9547de` was resolved first (three semantically identical Admin-gate conflicts in `api/meta_specs.rs`, both sides implementing the same authorization; HEAD's comments kept).

Test evidence (this round): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test e2e_ralph_loop` - 1 passed; `--test git_integration` - 16 passed, 1 pre-existing ignored; `--lib` - 1321 passed, 0 failed (includes all F5/F6/F9 trust-gate tests); `cargo fmt --all -- --check` clean; `check-task-commit-attribution.sh` green with `09d52669` recorded in this file's frontmatter. Evidence: `/tmp/stage/review-evidence/probe-e2e-ralph-loop-fixed.log`, `probe-git-integration-fixed.log`, `probe-full-lib-after-repair.log`.

### Feature as shipped (recap; full round history in the pipeline task record)

Trust gradient per HSI SS9-13: `TrustLevel` enum (`Supervised`/`Guided`/`Autonomous`/`Custom`, default Supervised) on the Workspace entity with DB migration; trust preset policy sets (`trust:` prefix, priority 100-199; `trust_policies_for_level`); atomic trust transitions via `PUT /api/v1/workspaces/:id` (single transaction, 409 on rollback, ABAC cache invalidated after commit); `builtin:require-human-spec-approval` seeded immutable at priority 999; immutable-Deny-first ABAC evaluation; policy CRUD rejects `trust:`/`builtin:` prefixes (400); merge-time ABAC enforcement for the merge processor's service identity (F5) with human-approval escape `mr.status == Approved`, selection-loop + single-entry + atomic-group coverage, and fail-closed semantics on policy-store read failures (F9); fail-closed interrogation policy creation (F6); Custom transition directions and field-level generator assertions tested (F7/F8).

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
