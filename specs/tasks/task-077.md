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
commits: ["dddcb3a4e4e36417e99cee451670d72ce5e72a45", "4e17ffa5c94b84b1b8c9f2890a94997424932a58", "5b5e7c65957c79b12111ae987d71f7752abac702", "3372f2c3e498d1968bd3ef97cd05853280807ccb", "1b07c83be8904c63ad97e8b5d59abf50b8f59878", "ed36c1b6832b0242340a5de9a623d49fcb5a50d5", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0"]
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

### F9 repair round - supervised-trust merge gate fails CLOSED on policy-store read failure (commit `dddcb3a4`)

Review repair 8f44eb6d (finding F9): `evaluate_merge_abac` degraded `state.policies.list()` errors to an empty policy set via `unwrap_or_default()`, and because `supervised_trust_hold` only holds on an EXPLICIT Deny match, a Supervised workspace's merge proceeded with the error silently discarded - fail OPEN, the same class the repo mechanically bans for refs (`check-fail-open-ref-resolution.sh`). Fixed on both read paths:

- `evaluate_merge_abac` now returns `anyhow::Result<EvalResult>` and PROPAGATES both the policy `list()` error and the sibling workspace `find_by_id` error (the former `Err(_) => None` silently dropped tenant context, which could make tenant-conditioned Denies silently not match), each with a `supervised trust gate: ... store read failed` context string.
- `supervised_trust_hold` - the shared predicate used by the selection loop and both merge paths - fails CLOSED: an evaluation error is warn-logged and mapped to a hold reason ("supervised trust: policy store unavailable for MR ..., failing closed"), matching the HTTP ABAC middleware's fail-closed direction. The hold is a requeue (never a failure), so store recovery unblocks the merge and the human-approval escape (`mr.status == Approved`) still applies.
- `MemPolicyRepository` gains a `fail_lists()` test hook (mirror of the F6 `fail_creates()` hook) and `crate::mem::test_state_failing_policy_lists()`; two integration tests assert fail-closed behavior with a list-failing repo: `policy_store_read_failure_holds_supervised_merge_fail_closed` (Supervised: MR stays Open, entry requeued with a reason naming the fail-closed hold) and `policy_store_read_failure_holds_guided_merge_fail_closed` (Guided: held too - "cannot determine state" is never "state is fine", inverting the fail-open asymmetry the HTTP middleware does not have). Negative probe: reverting the `Err` arm to the old fail-open behavior (`return None`) fails BOTH tests.

Test evidence (this round, on HEAD `dddcb3a4`): `cargo test -p gyre-server --lib merge_processor` - 60 passed (incl. both F9 tests); `api::workspaces` 16, `policy_engine` 19, `create_interrogation_policies` 1 - all passed; `cargo check -p gyre-server --all-targets` clean; `cargo fmt --all` applied; `cargo clippy -p gyre-server --all-targets` has ZERO diagnostics in the two touched files (workspace warnings are pre-existing elsewhere - nothing for changed-lines clippy to flag); mechanical gates arch / fail-open-ref-resolution / inert-enforcement / warn-continue-creation / non-atomic-creation / task-commit-attribution / mem-port-contracts / placeholder-stubs / conditional-test-guards / no-em-dash / stale-enforcement-mode all rc=0, and the four scripts that exit non-zero here (silent-result-discard rc=1; self-confirming / mirrored / dead-test rc=2) produce byte-identical output against the pristine branch HEAD (verified via `git stash`) - pre-existing baseline/environment conditions, not introduced by this change. Evidence: `/tmp/stage/review-evidence/f9-fail-closed-trust-gate.md`.

### Feature as shipped (recap; full round history in the pipeline task record)

Trust gradient per HSI SS9-13: `TrustLevel` enum (`Supervised`/`Guided`/`Autonomous`/`Custom`, default Supervised) on the Workspace entity with DB migration; trust preset policy sets (`trust:` prefix, priority 100-199; `trust_policies_for_level`); atomic trust transitions via `PUT /api/v1/workspaces/:id` (single transaction, 409 on rollback, ABAC cache invalidated after commit); `builtin:require-human-spec-approval` seeded immutable at priority 999; immutable-Deny-first ABAC evaluation; policy CRUD rejects `trust:`/`builtin:` prefixes (400); merge-time ABAC enforcement for the merge processor's service identity (F5) with human-approval escape `mr.status == Approved`, selection-loop + single-entry + atomic-group coverage, and - as of this round - fail-closed semantics on policy-store read failures (F9); fail-closed interrogation policy creation (F6); Custom transition directions and field-level generator assertions tested (F7/F8).

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
