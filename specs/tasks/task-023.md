# TASK-023: Spec Links — Accountability Agent Patrol

**Spec reference:** `spec-links.md` §Accountability Agent Integration  
**Depends on:** TASK-016 (staleness job), TASK-019 (query endpoints and cycle detection)  
**Progress:** `ready-for-review`  
**Review:** [`specs/reviews/task-023.md`](../reviews/task-023.md)

## Spec Excerpt

From `spec-links.md` §Accountability Agent Integration:

> The Accountability agent's patrol gains spec-graph awareness:
>
> - **Stale links:** flag specs with stale links that haven't been reviewed
> - **Orphaned supersessions:** a spec is superseded but code still references it
> - **Unresolved conflicts:** two conflicting specs are both approved
> - **Dangling implementations:** an `implements` link points to a spec that was deleted
> - **Dangling implementations:** an `implements` link points to a spec that was deleted
> - **Deep dependency chains:** specs with >5 levels of `depends_on` (decomposition smell)

## Current State

- **Accountability persona** exists as a meta-spec (`"accountability"` in `lib.rs` startup seeding)
- **Spec links store** (`SpecLinksStore`) tracks links with health status (`active`, `stale`, `broken`, `conflicted`)
- **Staleness job** is a no-op stub (TASK-016 will implement it)
- **Query endpoints** for stale-links and conflicts do not yet exist (TASK-019 will add them)
- **No accountability patrol logic** exists for spec-graph awareness — the accountability agent has no mechanism to detect or report these conditions

## Implementation Plan

1. **Patrol endpoint** — `POST /api/v1/patrol/spec-links`:
   - Runs all 5 spec-graph checks and returns findings
   - Can be triggered by the accountability agent or a background job
   - Response: `{ findings: [{ type, severity, spec_path, detail, suggested_action }] }`

2. **Stale link detection** — Check `SpecLinksStore` for links with `health = "stale"`:
   - Filter for links stale longer than a configurable threshold (default: 7 days)
   - Each finding: `{ type: "stale_link", severity: "warning", spec_path, detail: "Stale since {date}, linked to {target}" }`

3. **Orphaned supersession detection** — For each `supersedes` link where the target is deprecated:
   - Grep the codebase (or knowledge graph) for references to the superseded spec path
   - If references found: `{ type: "orphaned_supersession", severity: "warning", spec_path, detail: "Code still references superseded spec {old_path}" }`

4. **Unresolved conflict detection** — Check for `conflicts_with` links where both specs are approved:
   - Query spec ledger for approval status of both sides
   - If both approved: `{ type: "unresolved_conflict", severity: "error", spec_path, detail: "Conflicts with {other_path}, both approved" }`

5. **Dangling implementation detection** — For each `implements` link:
   - Check if the target spec still exists in the manifest
   - If not: `{ type: "dangling_implementation", severity: "error", spec_path, detail: "Implements {deleted_path} which no longer exists" }`

6. **Deep dependency chain detection** — Walk `depends_on` chains:
   - Use DFS/BFS to compute chain depth for each spec
   - If depth > 5: `{ type: "deep_dependency_chain", severity: "info", spec_path, detail: "Dependency chain depth: {depth}" }`

7. **Notification integration** — For error-severity findings:
   - Create priority-3 notifications for workspace Admin/Developer members
   - Include suggested remediation action

8. **Add tests:**
   - Each finding type detected correctly
   - Threshold-based filtering works
   - Notification created for error findings
   - Empty findings when no issues exist

## Acceptance Criteria

- [ ] Patrol endpoint runs all 5 spec-graph checks
- [ ] Stale links flagged when stale beyond threshold
- [ ] Orphaned supersessions detected (code references superseded specs)
- [ ] Unresolved conflicts detected (both specs approved)
- [ ] Dangling implementations detected (target spec deleted)
- [ ] Deep dependency chains flagged (depth > 5)
- [ ] Error-severity findings create notifications
- [ ] Tests cover each finding type
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/spec-links.md` §Accountability Agent Integration
3. Read `specs/personas/accountability.md` for the accountability agent's role
4. Read `crates/gyre-server/src/spec_registry.rs` for `SpecLinksStore`, `SpecLinkEntry`, link types
5. Read `crates/gyre-server/src/api/specs.rs` for spec ledger queries (approval status)
6. Read `crates/gyre-server/src/jobs.rs` for background job pattern
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(spec-links): implement accountability agent patrol (TASK-023)`
- `fix(spec-patrol): address R1 findings — longest-path algorithm and notification dedup (TASK-023)`
