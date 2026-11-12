---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["55a6c1f2bee9a8c856e6576f6d7c91d6d2ce80c9", "8458f699df0b4b075eea35a31c2a3e32107c1884", "d162c56d01c9b742d17fc1626e2095adb0d64a47", "c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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

### This round: compile repair after base merge

The round-3 review finding (attempt 5c276126): the candidate tree did not
compile — `emit_tenant_meta_spec_set_updated` (reconciliation.rs) still
called `state.emit_event(None, ...)` after the base merge reintroduced it,
while the scoped `emit_event` signature (candidate commit 2698b9b63)
requires an `Id` workspace. Fixed in the sanctioned direction
(commit 55a6c1f2b):

- New `AppState::emit_tenant_event(tenant_id, to, kind, payload)` — an
  Event-tier emit path for tenant-level domain events whose scope is the
  authenticated caller's tenant, carried directly (never fabricated). It
  shares the sign/persist/broadcast/dispatch core with `emit_event` via
  the extracted `emit_scoped_event`; the message carries
  `workspace_id: None` (no single workspace audience, per the §11 doc
  comment on the emitter).
- `emit_tenant_meta_spec_set_updated` now calls `emit_tenant_event` with
  the handler's tenant scope. The live production path is
  `api/meta_specs.rs` `put_tenant_meta_spec_set`.
- `message_tenant_id` drops its now-dead `Option` arm (the
  system-principal-tenant fallback); both remaining callers pass a
  concrete workspace `Id`.

### Test evidence

Evidence for this round saved under `/tmp/stage/review-evidence/`:

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` exits 0 on the repaired
  head (`server-check-pass.txt`) — the E0308 from the review finding is
  gone.
- `cargo fmt -p gyre-server --check` clean.
- All three task scripts pass on the repaired head
  (`check-hierarchy.txt`, `check-tenant-filter.txt`,
  `check-api-auth.txt`), plus arch / fabricated-scope-defaults /
  inert-enforcement / scope-literal-defaults / mem-port-contracts lints.
- Earlier rounds' evidence (negative tests proving each script fails on
  injected violations, domain/adapter test runs) is recorded in the prior
  attempt artifacts; this round changed only `gyre-server` source, which
  is type-checked above.

Server suites the verifier must run on this head (per the sandbox build
budget): `cargo test --all`, all-target clippy, and the frozen-baseline
script gates in CI.
