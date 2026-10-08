---
title: "HSI Trust Gradient — Trust Levels, Enforcement & Mechanical Implementation"
spec_ref: "human-system-interface.md §9–13"
depends_on: []
progress: needs-revision
review: specs/reviews/task-077.md
coverage_sections:
  - "human-system-interface.md §9 2. Trust Gradient"
  - "human-system-interface.md §10 The Problem"
  - "human-system-interface.md §11 Trust Levels"
  - "human-system-interface.md §12 What Each Level Controls"
  - "human-system-interface.md §13 Mechanical Implementation"
commits: ["9dba5a28fd8447d646346f1dcbd9aafe882ab148", "0000d0eb2eafe1d5203c8dfe26ebcb8d4563cdf9", "6e9257b2ec906fae215b28bf6c190bedd3a23d4a", "d93cde7f2e230151edfb2b0dcc754303757e573c", "af1ff00975bbc994cb67cac10916a3c8bb45d807", "1375c92cc907565b9bf32a0ea1018d033c94060e", "1fb4d8c3bb2e13d4d10e9f4cf0836feb6c6a0189", "8eba770746f6f2d673fb199b171c616c350b2227", "2db3f1efe11140eadb6dbe704eda86ba94d2e1b0", "85830fa44abb6bb2305318f46a4cad4c20e00e33", "4cd20f3b4c25b260443ad5a3814e974c038d2e4a", "7d0019ae23a5fbe336b29b2cbe4c317be4cb394c", "545e986f231d1ceed0a5e537ec156c78a9d48d79", "ed2dd6e7312b5c7aec20778b4cc71e1ffed7e374", "5e32b615f6786841da3819888d5cd6d5643416ab", "c70fcfab2dd9c75549ccc55f52d584281bb3e9eb", "81cab405e273c361215e83183645dc395bb37366", "b4a5f7ba88adb28d5e4937d7a694c5e415dd8424", "d055db5c2a61c26d4f632ca459bd4502e0e90eed", "b8ec108ff189ec1e23baa27872f1b322fcf859fa", "2174cdf352111d8b0f79804f5fc6d6cc687467f3", "5c831a8d3dbdae54d4dd5708d9f9f03606b8d244"]
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

- [x] `TrustLevel` enum exists in `gyre-common` (see R4 note 1 — lives in `gyre-domain` with the Workspace entity)
- [x] Workspace entity has `trust_level` field, default `Supervised`
- [x] DB migration adds `trust_level` to workspaces, `immutable` to policies
- [x] Trust preset policy sets defined for Supervised, Guided, Autonomous
- [x] Trust transitions run in a single DB transaction (atomic)
- [x] ABAC engine evaluates immutable Deny policies first
- [x] `builtin:require-human-spec-approval` seeded at startup
- [x] Policy CRUD rejects `trust:` and `builtin:` prefixes (400)
- [x] `PUT /api/v1/workspaces/:id` accepts `trust_level`, applies transition
- [x] 409 returned on failed trust transition
- [x] ABAC cache invalidated after trust transition commit (see R4 note 2)
- [x] Unit tests for trust policy generation and transition logic
- [x] Integration test: change trust level → verify policies created/deleted
- [x] `cargo test --all` passes, `cargo fmt --all` clean

## R4 Revision Notes (2026-10-06, addresses R2 F5–F8)

**F5 (merge-time enforcement):** `merge_processor.rs` now evaluates ABAC before every
merge with the specced identity — `subject.type: "system"`, `subject.id: "merge-processor"`,
`action: "merge"`, `resource_type: "mr"` (`merge_processor.rs:2626-2628`). On Deny
(i.e. a Supervised workspace's `trust:require-human-mr-review`) the entry is HELD
(requeued with reason, not failed) until a human sets `Approved` via the MR status
endpoint; the endpoint rejects agent subjects (403) and the processor's own
`Open → Approved` transition happens only after the gate, so it cannot self-satisfy.
Covered by `merge_processor.rs` trust-gate tests (hold on Supervised, proceed after
human approval) and the endpoint guard tests (human 200 / agent 403 + MR stays Open).
`system-full-access` matches by `subject.id == "gyre-system-token"`, not type
(policy_engine test `system_full_access_matches_by_id_not_type`);
`hierarchy-enforcement.md` §4 amended to match (identity bypass).
`scripts/inert-enforcement-exemptions.txt` merge_processor entries deleted.

**F6 (fail-closed restriction creation):** `create_interrogation_policies_in`
(spawn.rs:252-263) propagates the first create error; the spawn handler rolls back
the agent record + token on failure (spawn.rs:493-497). `cleanup_interrogation_policies`
keeps the kv id record on partial delete failure so the next pass retries
(spawn.rs:286-310). `scripts/warn-continue-creation-exemptions.txt` is now empty.

**F7 (Custom transition directions):** `trust_transition_preset_to_custom_preserves_trust_policies`
and the Custom → Guided test (workspaces.rs:942-1104) assert preservation and
delete+reseed respectively, including that a non-trust user policy survives.

**F8 (field-level generator tests):** `gyre-domain/src/policy.rs` test module asserts
effect/priority(150, band 100-199)/actions/resource_types/`subject.type == "system"`
condition per level; `workspace.rs` covers `from_db_str` four arms + unknown →
Supervised fallback. `cargo test -p gyre-domain --lib`: 369 passed.

**Note 1 (AC wording):** the plan text said `gyre-common`, but the `Workspace` entity
itself lives in `gyre-domain` (the plan's "workspace entity is in gyre-common" is
factually wrong for this repo); `TrustLevel` is colocated with its entity and
re-exported through the domain crate. HSI §2 mandates the field, not the crate.

**Note 2 (AC 11):** there is no ABAC policy-result cache (only JWKS/graph/dep-staleness
caches). Every evaluation loads `state.policies.list()` fresh
(abac_middleware.rs:836-842), and the transition commits through the same store in one
transaction, so a transition is visible on the next request — the spec's
"invalidate after commit" requirement holds vacuously; documented at the load site.

**Note 3 (attribution tooling):** the `commits:` list had dropped the five R1 revision
SHAs twice. Root cause: `scripts/dev-remote.sh` rebuilt the field from
`origin/main..HEAD` only — R1 SHAs merged to main fall outside that range, and
`check-task-commit-attribution.sh` scans ALL history. Fixed by unioning existing
frontmatter entries with the branch-range list (verified idempotent).

## Agent Instructions

Read `specs/system/human-system-interface.md` §9–13 (Trust Gradient) carefully — the mechanical implementation section has precise details about policy naming, priorities, and transaction behavior. Also read `specs/system/abac-policy-engine.md` for the existing ABAC engine design. The key amendment is adding `immutable` flag support to the ABAC evaluation engine. Check `crates/gyre-domain/src/` for existing ABAC evaluation code and `crates/gyre-adapters/migrations/` for migration numbering (currently at 000046+). The workspace entity is in `gyre-common` — grep for `Workspace` struct.
