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
commits: ["6009d2b46df2d7aeb432f8aab310f381eefa2ea5", "34c4225083d93e40354afa1fd1be6e45d20aa0d0", "2fea93f03006238a8f85f3e8f0b80cf2fa781603", "71b202e6386a1ee211f9f265e4d531afdf0d411c", "099f08c43f2afdbc53319850d57b6be1445a7ce7", "09d5266952f214c2e38911c4bcd548c4c79c0f68", "f4f32a01a8891b67010600b6af3857c9a4be695d", "accffacfbfd7cab9f6913964c0fe8da7e667748d", "dddcb3a4e4e36417e99cee451670d72ce5e72a45", "4e17ffa5c94b84b1b8c9f2890a94997424932a58", "5b5e7c65957c79b12111ae987d71f7752abac702", "3372f2c3e498d1968bd3ef97cd05853280807ccb", "1b07c83be8904c63ad97e8b5d59abf50b8f59878", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0"]
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

### Verification repair - clippy diff-gate findings on trust-gradient code (commit `6009d2b4`)

The deterministic verification run (`bash /tmp/stage/checks.sh`, exit 1) flagged exactly three Clippy findings on lines changed vs the assignment base `e69fd990`; every other gate in that run (rustfmt diff, architecture, all static checks, web build) passed. All three are repaired:

- **`gyre-domain/src/workspace.rs:92-93` `needless_borrow`:** `GUIDED_SUPPRESSED`/`AUTONOMOUS_SUPPRESSED` are already `&[NotificationType]` consts, so `&CONST` in the `suppressed_notification_types` match arms created a reference immediately dereferenced by coercion. Fixed by binding the consts directly - no behavior change (the full trust/notification-filter suite still passes, including `suppressed_type_strings_round_trip_through_parse` and `inbox_range_excludes_rank_ten_chatter_but_not_action_required_types`, which exercise the edited function).
- **`gyre-ports/src/workspace.rs:48` `double_must_use`:** this branch added `PersonaRepository::apply_approval_reset_with_task`; async_trait generates a `#[must_use]` future per method and reports the diagnostic at the trait's `#[async_trait]` line with a span covering the new method (48-82), so the diff gate saw it on changed lines. Fixed with the file's established `#[allow(clippy::double_must_use)]` pattern (identical to `WorkspaceRepository` five lines above). No exemption files touched; the gate itself is unchanged.
- **Frontmatter attribution repair:** the task body had been synced to a stale pipeline record that dropped four task-labeled branch commits (`34c42250`, `2fea93f0`, `71b202e6`, `099f08c4`) while listing the pre-branch checkpoint `ed36c1b6` - which would have failed `check-task-commit-attribution.sh` (all four remain in branch history and on the product surface). The `commits:` list is regenerated from actual branch history via the executor's `dev-attribution.py` and now records every product-surface commit including this round's `6009d2b4`.

Test evidence (this round): `cargo test -p gyre-domain` - 569 passed, 0 failed (includes the four `trust_policies_for_level_*` tests and the 21-test workspace trust/notification-filter suite); `cargo test -p gyre-ports` - ok; clippy diff-scope probe over both touched files - 0 findings on changed lines; `cargo fmt --all -- --check` clean. Evidence: `/tmp/stage/review-evidence/probe-clippy-repair.log`, `probe-clippy-diff-scope-fixed.json`. Full-workspace suites and remote verification remain owned by the verifier.

### Feature as shipped (recap; full round history in the pipeline task record)

Trust gradient per HSI SS9-13: `TrustLevel` enum (`Supervised`/`Guided`/`Autonomous`/`Custom`, default Supervised) on the Workspace entity with DB migration; trust preset policy sets (`trust:` prefix, priority 100-199; `trust_policies_for_level`); atomic trust transitions via `PUT /api/v1/workspaces/:id` (single transaction, 409 on rollback, ABAC cache invalidated after commit); `builtin:require-human-spec-approval` seeded immutable at priority 999; immutable-Deny-first ABAC evaluation; policy CRUD rejects `trust:`/`builtin:` prefixes (400); merge-time ABAC enforcement for the merge processor's service identity (F5) with human-approval escape `mr.status == Approved`, selection-loop + single-entry + atomic-group coverage, and fail-closed semantics on policy-store read failures (F9); fail-closed interrogation policy creation (F6); Custom transition directions and field-level generator assertions tested (F7/F8); unified server-side trust notification filter for the inbox list and badge count, with no client-side priority cap (F24) and merge-expecting integration tests created at Autonomous trust.

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
