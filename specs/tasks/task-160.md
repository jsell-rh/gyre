---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["9a91f0cd96ca1dafc35df7457461704596c54e97", "fe9ea725903e8038442914b170ea7786bd5b5761", "c964e54a414819bbd54cb3cd69602919a1a88006", "3b8b0eb6d4596edfeb900a3f7fe74db04733376c", "0c0a0ff17f541581acfa753a849f760eb0ba575a", "32a42f7e5590b6f700478739a5f690d588cb8c39", "531f1bcfe1263ffec480597b838b5eb1ac7a1615", "06b13c0defde8a92f3dfcc1c333774e448e0c77a", "1724c6a3d38c6086c0d01f198783cd860d4a7787", "55a6c1f2bee9a8c856e6576f6d7c91d6d2ce80c9", "8458f699df0b4b075eea35a31c2a3e32107c1884", "d162c56d01c9b742d17fc1626e2095adb0d64a47", "c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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

All three hierarchy-enforcement scripts exist, are executable, carry
usage comments, and are wired into `.pre-commit-config.yaml` and
`.github/workflows/ci.yml` (blocking, no `|| true`):

- `scripts/check-hierarchy.sh` — awk state machine over every domain
  `.rs` file; fails on `Option<...>` hierarchy fields and on deletion of
  a required field (spec §2). Gated via `GYRE_CHECK_HIERARCHY`
  (enabled by default, `=0` bypasses).
- `scripts/check-tenant-filter.sh` — POSIX-awk scan of read query
  methods in `src/sqlite` + `src/postgres`; the tenant-column table set
  is derived from the migrations; documented `SKIP_LIST` for
  structurally-isolated or deliberately cross-tenant adapters (spec
  §3); tenant-less tables reported as non-failing backlog.
- `scripts/check-api-auth.sh` — ABAC middleware chain on the api
  router, per-handler auth extractors on non-ABAC routes, and the
  route↔`RouteResourceMapping` cross-reference delegated to the
  frozen-baseline `check-abac-route-registry.sh` (spec §7).

### CI repair on this round (`9a91f0cd`)

The reconciled candidate `15fe9a5b` failed the CI "Architecture lint"
job (run 44086888872) on `check-tenant-filter.sh` — the only blocking
step that did not pass locally. The task-013 briefing merge (base
`ce336114`) introduced `NotificationRepository::list_by_workspace_type_since`
in both the SQLite and PostgreSQL adapters without a `tenant_id`
filter, and this task's now-blocking lint correctly flagged the two
methods.

- Added `.filter(notifications::tenant_id.eq(&tenant))` (handle-tenant
  scope, cloned into the `spawn_blocking` closure) to both adapters —
  the same contract as every sibling notification read on the same
  handle (`get`, `list_for_user`, `count_unread`, `list_recent`,
  `has_recent_dismissal`), the pattern round-4 review verified as real
  enforcement.
- Strengthened the existing adapter test
  `sqlite::notification::tests::list_by_workspace_type_since_scopes_and_is_unbounded`
  with a same-workspace, same-type, qualifying row filed under
  `tenant-2`: it must not leak into the tenant-1-scoped query.
  Mutation-checked on this head: removing the filter makes the test
  fail with `n-saf-cross-tenant` in the result (log:
  `mutation-check-test-fails.log`).

### Evidence on this head (`9a91f0cd`, logs under /tmp/stage/review-evidence/)

- `cargo-check-gyre-adapters.log` — `SKIP_WEB_BUILD=1 cargo check -p
  gyre-adapters` → exit 0.
- `cargo-test-sqlite-notification-final.log` — focused sqlite
  notification adapter tests → 10 passed, 0 failed.
- `cargo-test-tenant-isolation.log` — `--test tenant_isolation` →
  2 passed, 0 failed.
- `cargo-test-gyre-adapters-lib.log` — full `-p gyre-adapters --lib`
  → 575 passed, 0 failed (6 ignored).
- All blocking steps of the CI "Architecture lint" job pass on this
  head under mawk (scripts are POSIX-awk; GitHub runners' gawk is a
  superset for these constructs): `check-arch.sh`,
  `check-api-auth.sh`, `check-tenant-filter.sh` (126 read methods
  checked, 0 violations), `GYRE_CHECK_HIERARCHY=1 check-hierarchy.sh`,
  `check-sandbox-sweep-artifacts.sh`, `check-abac-route-registry.sh`,
  `check-migration-versions.sh`, `check-byte-slice-truncation.sh`,
  `check-dead-message-kinds.sh`,
  `check-notification-toggle-coverage.sh`.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 (the
  frontmatter commit list above includes this round's repair commit).

### Deferred to the independent verifier

`cargo test --all` (acceptance criterion) — the sandbox build wrapper
reserves large server builds for the verifier. On head `9a91f0cd`
please run the full `cargo test --all` plus the GitHub checks; the
adapters crate is fully covered above, and the changed code paths
(sqlite/pg notification adapters) have no other dependents in the
server test suite beyond the mem-mode tests (mem adapter unchanged).
