---
title: "Implement tenant-scope meta-spec changes"
spec_ref: "meta-spec-reconciliation.md §8"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §8 Tenant-Scope Meta-Spec Changes"
commits: ["b7eaa775ceb61c3da7b9802e1b1c17a627b74ace", "5ef42f5c3cef7360a801154841399d98b49b5fc7", "381962ab9cedd8ed2c574adf2d7a09e7878310e8", "3c22bfa874c70968192207603815adbddec5f6df"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §8 — Tenant-Scope Meta-Spec Changes:

> When a tenant-level meta-spec changes, all workspaces in the tenant are affected. The reconciliation process:
> 1. Computes blast radius across all workspaces
> 2. Creates per-workspace reconciliation plans
> 3. Applies workspace rollout policies individually
> 4. Tenant admin can override workspace policies for urgent changes

## Implementation Plan

1. **Add Tenant scope to MetaSpecScope** (`crates/gyre-domain/src/meta_spec.rs`):
   - Currently MetaSpecScope has `Global` and `Workspace` — add `Tenant(Id)` variant
   - Update serialization and DB schema to support tenant-scoped meta-specs

2. **Add tenant-level meta-spec set binding**:
   - Add a `tenant_meta_spec_set` concept — tenant-wide defaults that workspaces inherit unless overridden
   - API: `GET/PUT /api/v1/tenants/{id}/meta-specs/set` — manage tenant-level meta-spec set
   - Workspace meta-spec sets inherit from tenant, with workspace-level overrides taking precedence

3. **Extend reconciliation controller** for tenant scope:
   - When a tenant-level meta-spec is approved, iterate all workspaces in the tenant
   - For each workspace, check if the workspace overrides this meta-spec — if so, skip
   - For non-overriding workspaces, apply the workspace's rollout policy to create reconciliation tasks
   - Compute cross-workspace blast radius using the existing blast-radius endpoint

4. **Tenant admin override**:
   - Add `force` flag on reconciliation trigger that ignores workspace-level rollout policies
   - Used for urgent compliance-driven changes that must be applied immediately

5. **Migration**:
   - Add `tenant_id` to relevant meta-spec tables if not already scoped
   - Add `tenant_meta_spec_sets` table or column

6. **Tests**:
   - Unit test: tenant-scoped meta-spec change creates workspace reconciliation tasks
   - Unit test: workspace-level override prevents tenant propagation
   - Unit test: force flag bypasses workspace rollout policy
   - Unit test: blast radius includes all non-overriding workspaces

## Acceptance Criteria

- [ ] MetaSpecScope includes Tenant variant
- [ ] Tenant-level meta-spec sets can be managed via API
- [ ] Workspace meta-spec sets inherit from tenant with override support
- [ ] Tenant meta-spec approval triggers per-workspace reconciliation
- [ ] Workspaces with overrides are skipped during tenant reconciliation
- [ ] Tenant admin force flag bypasses workspace rollout policies
- [ ] Tests cover inheritance, override, and force scenarios

## Agent Instructions

- Read `crates/gyre-domain/src/meta_spec.rs` for MetaSpecScope enum (currently Global, Workspace)
- Read `crates/gyre-server/src/api/meta_specs.rs` for existing meta-spec API (blast-radius endpoint at lines ~252-308)
- Read task-156 for the reconciliation controller this task extends
- The tenant meta-spec set is analogous to workspace meta-spec set — follow the same pattern in `gyre-ports/src/meta_spec_set.rs`
- Workspace inheritance: when resolving a workspace's effective meta-spec set, merge tenant defaults with workspace overrides (workspace wins on conflict)

## Shipped

**Tenant scope on the registry (§8)** — `MetaSpecScope` gains a `Tenant`
variant (scope entity in `scope_id`, mirroring the Workspace design, so the
enum stays a plain string column). Migration `2026-11-07-000059` (renumbered from 000057 during the rebase onto main — 000057 collided with `audit_detail_compression`) widens the
`meta_specs.scope` CHECK to `Global|Tenant|Workspace` (portable three-table
rebuild — children dropped before the parent, parents copied before
children) and creates `tenant_meta_spec_sets`. Creation validates that the
scope tenant exists; `scope=Tenant&scope_id=` listing requires
caller-tenant containment.

**Tenant-level meta-spec set (§8 binding unit)** — new
`TenantMetaSpecSetRepository` port (get/upsert/delete by tenant id) with
mem, SQLite, and PostgreSQL adapters (separate table from workspace sets,
so a tenant and workspace sharing an id string never shadow each other).
API `GET/PUT /api/v1/tenants/{id}/meta-specs/set`: Admin-only plus tenant
containment, registered in the ABAC route resolver (resource `meta_spec`)
with per-handler authorization.

**Inheritance with override support** —
`resolve_effective_meta_spec_set` merges tenant defaults with the
workspace's own overrides (workspace pin wins on conflict; persona keys
rebound by the workspace drop the tenant pin). Every consumer resolves the
effective set: the workspace set GET (merged view),
`compute_meta_spec_set_sha` (byte-identical for tenant-less workspaces, so
SHAs recorded before this change stay valid — no false drift on upgrade),
the blast-radius endpoint (reason `tenant_inheritance` vs
`workspace_binding`), the preview blast radius, and the
`meta_spec_changed` fan-out.

