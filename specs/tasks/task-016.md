# TASK-016: Spec Links — Staleness Job & Approval Gate Enforcement

**Spec reference:** `spec-links.md` §Link Types, §Forge-Maintained Spec Graph  
**Depends on:** None (spec links store and approval flow already exist)  
**Progress:** `ready-for-review`
**Review:** [`specs/reviews/task-016.md`](../reviews/task-016.md)

## Spec Excerpt

From `spec-links.md` §Link Types:

> | Link Type | Meaning | Forge Enforcement |
> |---|---|---|
> | `implements` | This spec is a concrete plan for a higher-level spec | Implementation spec can't be approved until parent is approved |
> | `supersedes` | This spec replaces another spec | Old spec marked deprecated in registry. Code referencing old spec gets flagged. |
> | `depends_on` | This spec can't be implemented until the linked spec is implemented | Implementation tasks blocked until dependency spec's implementation is complete |
> | `conflicts_with` | These specs have overlapping or contradictory requirements | Neither spec can be approved simultaneously. Conflict must be resolved first. |
> | `extends` | This spec adds requirements on top of the linked spec | Changes to the parent spec trigger drift-review in the extending spec |

From `spec-links.md` §Forge-Maintained Spec Graph:

> When a spec changes, forge queries `spec_links` and marks links as `stale`. Creates drift-review tasks in source specs.

## Current State

**Approval flow** (`crates/gyre-server/src/api/specs.rs:413-453`): The `approve_spec` handler enforces two link types:
- `Implements` — parent must be approved ✓
- `ConflictsWith` — conflicting spec must not be approved ✓
- All other link types (`_ => {}`) — **NO enforcement** ✗

**Staleness job** (`crates/gyre-server/src/jobs.rs:316-331`): The `cross_workspace_link_staleness_check` job is registered but is a **no-op stub**:
```rust
tracing::debug!("cross_workspace_link_staleness_check: stub, no-op");
```

**SpecLinkEntry** (`crates/gyre-server/src/spec_registry.rs`): The `health` field exists with values `"active" | "stale" | "broken" | "conflicted" | "unresolved"`, but nothing sets it to `"stale"` automatically.

## Revision History

Full review history at [`specs/reviews/task-016.md`](../reviews/task-016.md). Currently at **R4**.

### R1 (4 findings — all addressed)

- **F1:** Push-time inbound staleness detection missing — fixed
- **F2:** Drift-review requires Task entity, not just drift_status flag — fixed
- **F3:** `extends` link target change must invalidate `approval_status` to `Pending` — fixed
- **F4:** No test for `extends` push-time behavior — fixed

### R2 (3 findings — all addressed)

- **F5:** Inbound extends drift-review tasks assigned to wrong repo/workspace — fixed
- **F6:** Duplicate drift-review task creation for same-repo extends links — fixed
- **F7:** Drift-review task creation limited to `extends` links, spec requires all types — fixed

### R3 (1 finding — addressed)

- **F8:** Drift-review tasks created for `references` links, but spec says "No mechanical enforcement" for references — fixed

### R4 (1 finding — OPEN)

- **F9:** Premature `Supersedes` deprecation at sync/push time. The `sync_spec_ledger` function deprecates the target spec unconditionally when processing a `supersedes` link, but the spec says deprecation should only happen **when the superseding spec is approved** (§Approval Gates). The correct logic already exists in `approve_spec`. The sync-time deprecation is a duplicate, premature trigger.

## Implementation Plan

1. **Add `DependsOn` approval gate** in `approve_spec` handler:
   - Before approving, check if spec has `depends_on` links
   - For each `depends_on` target, verify the target spec's implementation is complete (check for merged MRs referencing that spec, or an approval status indicating implementation is done)
   - If any dependency's implementation is incomplete, reject with a clear message

2. **Add `Supersedes` approval side-effect** in `approve_spec` handler:
   - When a spec with `supersedes` links is approved, mark the target spec as `Deprecated` in the ledger
   - Emit a Broadcast-tier event for the deprecation

