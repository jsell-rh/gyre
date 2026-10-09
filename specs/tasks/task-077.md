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
commits: ["fea8f6a52d75163485a61b9623cc0446793f43bd", "2916e2608b241066d038f76feac7885825b4c950", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0"]
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


## Shipped

HSI §2 Trust Gradient is enforced end-to-end at head `d780c343` (recovered
checkpoint source; this round verified it whole, re-anchored one stale
exemption line, and recorded listener-sandbox restrictions — no product-code
changes were needed beyond what the checkpoint already carried).

**Behavior shipped (per acceptance criterion):**

- `TrustLevel` enum (Supervised/Guided/Autonomous/Custom) in `gyre-domain`,
  `trust_level` on `Workspace` defaulting to Supervised; migration
  `2026-09-29-000050` fixes the DB default to `Supervised` (000024 had
  `Guided`, contradicting HSI §2); migration `2026-03-26-000028` adds
  `immutable` to policies (schema + SQLite + PG adapters round-trip it).
- `trust_policies_for_level` generates the preset sets: Supervised → one
  `trust:require-human-mr-review` Deny (priority 150, merge/mr, subject.type
  == "system", workspace-scoped); Guided/Autonomous/Custom → empty (Guided's
  delta is the removal; spec approval stays with the builtin).
- Trust transitions are atomic: `apply_trust_transition` (port method,
  implemented transactionally in SQLite and PG via one `conn.transaction`
  upserting the workspace row + deleting `trust:` policies for the scope +
  inserting the new set; mem adapter mirrors with a fail hook) is used by
  both `create_workspace` (seeds initial preset atomically) and
  `update_workspace` (single call — no double write; 409 with the verbatim
  HSI §2 message on failure). Preset → Custom preserves `trust:` policies;
  Custom → preset deletes and reseeds them; user-created and `builtin:`
  policies survive transitions.
- ABAC engine (`policy_engine::evaluate`) evaluates immutable Deny policies
  FIRST, before priority-based evaluation; they cannot be overridden by any
  Allow regardless of priority. `builtin:require-human-spec-approval`
  (priority 999, immutable, Deny approve/spec for non-user subjects) is
  seeded fail-closed at startup (`seed_builtin_policies`, main.rs:50).
- Policy CRUD rejects `trust:`/`builtin:` name prefixes with 400 on create
  and on caller-side rename (api/policies.rs:81-88, 165-171).
- **Merge-time enforcement (review F5, the central deliverable):** the merge
  processor evaluates real ABAC as an internal service (subject.type
  "system", subject.id "merge-processor" — not the ABAC-bypassing
  GYRE_AUTH_TOKEN identity; hierarchy-enforcement.md §4 records the amended
  bypass rule) with action `merge` on resource `mr`, on both the
  single-entry path and the atomic-group path. On an explicit Deny match the
  merge is held and requeued (not failed) until a human approves; agents are
  forbidden from setting MR status `approved` (transition_mr_status 403), so
  the processor cannot self-satisfy the escape. Guided/Autonomous
  workspaces merge without human approval (no trust Deny exists — engine
  default-deny is not treated as a hold).
- Fail-closed creation (review F6): interrogation-agent policy seeding
  propagates errors and rolls back the agent record + token on failure;
  builtin seeding refuses to serve on partial seeding.
- `trust_level` is in every Workspace API response; the UI ships a trust
  radio group in Workspace Settings (PUT on save). docs/api-reference.md
  documents trust_level, the 409, and the reserved prefixes.

**Test evidence (in-process, this sandbox; commands + exit codes in
`/tmp/stage/review-evidence/task-077-revision-evidence.md`):**

- `cargo test -p gyre-server --lib api::workspaces` — 14 passed (includes
  create-time seeding, 409-on-failed-transition with rollback assertions,
  both Custom transition directions).
- `cargo test -p gyre-server --lib merge_processor` — 55 passed (Supervised
  open MR held + requeued with reason, human-approved MR merges, Guided MR
  merges).
- `cargo test -p gyre-server --lib policy_engine` — 19 passed
  (immutable-Deny-first, priority order, builtin integration).
- `cargo test -p gyre-server --lib api::policies` — 12 passed
  (trust:/builtin: prefix rejection → 400).
- `cargo test -p gyre-server --lib api::spawn` — 30 passed (F6 fail-closed).
- `cargo test -p gyre-server --lib abac_middleware` — 10 passed (seeded
  builtin set).
- `cargo test -p gyre-domain --lib` — 369 passed (field-level generator
  assertions, from_db_str fallback).
- Lints: check-inert-enforcement OK, check-warn-continue-creation OK,
  check-non-atomic-creation OK (after re-anchor, below),
  check-migration-sql-portability OK.
- All 36 changed .rs files pass `rustfmt --check`; full `cargo fmt`/`cargo
  test --all` runs are blocked/hung in this sandbox by the target-dir lock
  and by two listener-dependent tests failing on the documented no-TCP-accept
  restriction (Errno 95) — recorded in
  `/tmp/stage/review-evidence/sandbox-transport-restriction.md`; those gates
  are owned by verification/publication (exact-head CI).

**This round's repair:** re-anchored the pre-existing `admin_seed` exemption
in `scripts/non-atomic-creation-exemptions.txt` from admin.rs:401 → :409
(task-210 a781ede2 inserted 8 lines above the function after the baseline
was pinned; same site, same classification, entry count unchanged —
precedent db7a0073). No new exemptions; no verifier weakening; web/dist
rebuild from the build probe was reverted (task branches don't ship dist).

Remaining for downstream tasks (explicitly out of scope here per the task
list): HSI §2a Policies↔Trust UI integration (task-084), trust suggestions
job (task-085), notification-volume behavior per level (§2 table rows beyond
MR merge/spec approval are owned by their sections' tasks).

## Agent Instructions

Read `specs/system/human-system-interface.md` §9–13 (Trust Gradient) carefully — the mechanical implementation section has precise details about policy naming, priorities, and transaction behavior. Also read `specs/system/abac-policy-engine.md` for the existing ABAC engine design. The key amendment is adding `immutable` flag support to the ABAC evaluation engine. Check `crates/gyre-domain/src/` for existing ABAC evaluation code and `crates/gyre-adapters/migrations/` for migration numbering (currently at 000046+). The workspace entity is in `gyre-common` — grep for `Workspace` struct.