**Per-workspace reconciliation (§8 steps 1-3)** —
`run_tenant_reconciliation(tenant, changed_paths, force)` lists the
tenant's workspaces, skips workspaces that override a changed path (their
own rollout policy governs), and runs a §6 reconciliation wave per
non-overriding workspace (task per repo, existing dedup/title rules).
A workspace whose own set cannot be read (storage error / corrupt JSON)
is skipped too — the override gate never fails open, so an unreadable
set is never treated as "no overrides" (counted in
`workspaces_skipped_error`, surfaced in both trigger log sites).
Triggers: tenant-set PUT (path diff; removals are not triggers) and
approval of a Tenant-scoped registry entry. Only paths the tenant set
pins propagate — an unbound change has an empty blast radius.

**Tenant admin force flag (§8 step 4)** — `force: true` on the tenant-set
PUT and on Tenant-scoped registry approval bypasses the workspace-level
override skip so urgent compliance changes apply immediately. Force is
only defined for Tenant scopes (400 otherwise) and only an admin of the
scope tenant may use it (403 cross-tenant).

**Spec lifecycle** — spec §8 amended to document the shipped binding-model
realization (tenant set + inheritance + override skip + force), replacing
the never-shipped `POST /api/v1/tenant/meta-spec-rollout` sketch.
Coverage matrix row 12 → `implemented`; api-reference documented.

### Review repair (round cd6d7fac, commit `381962ab9`)

Two findings from the independent review of candidate `8e4a59016`:

- **Fail-open override gate (code)** — `run_tenant_reconciliation`
  decoded an unreadable workspace set (storage error / corrupt JSON) as
  an empty set via `unwrap_or_default`, dropping that workspace's
  overrides so tenant propagation — including urgent force changes —
  could stomp a workspace that pinned its own version. Now fails closed:
  the workspace is skipped with a warn log and a
  `workspaces_skipped_error` summary counter. Killing test:
  `tenant_reconciliation_fails_closed_on_unreadable_workspace_set`
  (corrupt stored set + force → no propagation; fails under the old
  gate — verified by mutation).
- **No killing test for the set-PUT trigger (test)** — removing the
  `run_tenant_reconciliation` call from `put_tenant_meta_spec_set` left
  the full suite green (reproduced in review). New killing test:
  `tenant_set_put_propagates_approved_pins_to_inheriting_workspaces` —
  PUTs a tenant set pinning an APPROVED meta-spec through the API and
  asserts a reconciliation task in the inheriting workspace, none in
  the overriding one, and that a removal-only re-PUT creates none
  (fresh workspace added after the first PUT so dedup cannot mask a
  spurious trigger). Verified killing by mutation (trigger call
  deleted → test fails).

Spec §8 step 3 amended with the fail-closed clause.

### Review repair (round 6107f63d, commit `5ef42f5c3`)

Two findings from the independent review of candidate `d29f06ed7`:

