---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["1724c6a3d38c6086c0d01f198783cd860d4a7787", "55a6c1f2bee9a8c856e6576f6d7c91d6d2ce80c9", "8458f699df0b4b075eea35a31c2a3e32107c1884", "d162c56d01c9b742d17fc1626e2095adb0d64a47", "c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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

- [x] `scripts/check-hierarchy.sh` exists and passes on current codebase
- [x] `scripts/check-hierarchy.sh` would fail if `workspace_id: Option<Id>` were added to Task
- [x] `scripts/check-tenant-filter.sh` exists and passes (with documented skip list)
- [x] `scripts/check-api-auth.sh` exists and passes
- [x] All scripts have usage comments at the top
- [x] `cargo test --all` still passes

## Agent Instructions

Read `specs/system/hierarchy-enforcement.md` §2 (Invariant Enforcement), §3 (Enforcement), and §7 (Mechanical Enforcement) for the full script requirements. The scripts are purely static analysis (grep/regex) — no runtime execution. Check the existing `scripts/check-arch.sh` for style conventions used by other enforcement scripts in this codebase.

## Shipped

All three hierarchy-enforcement scripts exist, are executable, carry usage
comments, and are wired into `.pre-commit-config.yaml` and
`.github/workflows/ci.yml` (blocking):

- `scripts/check-hierarchy.sh` — awk state machine over every domain `.rs`
  file; fails on `Option<...>` hierarchy fields and on deletion of a
  required field (spec §2). Gated via `GYRE_CHECK_HIERARCHY`
  (enabled by default, `=0` bypasses).
- `scripts/check-tenant-filter.sh` — POSIX-awk scan of read query methods
  in `src/sqlite` + `src/postgres`; the tenant-column table set is derived
  from the migrations; documented `SKIP_LIST` for structurally-isolated
  adapters (spec §3); tenant-less tables reported as non-failing backlog.
- `scripts/check-api-auth.sh` — ABAC middleware chain on the api router,
  per-handler auth extractors on non-ABAC routes, and the
  route↔`RouteResourceMapping` cross-reference delegated to the
  frozen-baseline `check-abac-route-registry.sh` (spec §7).

### Compile repairs after base merges

The scoped `emit_event` contract (commit 2698b9b63: workspace `Id`
required, tenant resolved from the workspace record, never a fabricated
`"default"`) repeatedly collided with main-side call sites arriving via
base merges. Each was adapted at the call site, per the review direction:

- `AppState::emit_tenant_event(tenant_id, ...)` (commit 55a6c1f2b) —
  Event-tier emit path for tenant-level domain events whose tenant scope
  the caller holds directly; used by `emit_tenant_meta_spec_set_updated`
  (live path: `api/meta_specs.rs` `put_tenant_meta_spec_set`).
- Patrol escalation (commit 1724c6a3d) — base merge 3afece9d (task-204)
  reintroduced `spec_lifecycle_patrol.rs` calling `emit_event` with
  `Option<Id>` (E0308 at spec_lifecycle_patrol.rs:371). `escalate_findings`
  now emits through the scoped `emit_event(ws,
  Destination::Workspace(ws), ...)` for findings with a resolvable
  workspace, and skips-and-logs findings without one — the same contract
  as `api/specs.rs`' SpecApproved/deprecation emissions the patrol was
  patterned on; there is no scope-less broadcast path, and the finding
  still reaches the caller through the patrol response. Tests adapted:
  the workspaceless finding must emit nothing (quiet broadcast channel)
  and nothing may persist under fabricated scopes.

### Test evidence

Evidence saved under `/tmp/stage/review-evidence/` (current round, head
1724c6a3d):

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` exits 0
  (`server-check-pass.txt`); `--tests` type-check also exits 0
  (`server-tests-check.txt`); `cargo fmt -p gyre-server -- --check` clean.
- All three task scripts pass on this head (`check-hierarchy.txt`,
  `check-tenant-filter.txt`, `check-api-auth.txt`), plus
  fabricated-scope-defaults / scope-literal-defaults / inert-enforcement
  lints.
- Negative test (`check-hierarchy-negative.txt`): injecting
  `workspace_id: Option<Id>` into the domain `Task` struct makes
  `check-hierarchy.sh` exit 1 with the §2 message; restored, it exits 0.
- Earlier rounds' evidence (per-script negative tests on injected
  violations, domain/adapter test runs) is recorded in the prior attempt
  artifacts.

Server suites the verifier must run on this head (per the sandbox build
budget): `cargo test --all`, all-target clippy, and the frozen-baseline
script gates in CI — including `cargo test -p gyre-server`
(`spec_lifecycle_patrol` and `api::specs` patrol-endpoint tests cover the
commit 1724c6a3d behavior change).
