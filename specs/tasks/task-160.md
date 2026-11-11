---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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
- [ ] `cargo test --all` still passes (workspace suite is verifier-owned;
      `cargo check -p gyre-server --tests` clean in-sandbox — see Shipped)

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
end-to-end. `b82af6b4` additionally binds both `evaluate_attestation_abac`
results in `merge_processor.rs` and appends them to the policy decision
audit trail via `record_abac_decision`.

### This round — admin.rs merge-regression repair (c7ea6ab05)

Review findings 2d69e0d8 R1/R2: the c96170ce3 merge resolution kept the
branch's stale `admin.rs` wholesale, silently dropping upstream security
features present in the merge base and assigned base fbf1a63b5, and deleted
the three tests that pinned them. Repaired by restoring base `admin.rs` and
re-applying the branch's only legitimate post-base delta on top (the test
refactor: `test_workspace` helper,
`admin_audit_drops_telemetry_with_unresolvable_scope`, JWT claims without an
explicit tenant). Restored behavior, all verified present at HEAD:

- `admin_kill_agent`: `AuthenticatedAgent` extractor; backend-aware process
  kill via `spawned_backends` (docker rm --force / kubectl delete pod / SSH,
  LocalTarget only as the pre-task-117 unknown-backend fallback);
  `attested_pid` fallback; token revocation (`revoke_agent_token`, reason
  `admin_kill` — killed agents must not continue to authenticate);
  task-scoped ephemeral secret revocation (`revoke_agent_task_secrets`);
  `AgentKilled` audit event with caller attribution.
- `admin_reassign_agent`: `AuthenticatedAgent` extractor and per-task
  `AgentReassigned` audit events attributing the admin who moved the task.
- `admin_seed`: `seed_builtin_personas_for_tenant` for the tenant it creates.
- The three pinning tests restored verbatim from base:
  `admin_kill_revokes_task_scoped_secrets`,
  `admin_kill_emits_lifecycle_audit_events`,
  `admin_reassign_emits_agent_reassigned_audit_event`.

Diff audits: vs assigned base fbf1a63b5 the file differs only by the test
refactor (+56/−26); vs the pre-merge branch head 38e4097 only by the
security restore. Authz was never lost (both routes ABAC-mapped admin/write
in abac_middleware.rs:369-373).

### Test evidence (this round)

- `cargo check -p gyre-server --tests` — clean (server test compilation is
  verifier-owned; the build wrapper blocks in-sandbox server test builds).
- All three task scripts exit 0 on the merged tree; negative probe re-run:
  injecting `Task.workspace_id: Option<Id>` → exit 1 naming task.rs:60,
  restored → exit 0 (evidence: `/tmp/stage/review-evidence/`).
- Frozen-exemption gates stay green with the restored file:
  check-scope-literal-defaults, check-fabricated-scope-defaults,
  check-abac-exempt-handlers, check-forwarded-header-trust,
  check-inert-enforcement all exit 0.

### Server tests the verifier must execute

`cargo test -p gyre-server --lib admin::` — covers the restored kill/reassign
security features and audit events, including
`admin_kill_revokes_task_scoped_secrets`,
`admin_kill_emits_lifecycle_audit_events`,
`admin_reassign_emits_agent_reassigned_audit_event`, and
`admin_audit_drops_telemetry_with_unresolvable_scope`. Additionally, from
the earlier rounds: `attestation_abac`, `gate_passed_notification_deduplicated`,
`ws_activity_event`, `reconciliation`, `on_spec_approved`/`signal_chain`,
`spec_approval`; plus `cargo test -p gyre-server --test tenant_isolation` and
`cargo test --all` for the acceptance criterion.

Full workspace suites, architecture checks, all-target Clippy, and GitHub CI
are owned by verification and publication.
