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
commits: ["09d5266952f214c2e38911c4bcd548c4c79c0f68", "f4f32a01a8891b67010600b6af3857c9a4be695d", "accffacfbfd7cab9f6913964c0fe8da7e667748d", "dddcb3a4e4e36417e99cee451670d72ce5e72a45", "4e17ffa5c94b84b1b8c9f2890a94997424932a58", "5b5e7c65957c79b12111ae987d71f7752abac702", "3372f2c3e498d1968bd3ef97cd05853280807ccb", "1b07c83be8904c63ad97e8b5d59abf50b8f59878", "ed36c1b6832b0242340a5de9a623d49fcb5a50d5", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0", "099f08c432e5bd428dbf2d0f36b9f31832ba54c3"]
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

### Requirements-changed re-verification round (commit `099f08c4`)

Repair category `requirements_changed`: the assignment base moved to `078ec12a` (task-123 notification taxonomy unification + tasks 084/085/090/208 landings). The retained source checkpoint `fde1cde5` was merged onto the new base at branch head (merge `3f7cbee3`). Verification result: **main had already absorbed the complete task-077 implementation** — all 14 attributed commits are ancestors of the new base, so the merge resolved to a tree byte-identical to the base (`5f6ae70`). The current contract's acceptance criteria were re-verified present and green on the merged tree:

- `TrustLevel` enum (Supervised/Guided/Autonomous/Custom, default Supervised) on the Workspace entity; `trust_level` DB default corrected to `Supervised` (migration `000050`); `immutable` on policies (migration `000028`); `trust_policies_for_level` preset sets; atomic `apply_trust_transition` (single DB transaction, 409 on rollback, ABAC cache invalidated after commit); immutable-Deny-first ABAC evaluation (`policy_engine.rs` Step 1); `builtin:require-human-spec-approval` seeded immutable at priority 999; policy CRUD rejects `trust:`/`builtin:` prefixes (400); `PUT /api/v1/workspaces/:id` accepts `trust_level` and applies the transition; merge-time ABAC enforcement for the merge-processor service identity (`subject.type "system"`, `subject.id "merge-processor"`) with the `mr.status == Approved` human-approval escape.
- The only repair needed: one inherited rustfmt violation (stray blank line in `crates/gyre-ports/src/notification.rs`, task-123 surface in the new base) removed in commit `099f08c4` so the task's `cargo fmt --all` clean criterion holds. Whitespace-only; no behavior change.

### Test evidence (this round, on the merged tree + fmt fix)

- `SKIP_WEB_BUILD=1 cargo build --all` — full workspace compiles.
- `cargo test -p gyre-domain --lib` — 463 passed, 0 failed.
- `cargo test -p gyre-server --lib` — 1680 passed, 0 failed (includes all trust-gate, workspaces trust-transition, merge-processor hold, and 409-path tests); re-run of `api::workspaces` after the fmt fix — 19 passed.
- `cargo test -p gyre-server --test e2e_ralph_loop --test git_integration` — 17 passed, 0 failed.
- `cargo fmt --all -- --check` — clean.
- Full web suite (via `node ./node_modules/vitest/vitest.mjs run --no-file-parallelism --maxWorkers=1` after `npm ci`): 99 files, 2010 passed, 41 pre-existing skips, 0 failures. The repo's `check-web-tests.sh` wrapper (`npm test -- --maxWorkers=1`) fails in this sandbox only because npm 11 exports the `--`-separated flag as `npm_config_maxWorkers`, which vitest's cac merges with argv into a duplicate-option error — an invocation artifact, not a code defect; the direct vitest run above is the evidence.
- Mechanical checks: every check script that runs in this sandbox produced output identical to the new base (compared sorted, base worktree vs HEAD — `probe-check-parity-base-vs-head.log`); no new violations from the merge. The exit-1 failures that exist (cli-spec-parity, dead-components, mcp-wrapper-parity, etc.) are pre-existing on main and owned by other tasks, unchanged by this branch.
- Infra restriction for remote verification: this sandbox provides mawk only (no gawk, no root to install), so 13 gawk-syntax check scripts (notification-priority, tenant-filter, path-scope-binding, etc.) abort locally with awk syntax errors — exit 2 both on HEAD and on the base worktree. These run in CI on gawk hosts; the identical-to-base tree identity plus this record cover them.
- Evidence: `/tmp/stage/review-evidence/probe-mechanical-checks.log`, `probe-check-parity-base-vs-head.log`, `probe-integration-current.log`, `probe-web-suite.log`.

### Feature as shipped (recap; full round history in the pipeline task record)

Trust gradient per HSI SS9-13: `TrustLevel` enum on the Workspace entity with DB migrations; trust preset policy sets (`trust:` prefix, priority 100-199); atomic trust transitions via `PUT /api/v1/workspaces/:id`; `builtin:require-human-spec-approval` immutable at priority 999; immutable-Deny-first ABAC evaluation; policy CRUD prefix guard (400); merge-time ABAC enforcement for the merge processor's service identity with human-approval escape, selection-loop + single-entry + atomic-group coverage, and fail-closed semantics on policy-store read failures; fail-closed interrogation policy creation; Custom transition directions and field-level generator assertions tested.

Full-workspace suites, all-target Clippy, and GitHub CI remain owned by verification/publication per the assignment.
