---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
progress: needs-revision
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["d876f8883cc4d7d374786a6b56255a9549af75bc", "f29a3464eaca73ac6e932dfd95da6522a10e438e", "ceb62097a1dac7012bb7156add686b54a75f8c25", "a4d0980a972f0a268cb5406b467077d416100589", "affd4788cb6c8aa825c0d5e8a0e1d0c503d5b1d6", "1f092d62dc5680fb49bd02f960b9c79ced0c33c5"]
---

## Spec Excerpt

### §2 Invariant Enforcement (hierarchy-enforcement.md)

`scripts/check-hierarchy.sh` scans domain struct definitions for `Option<Id>` on hierarchy fields and fails if any are found. The script is gated by `GYRE_CHECK_HIERARCHY=1` env var.

```bash
REQUIRED_FIELDS=(
    "workspace_id"  # on Task, Agent, MergeRequest, Repository
    "tenant_id"     # on Workspace
)
```

### §3 Enforcement (hierarchy-enforcement.md)

`scripts/check-tenant-filter.sh` scans all Diesel query methods in `crates/gyre-adapters/src/sqlite/` and `crates/gyre-adapters/src/pg/` for the pattern `.filter(table::tenant_id.eq(`. Any query method that builds a Diesel query without this filter fails the check.

### §7 New Scripts (hierarchy-enforcement.md)

| Script | What it checks | Run by |
|---|---|---|
| `scripts/check-api-auth.sh` | Every route (GET/POST/PUT/DELETE) has a `RouteResourceMapping` in the ABAC `ResourceResolver` | Pre-commit, CI |
| `scripts/check-tenant-filter.sh` | Every Diesel query method in adapters filters by `tenant_id` | Pre-commit, CI |
| `scripts/check-hierarchy.sh` | Domain structs don't use `Option<Id>` for hierarchy fields | Pre-commit, CI |

## Implementation Plan

1. **`scripts/check-hierarchy.sh`**: Grep `crates/gyre-domain/src/` for struct definitions containing `workspace_id: Option<Id>` or `tenant_id: Option<Id>`. Exit non-zero if found. Must exclude `gyre-common` types (e.g., Message.workspace_id is intentionally Option).

2. **`scripts/check-tenant-filter.sh`**: Scan all `*.rs` files in `crates/gyre-adapters/src/sqlite/` and `crates/gyre-adapters/src/pg/` (or `postgres/`). For each function that builds a Diesel query (look for `.filter(` or `diesel::` query builder patterns), verify that `tenant_id.eq(` appears. Maintain a skip list with documented rationale for exempt files (e.g., `message.rs` — queries by workspace_id which is tenant-bound).

3. **`scripts/check-api-auth.sh`**: The ABAC middleware already has a `non_exempt_patterns()` method that returns all registered patterns. This script should extract registered route patterns from the ABAC middleware and cross-reference against routes registered in `api/mod.rs`. Any route in mod.rs without a corresponding RouteResourceMapping entry fails. Consider using `cargo test` with a test that does this comparison, or grep-based approach.

4. Make all three scripts executable (`chmod +x`).

## Acceptance Criteria

- [ ] `scripts/check-hierarchy.sh` exists and passes on current codebase
- [ ] `scripts/check-hierarchy.sh` would fail if `workspace_id: Option<Id>` were added to Task
- [ ] `scripts/check-tenant-filter.sh` exists and passes (with documented skip list)
- [ ] `scripts/check-api-auth.sh` exists and passes
- [ ] All scripts have usage comments at the top
- [ ] `cargo test --all` still passes

## Agent Instructions

Read `specs/system/hierarchy-enforcement.md` §2 (Invariant Enforcement), §3 (Enforcement), and §7 (Mechanical Enforcement) for the full script requirements. The scripts are purely static analysis (grep/regex) — no runtime execution. Check the existing `scripts/check-arch.sh` for style conventions used by other enforcement scripts in this codebase.
