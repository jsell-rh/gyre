---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3", "c96170ce31ac484728a66d7d2423d5872eea0933", "45cd250d7655a2661f1cd7488ea4ab91ade1819e"]
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
  (`GYRE_CHECK_HIERARCHY`; enabled by default, `=0` to bypass). Mutation
  probe re-run this round: `Task.workspace_id -> Option<Id>` → exit 1 naming
  task.rs:60; restored → exit 0 (evidence:
  `/tmp/stage/review-evidence/blocking-gates.log`).
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
end-to-end. `b82af6b4` additionally binds both `evaluate_attestation_abac`
results in `merge_processor.rs` and appends them to the policy decision
audit trail via `record_abac_decision`.

### This round — rebase repair onto base cfeefbda (c96170ce3)

Resolved the 15-file merge of upstream `cfeefbda` (task-091/104/115/134/197
work) onto the retained checkpoint, keeping both sides' fixes:

- **lib.rs** — upstream's new modules/AppState fields kept; the branch's
  scoped `emit_event(Id)` API retained on top of upstream's
  `message_tenant_id` (tenant resolved from the workspace record,
  skip-and-log on unresolvable — no fabricated "default").
- **api/specs.rs** — upstream's transition-into-Approved guard, payload
  `workspace_id`, and signal-chain interception (`on_spec_approved`)
  combined with the branch's skip-and-log scope semantics (no Broadcast
  fallback under a fabricated tenant); upstream's task-secret revocation on
  spec-reject kept.
- **gate_executor.rs** — upstream's task-134 fix taken (skip GateFailure
  emission when the MR's scope is unresolvable), replacing the branch's
  still-exempted `unwrap_or("default")` site.
- **git_http.rs** — upstream's task-104 audit events, PushRejected
  gate-name reason, and applied-updates-gated PushAccepted taken.
- **merge_processor.rs** — merge-time attestation ABAC now BOTH records
  the decision (branch, `record_abac_decision`) AND enforces Deny by
  failing the queue entry (upstream task-077).
- **reconciliation.rs** — upstream's rolling-rollout structure taken; the
  per-task `TaskCreated` emission retained inside
  `create_reconciliation_task`.
- **ws.rs** — upstream's tenant-default-workspace telemetry fallback taken
  (a real workspace record, skip-and-log when the tenant has none), with
  its two hard tests.
- **Enforcement exemption lists pruned to the union of both sides' fixes**,
  verified by running the checkers on the merged tree:
  fabricated-scope-defaults 4→1, scope-literal-defaults 10→2,
  inert-enforcement 0. `check-tenant-filter.sh`: upstream's new
  `has_active_notification` (sqlite+postgres) now filters by tenant like
  its twin `has_recent_dismissal`; `user.rs` added to the documented
  SKIP_LIST (identity resolution precedes tenant context; `users.tenant_id`
  is a nullable home-tenant binding per migration 000058, so a filter
  would hide legacy unbound rows from OIDC login).
- **Coverage matrix** — task-160 rows (10/14/27) kept implemented;
  task-197's Bootstrap Behavior row added from base; 16/18.

### Test evidence (this round)

- `cargo check --workspace --all-targets` — clean (0 errors).
- `cargo test -p gyre-adapters --lib notification` — 6 passed, 0 failed.
- All blocking CI gates exit 0 on the merged tree: check-arch,
  check-api-auth, check-tenant-filter (116 read methods, 0 violations),
  check-hierarchy (+ negative probe), check-abac-route-registry,
  check-migration-versions, check-dead-message-kinds,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-inert-enforcement (evidence:
  `/tmp/stage/review-evidence/blocking-gates.log`).

### Server tests the verifier must execute

`cargo test -p gyre-server --lib` filters: `attestation_abac` (recorded +
enforced merge-time decisions), `gate_passed_notification_deduplicated`
(has_active_notification tenant filter), `ws_activity_event` (both
tenant-default and skip-when-none paths), `reconciliation` (rolling
rollout), `on_spec_approved`/`signal_chain`, `spec_approval`; plus
`cargo test -p gyre-server --test tenant_isolation`.

### Known inherited-from-base findings (NOT merge-caused)

These pre-commit checks fail identically on upstream base `cfeefbda` (they
pass on the pre-merge branch HEAD); the violations are in upstream-owned
code and are recorded here for the verifier/PM rather than redesigned in a
rebase repair: `check-dead-parameters` (spawn.rs:331 log-only param),
`check-dependent-scope-resolution` (reconciliation.rs:927,
explorer_views.rs:736, graph.rs:664), `check-ignored-tool-tests` (11
docker/ssh/container backend fns), `check-non-atomic-creation`
(git_http.rs process_persona_changes). None run blocking in CI.

Full workspace suites, architecture checks, all-target Clippy, and GitHub CI
are owned by verification and publication.