- **Global/Workspace approval fan-out 500s after persisting (code)** —
  the else-branch of the `newly_approved` block propagated per-workspace
  set-resolution errors with `?` AFTER `state.meta_specs.update(&ms)`
  had already persisted the approval: one workspace anywhere with a
  corrupt stored `meta_spec_sets` JSON row made every Global/
  Workspace-scoped approval return 500 while the approval WAS persisted
  and the fan-out was partially executed (workspaces listed before the
  corrupt one had already received reconciliation tasks). Inconsistent
  with the tenant controller's fail-closed skip one branch above. Now
  matches the resolve result and skips the workspace with a warn log
  (same fail-closed-skip as `run_tenant_reconciliation`), keeping the
  approval response 200. Killing test:
  `global_spec_approval_skips_unreadable_workspace_set` — corrupt
  workspace created first (strongest ordering: under the old `?` the
  loop aborted before the healthy workspace was scanned), then a
  binding workspace: approval 200, `approval_status` Approved both in
  response and persisted via the port, healthy workspace still gets
  its reconciliation task, corrupt workspace gets none. Not executed
  in this repair sandbox — crates.io unreachable and no cached
  registry/target dir, so `cargo test` could not run (restriction
  recorded in `/tmp/stage/review-evidence/build-restriction-round6107f63d.txt`;
  remote verification must run it, plus the mutation kill: restore the
  `?` propagation → test must fail).
- **Compiler warning (code, minor)** —
  `tenant_meta_spec_set_requires_admin` left the
  `tokio::task::block_in_place(...)` result unused (`unused_must_use`).
  Fixed with `.expect("seed built-in policies")`, matching the sibling
  tests' pattern.

### Review repair (round aa6fdd18, commit `b7eaa775c`)

One finding from the independent review of candidate `bac70a024`:

- **Rustfmt violations on lines this task introduced (style)** —
  `python3 scripts/check-rustfmt-diff.py 233664cf8` (assigned base)
  exited 1, flagging non-canonical regions in `reconciliation.rs`,
  `api/meta_specs.rs`, `api/specs.rs`, `constraint_check.rs`,
  `abac_middleware.rs`, and `sqlite/meta_spec_set.rs`; `git blame`
  attributes every flagged line to this task's feature commit
  `3c22bfa8`. The gate passed at the pushed head only because HEAD^1
  was a docs-only commit; on integration against the main baseline it
  fails. Repair is mechanical rustfmt on the six files. (The re-pin of
  the `specs.rs` fabricated-scope-defaults exemption entry from line
  959 to 957 became moot in this rebase round: main's task-193 fix
  removed that site entirely, so the entry is gone — see the rebase
  repair section below.)
  Verified offline: rustfmt gate green against assigned base
  `233664cf8`, job base `4d6759a0b`, and `HEAD^1`; whitespace-stripped
  token comparison proves the change is formatting-only (only
  rustfmt canonical normalizations: trailing commas, closure braces,
  one import reorder); every reformatted statement contains a line
  authored by this task's commits, so no unrelated debt was touched.
  Not compiled in that sandbox (crates.io unreachable — same
  restriction as prior rounds); remote verification must run the
  deferred Rust checks.

### Rebase repair (round 38c991eb, onto main `fefde2946`)

Rebased the candidate onto the new main baseline `fefde2946` (which
now contains task-157's rollout policies, task-103/task-193 scope-fix
rounds, and `2026-11-07-000057_audit_detail_compression`). Conflict
resolutions, all source-level:

- **Keep-both conflicts** (schema.rs, sqlite/mod.rs table list,
  lib.rs/mem.rs/middleware.rs AppState wiring, api/meta_specs.rs
  types and tests, api-reference rows): §7 rollout-policy and §8
  tenant-set code coexist — both ports wired in AppState (mem and
  store! sites), both adapter impls, both route + ABAC resolver
  entries, both test suites. `run_tenant_reconciliation` composes
  with task-157's machinery: its per-workspace wave goes through
  `run_reconciliation`, which applies each workspace's rollout
  policy individually (§8 step 3).
- **Migration renumber** — `2026-11-07-000057_tenant_meta_spec_sets`
  collided with base's `2026-11-07-000057_audit_detail_compression`
  (duplicate Diesel version: only one runs, alphabetically first).
  Renumbered to `2026-11-07-000059_tenant_meta_spec_sets` (next
  unused sequence; migrations are embedded by directory glob, no
  code references the name). check-migration-versions green.
- **fabricated-scope-defaults exemptions** — resolved to main's
  version (2 entries, frozen count 3): main's task-193 and task-103
  rounds fixed the specs.rs and auth.rs sites outright, making the
  candidate's re-pinned specs.rs:957 entry stale; the git_http entry
  is likewise gone in main. Check green against the merged tree.
- **Coverage matrix** — row 11 (§7 Rollout Policy) takes main's
  `implemented` (task-157 shipped there); row 12 (§8) keeps this
  task's `implemented` row, with the migration number updated to
  000059.