3. **Push-time inbound staleness detection** in `sync_spec_ledger`:
   - After detecting which specs have changed SHAs (step 4 of the sync, around line 353), scan the links store for ALL links where `target_path` matches any spec whose SHA changed
   - For each matching link, mark it as `"stale"` with `stale_since = now` — this applies to ALL link types (`implements`, `depends_on`, `supersedes`, `conflicts_with`, `extends`), not just `extends`
   - **For `extends` links specifically:**
     - Set `drift_status = "drifted"` on the extending spec's ledger entry
     - **Invalidate the extending spec's approval:** set `approval_status = ApprovalStatus::Pending` (same pattern as the auto-invalidation at line 362-364 when a spec's own content changes)
     - **Create a drift-review Task entity** (not just a drift_status flag) in the extending spec's repo, with actionable context: which parent spec changed, old SHA → new SHA, what the extending spec needs to review

4. **Implement the staleness job** (`cross_workspace_link_staleness_check`):
   - For each `SpecLinkEntry` with a `target_sha`:
     - Resolve the target spec's current SHA (from the spec ledger or git)
     - If the current SHA differs from `target_sha`, mark the link as `"stale"`
   - For cross-workspace links (those with `@` prefix targets), resolve via the tenant-wide spec graph
   - Create priority-5 notifications for workspace Admin/Developer members when links go stale

5. **Add tests:**
   - `DependsOn` blocks approval when dependency implementation is incomplete
   - `Supersedes` marks target spec as Deprecated on approval
   - Staleness job detects SHA mismatch and marks links stale
   - **`Extends` push-time behavior:** call `sync_spec_ledger` with a manifest containing an `extends` link whose `target_sha` differs from the ledger's current SHA, and assert: (a) the link is marked `"stale"`, (b) the extending spec's `drift_status` is `"drifted"`, (c) the extending spec's `approval_status` is invalidated to `Pending`, (d) a drift-review Task entity is created
   - **Inbound staleness for non-extends links:** push a spec change and assert that existing `depends_on`/`implements` links pointing to the changed spec are marked `"stale"` at push time (not waiting for the daily job)

## Acceptance Criteria

- [ ] `DependsOn` approval gate rejects when dependency implementation is incomplete
- [ ] `Supersedes` approval marks target spec as Deprecated
- [ ] Push-time inbound staleness: when a spec's SHA changes, ALL links targeting that spec are marked `"stale"` immediately (not deferred to the daily job)
- [ ] `Extends` parent spec change invalidates extending spec's `approval_status` to `Pending`
- [ ] `Extends` parent spec change creates a drift-review **Task entity** (not just a `drift_status` flag)
- [ ] Staleness job resolves current SHAs and marks mismatched links as `"stale"`
- [ ] Stale links produce notifications for workspace members
- [ ] Test: `extends` push-time behavior (stale link + drift_status + approval invalidation + task creation)
- [ ] Test: inbound staleness for non-extends link types at push time
- [ ] `Supersedes` deprecation only triggers at approval time (not at push/sync time) — F9
- [ ] Tests cover each approval gate link type
- [ ] `cargo test --all` passes

## Agent Instructions

This task is in `needs-revision` state at **R4** with **1 open finding (F9)**. Read the review at `specs/reviews/task-016.md` FIRST, focusing on F9.

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/reviews/task-016.md` — the R4 review. Findings F1–F8 are resolved; F9 is open.
3. Read `crates/gyre-server/src/spec_registry.rs` — the `sync_spec_ledger` function, specifically the `SpecLinkType::Supersedes` match arm (around line 552-566). This is the premature deprecation that F9 identifies.
4. Read `crates/gyre-server/src/api/specs.rs` — the `approve_spec` handler (lines 697-750) where the **correct** Supersedes deprecation logic already lives.
5. Read `specs/system/spec-links.md` §Approval Gates — confirms deprecation is an approval-time side-effect, not a push-time side-effect.
6. Fix F9: Remove the `SpecLinkType::Supersedes` match arm from `sync_spec_ledger` step 6. The `approve_spec` handler already handles deprecation correctly at approval time.
7. Update or remove the `sync_supersedes_marks_target_deprecated` test (specs.rs:2601-2688) since it tests the premature behavior.
8. Add a test confirming that `sync_spec_ledger` does NOT deprecate the target when a `supersedes` link is processed — only `approve_spec` should trigger deprecation.
9. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(spec-links): implement staleness job & approval gate enforcement (TASK-016)`
- `fix(spec-links): address R1 findings F1-F4 — inbound staleness, drift-review tasks, approval invalidation (TASK-016)`
- `fix(spec-links): address R2 findings F5-F7 — repo assignment, dedup, all-type tasks (TASK-016)`
- `fix(spec-links): exclude references links from drift-review task creation (TASK-016)`
