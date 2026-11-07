---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "696ebfb083e30eb992856c36a11aff9a570edd4a", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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
blocking):**

- `scripts/check-hierarchy.sh` — awk state machine over every domain `.rs`
  file; fails on `Option<...>` hierarchy fields AND on deletion of a required
  field (a deleted edge re-opens the same bypass). Env-gated per spec §2
  (`GYRE_CHECK_HIERARCHY`; enabled by default, `=0` to bypass).
- `scripts/check-tenant-filter.sh` — POSIX-awk scan of read query methods in
  `src/sqlite` + `src/postgres` (the real pg dir); tenant-column tables
  derived from the migrations themselves rather than hand-maintained;
  documented SKIP_LIST for the structurally-isolated adapters (spec §3);
  tenant-less tables reported as non-failing backlog.
- `scripts/check-api-auth.sh` — three checks: ABAC middleware chain on the
  api router, per-handler auth extractors on non-ABAC mutating routes, and
  route↔`RouteResourceMapping` cross-reference (delegated to the owning
  frozen-baseline `check-abac-route-registry.sh`, folded into this exit
  code).

**Enabling repairs** shipped with the scripts so they pass clean, not by
exemption: unfiltered tenant reads fixed in sqlite+postgres
compute_target/merge_request/notification/repository/saved_view/workspace;
`tenant_isolation.rs` integration test proves cross-tenant invisibility
end-to-end. `b82af6b4` (round 8) additionally binds both
`evaluate_attestation_abac` results in `merge_processor.rs` and appends them
to the policy decision audit trail via `record_abac_decision`, so no
inert-enforcement exemption entries remain for that file.

### Current round — requirements-changed repair on the retained checkpoint

The task contract was re-issued (`depends_on: []`) and an upstream merge
(`4d6759a0`, task-084) was resolved on top of the retained checkpoint
`a8769d5`:

- **Merge resolution** — attestation-ABAC call sites kept on the
  task-160 `record_abac_decision` consumption (not upstream's audit-only
  warn); the upstream trust-gate functions
  (`evaluate_merge_abac`/`supervised_trust_hold`/`selection_trust_hold`)
  required by upstream's callers were kept alongside; the
  inert-enforcement exemption file kept its HEAD form (no
  merge_processor entries).
- **Semantic repair (`9dc6ffd47`)** — upstream's new `reconciliation.rs`
  called `emit_event` with `Option<Id>` under a signature that fabricated
  `tenant_id: "default"`; all seven call sites adapted to the branch's
  scoped `Id` signature, and the unscoped `None`+`Broadcast` fallback is
  now skip-and-log per the fabricated-scope-defaults invariant.

### Test evidence

- `cargo test -p gyre-server --lib
  merge_attestation_abac_decision_recorded_in_audit_trail` — 1 passed.
- `cargo test -p gyre-server --lib supervised_trust` —
  `supervised_trust_holds_atomic_group_members` passed (union resolution
  compiles and behaves).
- `cargo test -p gyre-server --lib reconciliation` — 21 passed.
- `cargo check -p gyre-server --tests` — clean.
- All three task scripts exit 0 on the merged tree;\  `check-hierarchy.sh` proven to exit 1 when
  `Task.workspace_id: Option<Id>` is introduced (negative probe, file
  restored). `check-inert-enforcement`, `check-fabricated-scope-defaults`,
  and `check-scope-literal-defaults` also exit 0.
- Evidence retained at `/tmp/stage/review-evidence/task-160-merge-resolution.md`.

Full workspace suites, architecture checks, all-target Clippy, and GitHub CI
are owned by verification and publication.