Not compiled in this sandbox — crates.io unreachable (connection
refused, no cached registry/target; same restriction as prior
rounds — see
`/tmp/stage/review-evidence/build-restriction-rebase-38c991eb.txt`).
Offline verification: all 17 Rust files changed vs base parse and
are rustfmt-canonical; `check-rustfmt-diff.py fefde2946` green; the
20 invariant checks green; every other check script that fails on
the merged tree fails with byte-identical (or line-shift-only)
output on the base tree itself (see
`/tmp/stage/review-evidence/noise-check-comparison-rebase-38c991eb.txt`).
Remote verification must run the deferred Rust checks
(`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` §8 suite +
`global_spec_approval_skips_unreadable_workspace_set`,
`SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib`).

### Merge repair (round 8f042330, onto main `b5a872087`)

Merged main `b5a872087` (task-108 impersonation, task-208, task-205,
task-104 rounds; migrations 000056–000058) into the rebase-repair head.
Two source conflicts plus one compile casualty, all resolved and — for
the first time in this task's repair history — **verified with the full
deferred Rust suites in-sandbox** (cargo/crates.io reachable this
round):

- **mem.rs AppState wiring** — main restructured the test AppState
  literal: `agent_inbox_max`/`user_workspace_state` moved up beside
  `wg_config`, and `meta_specs`/`meta_spec_bindings` now share
  `meta_spec_binding_store` (`with_binding_store`/`with_store`) so the
  meta-spec delete guard sees bindings written through the binding
  repo (main's `delete_fails_when_binding_references_meta_spec`
  contract test). Resolution keeps main's shared-store wiring (the
  candidate's separate `.default()` stores would break that contract)
  and adds this task's `tenant_meta_spec_sets` field beside
  `meta_spec_sets` — both features coexist.
- **prompt_assembly.rs (new from main, compile casualty)** — main's
  agent-runtime §2 prompt assembly matched `MetaSpecScope`
  exhaustively over Global/Workspace; this task's `Tenant` variant
  made it non-exhaustive. Resolution: `scope_str` gains a `"tenant"`
  arm, and required band 1 now also injects required **Tenant-scoped**
  meta-specs of the task workspace's tenant (§8: tenant defaults apply
  to every workspace in the tenant). Without this, a required
  Tenant-scoped registry entry — creatable via this task's API —
  would silently never inject into any agent prompt. The caller's
  tenant is resolved from the task's workspace via
  `workspaces.find_by_id`; lookup failure skips the tenant band with
  a warn (no fabricated tenant identity). Dedup/order unchanged
  (`seen_ids`, kind rank, stable tiebreak).
- **api-reference.md** — the two registry rows merged: main's shape
  (`content_hash`, filters, content-PUT version bump + archive,
  DELETE 409 on bindings) combined with this task's Tenant scope +
  `force` flag semantics.

Test evidence at HEAD `380c7d6cb` (all run in this sandbox, cargo
reachable):

