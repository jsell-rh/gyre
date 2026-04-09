# TASK-019: Spec Links — Merge Gates, Query Endpoints, and Cycle Detection

**Spec reference:** `spec-links.md` §Mechanical Gates (Merge Gates), §Querying the Graph, §Cycle Detection  
**Depends on:** TASK-016 (staleness job and approval gates must be operational)  
**Progress:** `needs-revision`  
**Review:** [specs/reviews/task-019.md](../reviews/task-019.md) — R2, 1 finding

## Spec Excerpt

### Merge Gates (spec-links.md §Mechanical Gates)

> When an MR references a spec (`spec_ref`), the forge also checks that spec's links:
>
> - If the spec `depends_on` another spec that isn't implemented yet, the MR gets a warning (not a block — the code might be valid, but the dependency isn't satisfied yet)
> - If the spec is `superseded`, the MR is rejected: "spec {path} has been superseded by {new_path}. Update your spec_ref."
> - If the spec has a `conflicts_with` link to an approved spec, the MR is blocked until the conflict is resolved

### Query Endpoints (spec-links.md §Querying the Graph)

> | Endpoint | Method | Purpose |
> |---|---|---|
> | `GET /api/v1/specs/{path}/dependents` | GET | "What specs depend on this one?" (impact analysis) |
> | `GET /api/v1/specs/{path}/dependencies` | GET | "What does this spec depend on?" |
> | `GET /api/v1/specs/stale-links` | GET | All stale links across the tenant |
> | `GET /api/v1/specs/conflicts` | GET | All active conflicts |

Note: `GET /api/v1/specs/graph` and `GET /api/v1/specs/{path}/links` already exist.

### Cycle Detection (spec-links.md §Cycle Detection)

> The forge rejects manifest changes that would create cycles in the spec graph:
> - A `depends_on` B `depends_on` C `depends_on` A → rejected
> - A `implements` B `implements` A → rejected
> - `conflicts_with` is bidirectional by nature (not a cycle)
> - `references` and `supersedes` are excluded from cycle detection

## Current State

**Merge gates:** The merge queue handler (`crates/gyre-server/src/api/merge_queue.rs`) does NOT check the referenced spec's links. An MR can reference a superseded spec and still merge.

**Query endpoints:** Only `GET /api/v1/specs/graph` and `GET /api/v1/specs/:path/links` exist. The four convenience endpoints (`dependents`, `dependencies`, `stale-links`, `conflicts`) are not registered in `mod.rs`.

**Cycle detection:** Cycle detection exists for MR dependencies (`merge_deps.rs`) but NOT for spec links. A manifest push with circular `depends_on` links is accepted without error.

## Implementation Plan

1. **Merge gate enforcement** — In the merge queue pre-merge check (or the merge processor):
   - When processing an MR with a `spec_ref`, query `spec_links` for that spec
   - If the spec has a `supersedes` inbound link (meaning it IS superseded by another spec), reject the merge: "spec {path} has been superseded by {new_path}. Update your spec_ref."
   - If the spec has a `conflicts_with` link to an approved spec, block the merge
   - If the spec has `depends_on` links where the target spec's implementation is incomplete, add a warning to the MR timeline (not a block)

2. **Query endpoints** — Add four new routes in `api/mod.rs`:
   - `GET /api/v1/specs/:path/dependents` — query `spec_links` for inbound `depends_on` and `implements` links targeting this spec. ABAC: per-handler auth (spec path, not UUID). Response: `[{source_repo_id, source_path, link_type, status}]`
   - `GET /api/v1/specs/:path/dependencies` — query `spec_links` for outbound `depends_on` and `implements` links from this spec. Same auth/response pattern.
   - `GET /api/v1/specs/stale-links` — query `spec_links` for all links with `status = "stale"`. ABAC: per-handler auth (tenant-scoped). Response: `[{source_path, target_path, link_type, stale_since}]`
   - `GET /api/v1/specs/conflicts` — query `spec_links` for all links with `status = "conflicted"` or `link_type = "conflicts_with"` where both specs are approved. Same auth pattern.

3. **Cycle detection on manifest push** — In the push-time spec registry hook (`spec_registry.rs`):
   - After parsing the manifest, build a directed graph of `depends_on` and `implements` links
   - Run DFS cycle detection (same algorithm as `merge_deps.rs`)
   - If a cycle is found, reject the push with a clear error: "spec link cycle detected: A → B → C → A"
   - Exclude `conflicts_with`, `references`, and `supersedes` from cycle detection

4. **Add tests:**
   - MR referencing superseded spec → rejected at merge time
   - MR referencing spec with unresolved conflicts_with → blocked
   - MR referencing spec with unimplemented depends_on → warning (not block)
   - Each query endpoint returns correct results
   - Manifest push creating cycle → rejected
   - Manifest push without cycle → accepted

## Acceptance Criteria

- [ ] Merge processor rejects MRs referencing superseded specs
- [ ] Merge processor blocks MRs referencing specs with active `conflicts_with`
- [ ] Merge processor warns (does not block) on unimplemented `depends_on`
- [ ] `GET /api/v1/specs/:path/dependents` returns specs that depend on the given spec
- [ ] `GET /api/v1/specs/:path/dependencies` returns specs the given spec depends on
- [ ] `GET /api/v1/specs/stale-links` returns all stale links
- [ ] `GET /api/v1/specs/conflicts` returns all active conflicts
- [ ] Manifest push with spec link cycle is rejected
- [ ] Tests cover all merge gates, query endpoints, and cycle detection
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `specs/system/spec-links.md` §Mechanical Gates (Merge Gates), §Querying the Graph, §Cycle Detection
3. Read `crates/gyre-server/src/api/merge_queue.rs` for the merge processor
4. Read `crates/gyre-server/src/api/specs.rs` for existing spec link handling patterns
5. Read `crates/gyre-server/src/spec_registry.rs` for `SpecLinkEntry`, `SpecLinksStore`
6. Read `crates/gyre-server/src/api/merge_deps.rs` for the DFS cycle detection pattern to reuse
7. Verify endpoint URLs against `crates/gyre-server/src/api/mod.rs` route registration before implementing
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(spec-links): implement merge gates, query endpoints, and cycle detection (TASK-019)`
- `fix(process): add wildcard-exclusion and bidirectional-filter guards (TASK-019)`
- `fix(spec-links): add extends to cycle detection, bidirectional conflicts_with merge gate (TASK-019)`
