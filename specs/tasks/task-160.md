---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["0c0a0ff17f541581acfa753a849f760eb0ba575a", "32a42f7e5590b6f700478739a5f690d588cb8c39", "531f1bcfe1263ffec480597b838b5eb1ac7a1615", "06b13c0defde8a92f3dfcc1c333774e448e0c77a", "1724c6a3d38c6086c0d01f198783cd860d4a7787", "55a6c1f2bee9a8c856e6576f6d7c91d6d2ce80c9", "8458f699df0b4b075eea35a31c2a3e32107c1884", "d162c56d01c9b742d17fc1626e2095adb0d64a47", "c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3", "3b8b0eb69df5a49dfad3e59e2a18d8c69c46ed33"]
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

### Rebase repair at the merged base (`3b8b0eb6`)

Merging base `6082be2f` (task-165 supply-chain + the task-099 F1 emit
contract) left three push-path call sites in `git_http.rs` calling
`emit_event(Some(repo_workspace_id.clone()), ...)`, but the base
changed `emit_event` to take `workspace_id: Id` directly (tenant now
resolved from the workspace record inside the emitter). The whole
`gyre-server` crate failed `cargo check` with three E0308s — this was
the failing verification finding. Fixed by unwrapping the `Some(...)`
at the three sites (stack-policy `PushRejected`, warn-mode
`ConstraintViolation` ×2), matching the sibling push-path call sites
that already pass the `Id`.

### Verification on this head (`3b8b0eb6`)

Fresh runs (logs under `/tmp/stage/review-evidence/`):

- `SKIP_WEB_BUILD=1 cargo check --workspace` → exit 0 (server,
  adapters, cli all type-check; was 3 × E0308 before the repair).
- `bash scripts/check-hierarchy.sh` → exit 0; mutation kill
  reconfirmed: making `Task.workspace_id` `Option<Id>` makes the script
  exit 1 naming the file and line
  (`check-hierarchy-mutation.log`), revert restores exit 0.
- `bash scripts/check-tenant-filter.sh` → exit 0: 124 read methods on
  tenant-column tables checked, 0 violations.
- `bash scripts/check-api-auth.sh` → exit 0.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --test tenant_isolation`
  → 2 passed, 0 failed (`tenant-isolation-test.txt`): two SqliteStorage
  handles over one db file differing only by tenant scope —
  cross-tenant rows invisible in both directions, and create() stamps
  the storage's real tenant rather than a fabricated one.

The task `commits:` frontmatter was restored to the full branch list
(15 prior SHAs) plus this round's repair commit, so
`check-task-commit-attribution.sh` passes again after the round reset.

Large server-wide suites (`cargo test --all`) are deferred to the
independent verifier per the assignment's build-budget rules; the
verifier should run `SKIP_WEB_BUILD=1 cargo test --all` and the three
scripts above.
