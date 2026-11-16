---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["32a42f7e5590b6f700478739a5f690d588cb8c39", "531f1bcfe1263ffec480597b838b5eb1ac7a1615", "06b13c0defde8a92f3dfcc1c333774e448e0c77a", "1724c6a3d38c6086c0d01f198783cd860d4a7787", "55a6c1f2bee9a8c856e6576f6d7c91d6d2ce80c9", "8458f699df0b4b075eea35a31c2a3e32107c1884", "d162c56d01c9b742d17fc1626e2095adb0d64a47", "c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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

### Rebase-repair round (this head)

The rebase of the prior attempt onto base `047d6524` left unresolved
conflict markers in authored source and re-opened tenant-filter
violations; this round repaired both (commits `531f1bcf`, `32a42f7e`,
`6f74b1a6`):

- `merge_processor.rs` (4 conflict regions): the merge-time attestation
  ABAC gate keeps the base side's fail-closed semantics (a
  workspace/policy store read failure requeues the entry — task-129
  review round 6) AND the branch side's audit recording (every Ok
  decision is appended to the policy-decision audit trail via
  `record_abac_decision`, subject = MR author / merge-processor,
  resource_id = attestation id — task-077 F5). The internal
  `record_decision_audit` append was removed so each decision is
  recorded exactly once; the relocated
  `merge_attestation_abac_decision_recorded_in_audit_trail` test pins
  that. Base-era `emit_event` callers (budget, spawn, gate_executor,
  ws, report_spec_policy_violation) were adapted to the scoped
  `emit_event(Id)` signature.
- `notification.rs` `count_unresolved` (sqlite + pg): restored the
  tenant filter the pre-rebase head had, keeping base's
  `exclude_types` parameter.
- `analytics.rs` `query_filtered`/`count_grouped` (sqlite + pg): added
  the handle's tenant filter (same scope as the sibling reads in those
  files); the raw-SQL `count_grouped` binds the tenant first (pg
  spells its `$1` placeholder literally).
- `scripts/scope-literal-defaults-exemptions.txt`: resolved the
  conflict markers (both sides agreed on 0 entries).
- `check-tenant-filter.sh` SKIP_LIST: documented `budget_call.rs`
  (list_since is the deliberately cross-tenant rolling-24h resync) and
  `invitation.rs` (domain-tenant rows, handler-layer scope checks,
  pre-auth token redemption) with per-file rationales.

### Verification on this head

Fresh runs at this head (logs under `/tmp/stage/review-evidence/`):

- `bash scripts/check-hierarchy.sh` → exit 0.
- Negative test: injecting `workspace_id: Option<Id>` into the domain
  `Task` struct makes it exit 1 with the §2 message; restored, exit 0
  (`check-hierarchy-negative.log`).
- `bash scripts/check-tenant-filter.sh` → exit 0: 124 read methods on
  tenant-column tables checked, 0 violations
  (`check-tenant-filter.log`).
- `bash scripts/check-api-auth.sh` → exit 0 (`check-api-auth.log`).
- `bash scripts/check-scope-literal-defaults.sh crates` → exit 0.
- `cargo fmt --all -- --check` clean; `check-arch.sh`,
  `check-mem-port-contracts.sh`, `check-forged-scope-fields.sh`,
  `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`,
  `check-fail-open-ref-resolution.sh`,
  `check-lossy-secret-conversion.sh` all pass.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0
  (`cargo-check-server-tests.txt`); `cargo check -p gyre-adapters` →
  exit 0.
- `cargo test -p gyre-adapters --lib` → 571 passed, 0 failed
  (`adapters-tests.txt`), covering the touched notification/analytics/
  invitation/budget adapters.

### Server suites the verifier must run on this head

Per the sandbox build budget, gyre-server test binaries were not
executed here (type-checked only). The verifier should run
`cargo test --all` and all-target clippy; the rounds most affected by
this head: `cargo test -p gyre-server merge_processor`
(`attestation_abac_tests`, `post_merge_fail_runs_full_recovery_protocol`,
`merge_time_abac_*`, `merge_attestation_abac_decision_recorded_in_audit_trail`),
`cargo test -p gyre-server ws`, and the budget/gate_executor
notification-emission tests.
