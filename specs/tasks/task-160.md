---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: [task-236]
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
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

### Verification (latest round — merged HEAD 8d2ae409; evidence:
`/tmp/stage/review-evidence/task-160-round6/`)

This round resumed after the prior run exhausted its token budget
mid-verification. The assigned merge c6c91e01 folded base `63aa1486`
(shared Fullsend dispatcher extraction, task-192 budget CLI, task-239
explorer WS sessions) into the candidate; the task-160 surface
(scripts/adapters/abac_middleware) is byte-stable through the merge — the
only deltas on wiring paths are base-side CI/dev-check changes that do not
weaken the lint steps (all three remain blocking in ci.yml, no `|| true`).

- Clean gates at 8d2ae409: `check-hierarchy.sh` exit 0;
  `check-tenant-filter.sh` exit 0 (111 read methods on tenant-column
  tables / 0 violations); `check-api-auth.sh` exit 0 (middleware chain +
  4 non-ABAC handlers + delegated frozen-baseline registry);
  `check-arch.sh` OK; `check-task-commit-attribution.sh` OK; and all 17
  dev-check static gates exit 0 (abac-route-registry, abac-exempt-handlers,
  mcp-write-tools, migration-versions, migration-sql-portability,
  dead-message-kinds, byte-slice-truncation, relative-path-defaults,
  fail-open-ref-resolution, mem-port-contracts, fabricated-scope-defaults,
  lossy-secret-conversion, inert-enforcement, forged-scope-fields,
  forwarded-header-trust, in-memory-state-stores,
  unbounded-external-http). `git diff --check HEAD^1 HEAD` clean.
- Fresh mutation kills in isolated worktree `/tmp/stage/task160-r6-mut`
  (restored after each; worktree removed after):
  `Task.workspace_id → Option<Id>` → hierarchy exit 1 naming `task.rs:60`;
  tenant predicate stripped from `sqlite/secret.rs::get_value` →
  tenant-filter exit 1 ("1 violation(s) out of 111", naming
  `secret.rs:229`); `RouteResourceMapping` for `/api/v1/activity` deleted →
  api-auth exit 1 via the delegated registry check. (Mutations applied
  newline-aware — the files carry CRLF endings.)
- `cargo test -p gyre-adapters --test tenant_isolation`: **2 passed /
  0 failed** — re-run this round because the base merge brought
  `gyre-common/src/view_spec.rs` (+257 lines, a gyre-adapters dependency)
  into the tree; run record in `tenant-isolation.txt` in the evidence dir.
- Inherited repair 8d2ae409: base-side task-239 inserted 5 lines into
  `crates/gyre-server/src/lib.rs` (AppState `explorer_sessions` field),
  shifting the two pre-existing task-099-owned exempted sites
  (`emit_event`/`emit_telemetry`) from lines 494/538 to 499/543;
  `check-scope-literal-defaults.sh crates` failed on the stale line-pinned
  exemption entries. Re-pinned the same two sites to their current lines —
  no entry added or removed, frozen count stays 12, the sites themselves
  are unchanged and still owned by the task-099 F1 revision round. FAIL
  exit 1 before, OK exit 0 after.

Review history: rounds 1–4 in `specs/reviews/task-160.md` (round-3 findings
F1 dead exemptions / F2 read-name blind spot fixed in 06df8bfb and verified
complete in round 4). Earlier per-round verification records
(contract-repair, final, rebase-repair, baseline-repair rounds at HEADs
fa6e6952, aa68bbba/72bd51d1, c4f1df9d, afd7379d) are preserved in this
file's git history.
