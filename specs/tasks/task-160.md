---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
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

## Shipped

**Scripts (all three exist, executable, usage-commented, wired pre-commit + CI
blocking — no `|| true`):**

- `scripts/check-hierarchy.sh` — awk state machine over every domain `.rs`
  file; fails on `Option<...>` hierarchy fields AND on deletion of a required
  field (a deleted edge re-opens the same bypass). Env-gated per spec §2
  (`GYRE_CHECK_HIERARCHY`; enabled by default, `=0` to bypass).
- `scripts/check-tenant-filter.sh` — POSIX-awk scan of read query methods in
  `src/sqlite` + `src/postgres` (the real pg dir — the stale `src/pg` path bug
  is fixed); tenant-column tables derived from the migrations themselves
  (CREATE/ALTER/DROP/RENAME, `_new` normalization) rather than hand-maintained;
  documented SKIP_LIST for the five structurally-isolated adapters (spec §3);
  tenant-less tables reported as non-failing backlog.
- `scripts/check-api-auth.sh` — three checks: ABAC middleware chain on the
  api router, per-handler auth extractors on non-ABAC mutating routes, and
  route↔`RouteResourceMapping` cross-reference (delegated to the owning
  frozen-baseline `check-abac-route-registry.sh`, folded into this exit code).

**Enabling repairs** (shipped with the scripts so they pass clean, not by
exemption): unfiltered tenant reads fixed in sqlite+postgres
compute_target/merge_request/notification/repository/saved_view/workspace and
activity/analytics record-conversions now stamp the storage's real tenant
(also deleted six dead scope-literal-defaults exemption entries, 18→12);
`tenant_isolation.rs` integration test (2 tests) proves cross-tenant
invisibility end-to-end.

**Verification at merged HEAD 32865a7c** (evidence:
`/tmp/stage/review-evidence/task-160-continued/`; task surface byte-identical
to reviewed candidate 13ff2a51 — empty diff on scripts/adapters/wiring):

- Clean: hierarchy exit 0; tenant-filter exit 0 (111 checked / 0 violations);
  api-auth exit 0; scope-literal-defaults `crates` OK; attribution OK.
- Mutation kills (isolated worktree, restored after each):
  `Task.workspace_id → Option<Id>` → hierarchy exit 1 (task.rs:60);
  stripping the tenant predicate from `sqlite/secret.rs::get_value` →
  tenant-filter exit 1 (file:line); deleting the resolver entry for
  `/api/v1/activity` → api-auth exit 1; renaming the route without the
  registry → api-auth exit 1. Deleting a route registration with the registry
  intact stays green (covered direction only — by design).
- `cargo test -p gyre-adapters --test tenant_isolation`: 2 passed / 0 failed
  (cold build resumed past the 300s cell deadline; total ~14 min).

**task-210 attribution drift (pre-existing at assignment base, fixed here as
the lint prescribes):** `a781ede2` landed on main 15:29 (before the 16:32
base) but was absent from `specs/tasks/task-210.md` frontmatter, failing
`check-task-commit-attribution.sh`. Recorded the SHA in the task file's
`commits:` list; no exemption added, frozen exemption file untouched. Gate
passes.

Review history: rounds 1–4 in `specs/reviews/task-160.md` (round-3 findings
F1 dead exemptions / F2 read-name blind spot fixed in 06df8bfb and verified
complete in round 4).
