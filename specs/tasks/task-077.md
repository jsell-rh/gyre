---
title: "HSI Trust Gradient — Trust Levels, Enforcement & Mechanical Implementation"
spec_ref: "human-system-interface.md §9–13"
depends_on: []
progress: not-started
coverage_sections:
  - "human-system-interface.md §9 2. Trust Gradient"
  - "human-system-interface.md §10 The Problem"
  - "human-system-interface.md §11 Trust Levels"
  - "human-system-interface.md §12 What Each Level Controls"
  - "human-system-interface.md §13 Mechanical Implementation"
commits: []
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
