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
commits: ["8a07efa5fd91a527f2291e8a5871c08f88df5f49", "6009d2b46df2d7aeb432f8aab310f381eefa2ea5", "34c4225083d93e40354afa1fd1be6e45d20aa0d0", "2fea93f03006238a8f85f3e8f0b80cf2fa781603", "71b202e6386a1ee211f9f265e4d531afdf0d411c", "099f08c43f2afdbc53319850d57b6be1445a7ce7", "09d5266952f214c2e38911c4bcd548c4c79c0f68", "f4f32a01a8891b67010600b6af3857c9a4be695d", "accffacfbfd7cab9f6913964c0fe8da7e667748d", "dddcb3a4e4e36417e99cee451670d72ce5e72a45", "4e17ffa5c94b84b1b8c9f2890a94997424932a58", "5b5e7c65957c79b12111ae987d71f7752abac702", "3372f2c3e498d1968bd3ef97cd05853280807ccb", "1b07c83be8904c63ad97e8b5d59abf50b8f59878", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0", "951a12dddab0237a7bf7685f345529ac846c4e2d"]
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

### Verification repair — `cargo test --all` findings at `24aaf522` (commit `8a07efa5`)

The verification run (exit 101) failed two gyre-server lib tests. Both are test-scaffold repairs; no enforcement or dispatch behavior changed:

- `api::workspaces::tests::trust_level_change_requires_admin_workspace_role` (got 404, expected 403): the test's Developer/Admin JWTs carried no `tenant_id` claim, so JWT provisioning bound both users to the bootstrapped "default" tenant while the workspace lives in tenant `t1` — the per-handler cross-tenant containment (404: existence not confirmed) fired before the trust gate. The JWTs now carry `"tenant_id": "t1"`, matching the doc-comment scenario (a Developer of the workspace's tenant). Every assertion is unchanged, and the no-membership case now actually reaches the per-handler trust gate it exists to pin.
- `reconciliation::tests::sweep_drift_notification_fans_out_to_configured_channels`: the task-112 emitter-path guard ran on the shared `make_workspace` scaffold (Autonomous), but the trust gradient governs every delivery channel (HSI §2 Notifications row) and suppresses `MetaSpecDrift` at Guided/Autonomous — so the email-outbox assertion exercised trust filtering instead of the dispatch wiring it guards. Added a `make_workspace_at_trust` helper and run this one test at Supervised (every state change delivered on every channel). The assertion is unchanged; trust-based dispatch suppression keeps its own dedicated suite (`notification_dispatcher::dispatch_to_channels_suppresses_by_workspace_trust_level`).

Test evidence (this round): `cargo fmt --all -- --check` clean; `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` clean (6m28s). gyre-server test compilation exceeds this sandbox's 4 GiB budget (build-wrapper refusal recorded), so the verifier must execute: `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib trust_level_change_requires_admin_workspace_role`, `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib sweep_drift_notification_fans_out_to_configured_channels`, and the full `cargo test --all`. Evidence: `/tmp/stage/review-evidence/probe-cargo-check.log`, `probe-test-wrapper-blocked.log`.

### Feature as shipped (recap; full round history in the pipeline task record)

Trust gradient per HSI SS9-13: `TrustLevel` enum (`Supervised`/`Guided`/`Autonomous`/`Custom`, default Supervised) on the Workspace entity with DB migration; trust preset policy sets (`trust:` prefix, priority 100-199; `trust_policies_for_level`); atomic trust transitions via `PUT /api/v1/workspaces/:id` (single transaction, 409 on rollback, ABAC cache invalidated after commit); `builtin:require-human-spec-approval` seeded immutable at priority 999; immutable-Deny-first ABAC evaluation; policy CRUD rejects `trust:`/`builtin:` prefixes (400); merge-time ABAC enforcement for the merge processor's service identity (F5) with human-approval escape `mr.status == Approved`, selection-loop + single-entry + atomic-group coverage, and fail-closed semantics on policy-store read failures (F9); fail-closed interrogation policy creation (F6); Custom transition directions and field-level generator assertions tested (F7/F8).

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
