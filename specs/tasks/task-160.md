---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
progress: complete
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["d876f8883cc4d7d374786a6b56255a9549af75bc", "f29a3464eaca73ac6e932dfd95da6522a10e438e", "ceb62097a1dac7012bb7156add686b54a75f8c25", "a4d0980a972f0a268cb5406b467077d416100589", "affd4788cb6c8aa825c0d5e8a0e1d0c503d5b1d6", "1f092d62dc5680fb49bd02f960b9c79ced0c33c5", "06df8bfb02cbf0c50059c580f1f87860708543ff"]
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


## Round-3 Repair (review findings F1, F2 — commit 06df8bf)

**F1 — dead scope-literal-defaults exemption entries.** Deleted the six
lines for the sites this task's record-conversion fixes already repaired
(`sqlite/activity.rs:56`, `sqlite/analytics.rs:115`+`225`,
`postgres/activity.rs:56`, `postgres/analytics.rs:115`+`225` — all now
stamp the storage's real tenant) and lowered `FROZEN_EXEMPTION_COUNT`
18→12 in both the header comment and the Python constant of
`scripts/check-scope-literal-defaults.sh`; exemption-file header updated.
Verified: no-exemption scan of `crates/` finds exactly 12 live violations,
all covered by the remaining 12 lines; `check-scope-literal-defaults.sh`
passes on clean HEAD.

**F2 — check-tenant-filter.sh read-name blind spot.** Added `record` /
`resolve` to the `is_read` prefix set. Pre-extension enumeration over both
adapter dirs: exactly two `record*`/`resolve*` fns carry pure-read
terminals — `sqlite/agent.rs::record_usage` (read-modify-write; `.first()`
on `agents`, already filters `agents::tenant_id.eq(&tenant)`) and
`sqlite/secret.rs::resolve_for_agent` (pure read; `.load()` on `secrets`,
already filters `secrets::tenant_id.eq(tenant_id)`); every other
`record*`/`resolve*` fn is a pure insert/update with no `.load/.first/
.get_result` terminal so the `has_diesel` gate excludes them, and the pg
`resolve_for_agent` twin is a `bail!` stub with no terminal. Clean HEAD:
111 checked / 0 violations (was 109). Mutation probes in an isolated
worktree (restored after each): removing the tenant predicate from either
fn fails the lint with exit 1 naming file and line.

## Shipped

- Three spec §7 enforcement scripts, wired into pre-commit and as blocking CI steps: `check-hierarchy.sh` (domain hierarchy fields must be non-optional `Id`; scans by struct name across the domain crate, kills `Option<Id>` and deleted-field mutations), `check-tenant-filter.sh` (every Diesel read method touching a tenant-column table must carry a `tenant_id` predicate; table set derived from the migrations with DROP/RENAME-recreation handling; 111 fns checked, 0 violations on HEAD), `check-api-auth.sh` (middleware chain + per-handler auth on exempt routes + route→ABAC registry coverage via the registry owner).
- Real adapter enforcement behind the lint, both backends: every tenant-column read filters `tenant_id.eq(&storage.tenant_id)`, raw SQL carries `AND tenant_id = ?/$4`, and the four create() sites (merge_request/repository/analytics × sqlite+postgres) stamp the storage's real tenant instead of `"default"` — proven by `tests/tenant_isolation.rs` (two storages over one db file), which fails with exact leak diagnostics when either defect class is reintroduced.
- Scope-literal-defaults exemption file cut 24→12 (dead entries for sites this task actually fixed removed; `FROZEN_EXEMPTION_COUNT` lowered to match), so a future `"default"` scope stamp at any formerly-exempted position now fails the lint.
- Read-by-behavior coverage of `record_*`/`resolve*` prefixes closes the last known name-heuristic blind spot (`record_usage`, `resolve_for_agent`); independent cross-check confirms the heuristic is now exhaustive over the tenant-column read surface.