- `SKIP_WEB_BUILD=1 cargo check --workspace --all-targets` — clean.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — **1635 passed,
  0 failed** (includes the §8 tenant suite, the set-PUT and
  fail-closed killing tests, `global_spec_approval_skips_unreadable_workspace_set`,
  main's prompt-assembly suite, and the meta-spec binding contract
  tests on the resolved mem wiring).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` — **414
  passed, 0 failed** (the renumbered `000059_tenant_meta_spec_sets`
  migration runs on a fresh DB alongside main's 000056–000058;
  tenant-set repo roundtrip/independence).
- `cargo fmt --all -- --check` — clean.
- 19 mechanical invariant checks green (arch, abac-route-registry,
  abac-exempt-handlers, migration-versions, migration-sql-portability,
  mem-port-contracts, in-memory-state-stores, forged-scope-fields,
  scope-literal-defaults, dead-message-kinds, inert-enforcement,
  mcp-write-tools, byte-slice-truncation, fail-open-ref-resolution,
  relative-path-defaults, lossy-secret-conversion,
  fabricated-scope-defaults, forwarded-header-trust,
  unbounded-external-http);
  `check-task-commit-attribution` green (merge commit also recorded
  in `commits:` frontmatter so review scoping covers the conflict
  resolutions).

### Merge repair (round da1e8f16, onto base `cd4c04fe`)

Merged the assignment base `cd4c04fe4` (task-153 search
autocomplete/facets and its ancestors: spec lifecycle configs, task
spec refs, signal chain, narrative, compute targets) into the candidate
head `f61745fec` (merge commit `388c48242`, recorded in `commits:`).
One content conflict plus keep-both auto-merges, all source-level:

- **`lib.rs` use-list (the one manual conflict)** — base's
  `SpecLifecycleRepository` joins the candidate's
  `TenantMetaSpecSetRepository` in the `gyre_ports` import block.
- **Keep-both auto-merges** — AppState wiring (`lib.rs`/`mem.rs`/
  `middleware.rs`) carries both `spec_lifecycle_configs` and
  `tenant_meta_spec_sets` stores; `schema.rs` + the SQLite table list
carry both tables; `ports/lib.rs` re-exports both ports;
  `abac_middleware.rs` registers both route mappings
  (`search/suggest`, `search/facets`, `tenants/:id/meta-specs/set`);
  `api/mod.rs` registers both route groups; `api-reference.md`
  carries both endpoint rows.
- **`compute_meta_spec_set_sha` (semantic casualty check)** — base
  never touched the function or any §8 file after the merge base
  `b5a872087` (verified: `git diff --name-only b5a872087 cd4c04fe`
  has zero overlap with the §8 file set); the repo-ref signature in
  base is the pre-merge-base state. The candidate's `&AppState`
  signature — required for tenant+workspace effective-set resolution
  — and all its call sites are unchanged and tree-wide consistent.
- **No migration renumber this round** — the candidate's
  `2026-11-07-000059_tenant_meta_spec_sets` and base's
  `2026-10-08-000059_spec_lifecycle_configs` /
  `2026-11-08-000058_spec_approval_ledger` derive distinct Diesel
  versions (the date is part of the version); check-migration-versions
green on the merged tree.

Not compiled in this sandbox — crates.io unreachable (connection
refused on index/static.crates.io, no cached registry or target dir;
same restriction as prior rounds — recorded in
`/tmp/stage/review-evidence/build-restriction-round-da1e8f16.txt`).
Offline verification at merge commit `388c48242`:

- `cargo fmt --all -- --check` — clean (cargo fmt needs no network).
- `python3 scripts/check-rustfmt-diff.py cd4c04fe` — clean (53 Rust
  files changed vs base, all canonical).
- 19 mechanical invariant checks green on the merged tree (arch,
  abac-route-registry, abac-exempt-handlers, migration-versions,
  migration-sql-portability, mem-port-contracts,
  in-memory-state-stores, forged-scope-fields, scope-literal-defaults,
  dead-message-kinds, inert-enforcement, mcp-write-tools,
  byte-slice-truncation, fail-open-ref-resolution,
  relative-path-defaults, lossy-secret-conversion,
  fabricated-scope-defaults, forwarded-header-trust,
  unbounded-external-http).
- Noise-check: every check script whose output differs between the
  merged tree and the base tree differs only by line shifts, list
  reordering (the scripts' grep order is filesystem-dependent), or
  script-internal noise present on both sides; on deterministic
  re-runs the cli-spec-parity outputs are byte-identical (see
  `/tmp/stage/review-evidence/noise-check-comparison-round-da1e8f16.txt`).
- Remote verification must run the deferred Rust suites:
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` (§8 tenant suite
  + the set-PUT and fail-closed killing tests +
  `global_spec_approval_skips_unreadable_workspace_set` + base's new
  search/spec-lifecycle/signal-chain suites sharing those files) and
  `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` (the
  renumbered `000059_tenant_meta_spec_sets` migration must run on a
  fresh DB alongside base's `2026-10-08-000059` /
  `2026-11-08-000058`).

### Test evidence (at this rebase round, merged onto `fefde2946`; prior rounds below)

At the rebase-repair head (round 38c991eb, merged onto main `fefde2946`):

- `cargo test` could NOT run in this sandbox — crates.io unreachable
  (connection refused), no cached registry or target dir; see
  `/tmp/stage/review-evidence/build-restriction-rebase-38c991eb.txt`.
  Offline verification for this round: all 17 Rust files changed vs
  base parse and are rustfmt-canonical; `check-rustfmt-diff.py
  fefde2946` green; the 20 invariant checks green (see
  `/tmp/stage/review-evidence/invariant-checks-rebase-38c991eb.txt`);
  every other check script that fails on the merged tree fails with
  byte-identical (or line-shift-only) output on the base tree itself
  (see
  `/tmp/stage/review-evidence/noise-check-comparison-rebase-38c991eb.txt`).
  Remote verification must run the full deferred Rust suite (the §8
  tests, the rollout-policy suite from task-157 now sharing
  `meta_specs.rs`, and `gyre-adapters` migration/repo tests — the
  renumbered `000059_tenant_meta_spec_sets` migration must run on a
  fresh DB).
