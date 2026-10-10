---
title: "HSI Trust Gradient — Trust Levels, Enforcement & Mechanical Implementation"
spec_ref: "human-system-interface.md §9–13"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-077.md
coverage_sections:
  - "human-system-interface.md §9 2. Trust Gradient"
  - "human-system-interface.md §10 The Problem"
  - "human-system-interface.md §11 Trust Levels"
  - "human-system-interface.md §12 What Each Level Controls"
  - "human-system-interface.md §13 Mechanical Implementation"
commits: ["3372f2c3e498d1968bd3ef97cd05853280807ccb", "1b07c83be8904c63ad97e8b5d59abf50b8f59878", "ed36c1b6832b0242340a5de9a623d49fcb5a50d5", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0"]
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

Revision round (F5–F8 from `specs/reviews/task-077.md` R2), completing the checkpointed implementation:

- **F5 — merge-time ABAC enforcement:** `merge_processor.rs` now evaluates ABAC with the processor's internal service identity (`subject.type "system"`, `subject.id "merge-processor"`) before every single-entry merge and every atomic-group member merge. An explicit Deny match (`matched_policy.is_some()`, e.g. `trust:require-human-mr-review` in a Supervised workspace) HOLDS the merge — single entries are requeued (not failed) with a "supervised trust" reason so the human-approval path stays live; group members roll the whole group back to Queued. The human-approval escape is `mr.status == Approved` (set only via the MR status endpoint, never by the processor before the gate). The pipeline catch-all `builtin-default-deny` is excluded from this evaluation scope: it is an HTTP-pipeline statement, and leaving it in scope would make Guided/Autonomous (the spec's "processor is NOT blocked" state) unrepresentable. Cross-workspace Deny policies are filtered by scope_id.
- **F6 — fail-closed interrogation policy creation:** `create_interrogation_policies` propagates creation errors (`?`) instead of warn-and-continue; the spawn handler rolls back the agent record and token before returning the error, so an interrogation agent can never run with a subset of its restriction policies. Exemption entries for both former discard sites are deleted (`scripts/inert-enforcement-exemptions.txt`, `scripts/warn-continue-creation-exemptions.txt`); both checks run green with zero task-077-owned entries.
- **F7 — Custom transition directions tested:** `trust_transition_preset_to_custom_preserves_trust_policies` (Supervised → Custom preserves `trust:` policies) and `trust_transition_custom_to_preset_deletes_and_reseeds` (Custom → Guided deletes ALL `trust:` policies for the workspace, including operator-created `trust:`-prefixed ones, reseeds nothing for Guided, and a non-trust user policy survives).
- **F8 — field-level generator assertions + `from_db_str` fallback:** `trust_policies_for_level_supervised_generates_merge_hold_deny` asserts every field the F5 gate consumes (name, effect Deny, priority 150 in the 100–199 band, actions `[merge]`, resource_types `[mr]`, `subject.type == "system"` Equals condition, Workspace scope/scope_id, enabled, non-immutable, non-builtin, created_by); Guided/Autonomous/Custom assert empty sets. `from_db_str_parses_all_four_levels` and `from_db_str_unknown_falls_back_to_supervised` cover the changed fallback.
- **CI repair:** `check-task-commit-attribution` failed on inherited main (task-189's landed merge commit `f4acb4eb` missing from its frontmatter) — added the SHA to `specs/tasks/task-189.md` `commits:`; check now green. `cargo fmt --all` drift in the two touched test files fixed.
- **Checkpoint repair (this round, commit `6bbefbc5`):** the interrupted-agent checkpoint `3372f2c3` reverted `scripts/check-non-atomic-creation.sh` from the python3 port (`8adb857b`) back to the gawk-only awk version and deleted its exemptions baseline — the awk version aborts on mawk hosts (rc=2, gawk-only 3-arg `match()`), a red gate regardless of code. Restored the python3 port and re-baselined `scripts/non-atomic-creation-exemptions.txt` at 5 entries; `invite_member` is not re-exempted because the checkpoint genuinely fixed it (notification routed through `notify_rich`, no second `state.<repo>.create()` remains). Negative probe confirmed the restored gate still detects planted violations (rc=1) while passing clean on HEAD (rc=0).

- `cargo test -p gyre-server --lib api::workspaces` — 16 passed (incl. both F7 Custom-direction tests, the 409 rollback test, the invalid-`trust_level` 400 test).
- `cargo test -p gyre-server --lib merge_processor` — 55 passed (incl. `supervised_workspace_open_mr_merge_is_held_and_requeued`, `supervised_workspace_approved_mr_merges`, `guided_workspace_open_mr_merges`, `supervised_trust_denies_atomic_group_member_rolls_back_group`, and the two startup-seeded-builtin regression tests).
- `cargo test -p gyre-server --lib policy_engine` — 19 passed.
- `cargo test -p gyre-server --lib api::spawn::tests::create_interrogation_policies` — 1 passed (F6 fail-closed).
- `cargo test -p gyre-domain --lib policy` — 4 passed; `--lib from_db_str` — 3 passed (F8).
- `cargo fmt --all --check` clean; all 17 `scripts/check-*.sh` mechanical gates pass.

Repair-round verification (post-merge HEAD `696040ee` + `6bbefbc5`; evidence: `/tmp/stage/review-evidence/verifier-restoration.md`): `merge_processor` 55 passed, `api::workspaces` 16, `api::meta_specs` 15 (incl. `registry_endpoints_reject_non_admin` from the checkpoint), `policy_engine` 19, `create_interrogation_policies` 1, `gyre-domain --lib` 371, `api::users` 9; `cargo fmt --all --check` clean; all 8 checkpoint-touched mechanical gates plus the restored `check-non-atomic-creation.sh` pass, with a negative probe confirming the gate still detects planted violations.

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
