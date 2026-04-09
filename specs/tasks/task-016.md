# TASK-016: Spec Links — Staleness Job & Approval Gate Enforcement

**Spec reference:** `spec-links.md` §Link Types, §Forge-Maintained Spec Graph  
**Depends on:** None (spec links store and approval flow already exist)  
**Progress:** `needs-revision`
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

## Implementation Plan

1. **Add `DependsOn` approval gate** in `approve_spec` handler:
   - Before approving, check if spec has `depends_on` links
   - For each `depends_on` target, verify the target spec's implementation is complete (check for merged MRs referencing that spec, or an approval status indicating implementation is done)
   - If any dependency's implementation is incomplete, reject with a clear message

2. **Add `Supersedes` approval side-effect** in `approve_spec` handler:
   - When a spec with `supersedes` links is approved, mark the target spec as `Deprecated` in the ledger
   - Emit a Broadcast-tier event for the deprecation

3. **Add `Extends` drift-review trigger** in the push-time spec registry hook:
   - When a spec changes (new SHA on push), check if any other specs have `extends` links pointing to it
   - For each extending spec, update the link health to `"stale"` and create a drift-review task

4. **Implement the staleness job** (`cross_workspace_link_staleness_check`):
   - For each `SpecLinkEntry` with a `target_sha`:
     - Resolve the target spec's current SHA (from the spec ledger or git)
     - If the current SHA differs from `target_sha`, mark the link as `"stale"`
   - For cross-workspace links (those with `@` prefix targets), resolve via the tenant-wide spec graph
   - Create priority-5 notifications for workspace Admin/Developer members when links go stale

5. **Add tests:**
   - `DependsOn` blocks approval when dependency implementation is incomplete
   - `Supersedes` marks target spec as Deprecated on approval
   - `Extends` triggers drift-review task on parent spec change
   - Staleness job detects SHA mismatch and marks links stale

## Acceptance Criteria

- [ ] `DependsOn` approval gate rejects when dependency implementation is incomplete
- [ ] `Supersedes` approval marks target spec as Deprecated
- [ ] `Extends` parent spec change triggers drift-review in extending specs
- [ ] Staleness job resolves current SHAs and marks mismatched links as `"stale"`
- [ ] Stale links produce notifications for workspace members
- [ ] Tests cover each link type enforcement
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/specs.rs` — the `approve_spec` handler (lines 375-490)
3. Read `crates/gyre-server/src/spec_registry.rs` — `SpecLinkEntry`, `SpecLinkType`, staleness check
4. Read `crates/gyre-server/src/jobs.rs` — `cross_workspace_link_staleness_check` stub
5. Read `specs/system/spec-links.md` for the full enforcement table
6. Add enforcement cases to the `match &link.link_type` block in `approve_spec`
7. Implement the staleness job body
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(spec-links): implement staleness job & approval gate enforcement (TASK-016)`