- No test logic was modified this round — only conflict resolution,
  the migration directory rename, and doc updates. Both test suites
  (§8 tenant + §7 rollout-policy) are present verbatim in
  `api/meta_specs.rs` and `reconciliation.rs`.

At HEAD `b7eaa775c` (prior round — formatting-only):

- `cargo test` could NOT run in this repair sandbox — crates.io
  unreachable (connection refused), no cached registry or target dir;
  see
  `/tmp/stage/review-evidence/build-restriction-round-aa6fdd18-repair.txt`.
  The change is verified formatting-only by whitespace-stripped token
  comparison plus rustfmt parse (see
  `/tmp/stage/review-evidence/rustfmt-repair-verification-round-aa6fdd18.txt`).
- `python3 scripts/check-rustfmt-diff.py 233664cf8` (assigned base),
  `4d6759a0b` (job base), and `HEAD^1` — all clean.
- 20 mechanical checks green offline (see
  `/tmp/stage/review-evidence/mechanical-checks-round-aa6fdd18-repair.txt`).

At HEAD `5ef42f5c3` (prior round):

- `cargo test` could NOT run in this repair sandbox — crates.io
  unreachable (connection refused on `index.crates.io:443` and
  `static.crates.io:443`), no cached crate registry, no prebuilt target
  dir; see
  `/tmp/stage/review-evidence/build-restriction-round6107f63d.txt`.
  The new killing test and the two fixes were verified by inspection
  against compiling sibling code paths (same call shapes, imports, and
  port signatures) and by rustfmt parse. Remote verification must run:
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib
  global_spec_approval_skips_unreadable_workspace_set` plus the §8
  suite, and the mutation kill (restore `?` → test fails).
- `python3 scripts/check-rustfmt-diff.py HEAD~1` — clean (new lines
  rustfmt-clean; only pre-existing debt lines differ).
- 20 mechanical checks green offline (see
  `/tmp/stage/review-evidence/mechanical-checks-round6107f63d.txt`).

Prior rounds (at HEAD `381962ab9`, merged base `a884329d7`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — **1356 passed, 0
  failed**. §8 tests in `reconciliation.rs`: tenant propagation creates
  per-workspace tasks; override prevents propagation; force bypasses
  the override; unbound paths do not propagate; blast radius includes
  all non-overriding workspaces (never crossing tenants); merge
  semantics; effective-set SHA reflects inheritance; workspace GET
  returns the effective set; full API flow — Tenant-scoped spec creation
  → tenant-set bind → approval reconciles only the inheriting workspace
  → force approval reaches the overrider; **set-PUT propagation trigger
  (killing)**; **fail-closed override gate (killing)**. `meta_specs.rs`:
  tenant-set GET/PUT roundtrip, Admin-only, tenant containment, unknown
  tenant 404, Tenant-scope creation validation, force 400 on Global
  scope, force 403 cross-tenant.
- Mutation kills (evidence retained under
  `/tmp/stage/review-evidence/`): trigger-call deletion →
  `tenant_set_put_propagates…` FAILED; fail-open gate restored →
  `tenant_reconciliation_fails_closed…` FAILED.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` — **359 passed, 0
  failed** at the prior checkpoint (no adapter code touched this round):
  tenant set repo roundtrip/overwrite/delete/independence (SQLite,
  fresh-migrated DB), Tenant-scoped meta-spec round-trip + scope filter.
- Mechanical checks 19/19 green at HEAD (arch, abac-route-registry,
  abac-exempt-handlers, migration-versions, migration-sql-portability,
  mem-port-contracts, in-memory-state-stores, forged-scope-fields,
  scope-literal-defaults, dead-message-kinds, inert-enforcement,
  mcp-write-tools, byte-slice-truncation, fail-open-ref-resolution,
  relative-path-defaults, lossy-secret-conversion,
  fabricated-scope-defaults, forwarded-header-trust,
  unbounded-external-http; see
  `/tmp/stage/review-evidence/mechanical-checks-round2.txt`).
