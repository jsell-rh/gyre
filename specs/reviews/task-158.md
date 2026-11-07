# Review — task-158 (Implement tenant-scope meta-spec changes)

Spec: `specs/system/meta-spec-reconciliation.md` §8 (amended in-scope via spec
lifecycle to the shipped binding model — the amendment matches the task's own
implementation plan, which specified the tenant-set + inheritance + override
+ force model). Task: `specs/tasks/task-158.md`.
Candidate under review: `de7adce4bf63fb5f819d11049f88204776972880` on
`pipeline/task-158/275ad5fa2d7741289290af13004e9165-1`, diffed against the
assigned base `4d6759a0bfd637e899bbf65e725f91b07f745c69`. Prior review rounds
(cd6d7fac, 6107f63d, aa6fdd18) repaired findings on earlier candidates; this
round re-verifies the final state end-to-end **with real test execution** —
prior rounds' sandboxes could not reach crates.io, so their Rust checks were
deferred to remote verification. This sandbox reached crates.io, so every
deferred check was run here.

Verdict: **approved** (no findings). Evidence under
`/tmp/stage/review-evidence/` (this round's files suffixed `-de7adce4`).

## Test execution (this checkout, HEAD = candidate)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1367 passed, 0
  failed** (`full-server-suite-de7adce4.txt`). Prior rounds deferred this to
  remote verification; now actually run.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` → **359 passed, 0
  failed** (`full-adapters-suite-de7adce4.txt`), incl. SQLite
  `tenant_meta_spec_sets` roundtrip/overwrite/delete/independence-from-
  workspace-set and `tenant_scoped_meta_spec_round_trip` (fresh-migrated DB
  through migration 000057).
- §8 suite in `reconciliation.rs`: tenant propagation creates per-workspace
  tasks (never crossing tenants); override skip; force bypass; fail-closed
  override gate on unreadable workspace set; unbound-path gate; blast radius
  with `tenant_inheritance`/`workspace_binding` reasons; merge semantics;
  effective-set SHA inheritance; workspace GET returns effective set;
  Tenant-scoped approval → per-workspace reconciliation (and force reaches
  the overrider); set-PUT propagation trigger (killing) — all pass
  (`tenant-tests-de7adce4.txt`).
- API tests in `meta_specs.rs`: tenant-set GET/PUT roundtrip, Admin-only,
  tenant containment, unknown tenant 404, Tenant-scope creation requires an
  existing tenant, force 400 on non-Tenant scope, force 403 cross-tenant,
  global approval skips unreadable workspace set — all pass.

## Mutation kills (each edit reverted; `git status --porcelain` clean after)

1. Delete the `run_tenant_reconciliation` call in `put_tenant_meta_spec_set`
   (replace with `TenantReconciliationSummary::default()`) →
   `tenant_set_put_propagates_approved_pins_to_inheriting_workspaces`
   **FAILED** (`mutation1-put-trigger-kill.txt`).
2. Fail-open the override gate (restore
   `load_workspace_set(...).await.unwrap_or_default()`) →
   `tenant_reconciliation_fails_closed_on_unreadable_workspace_set`
   **FAILED** (`mutation2-failopen-gate-kill.txt`).
3. Restore the `?` error propagation in the Global/Workspace approval
   fan-out (the round-6107f63d fix, whose kill was deferred to remote
   verification) → `global_spec_approval_skips_unreadable_workspace_set`
   **FAILED** (`mutation3-global-fanout-kill.txt`).

All three killing tests are genuinely anchored to the production behavior.

## Mechanical gates (at HEAD, against assigned base `4d6759a0b`)

- 19/19 standing checks green (arch, abac-route-registry,
  abac-exempt-handlers, migration-versions, migration-sql-portability,
  mem-port-contracts, in-memory-state-stores, forged-scope-fields,
  scope-literal-defaults, dead-message-kinds, inert-enforcement,
  mcp-write-tools, byte-slice-truncation, fail-open-ref-resolution,
  relative-path-defaults, lossy-secret-conversion,
  fabricated-scope-defaults, forwarded-header-trust,
  unbounded-external-http) — `mechanical-checks-round-de7adce4.txt`.
- `python3 scripts/check-rustfmt-diff.py 4d6759a0b` → clean (17 files).
- `bash scripts/check-task-commit-attribution.sh` → OK.
- `fabricated-scope-defaults-exemptions.txt`: 13 lines before and after —
  same site re-pinned (specs.rs line 959→957, rustfmt line shift only,
  disclosed in the repair commit message). No exemption growth.

## Verified by inspection

- **Tenant scope**: `MetaSpecScope::Tenant` in `gyre-domain` (scope entity in
  `scope_id`, mirroring Workspace); migration 000057 rebuilds `meta_specs`
  with the widened CHECK (portable three-table rebuild, children dropped
  before parent, parents copied before children; down.sql narrows back and
  deletes Tenant rows first); creation validates the scope tenant exists;
  Tenant-scope listing requires caller-tenant containment.
- **Tenant set port/adapters**: `TenantMetaSpecSetRepository`
  (get/upsert/delete) with mem, SQLite, and PostgreSQL implementations in a
  separate table from workspace sets (id-string shadowing impossible;
  adapter test pins independence). Wired through `AppState` via the `store!`
  pattern — no bare in-memory state.
- **API auth**: `GET/PUT /api/v1/tenants/{id}/meta-specs/set` is
  Admin-only + tenant containment per handler, route registered in the ABAC
  resolver (`meta_spec` resource). `force` on registry update is 400 for
  non-Tenant scopes and 403 for an admin outside the scope tenant.
- **Inheritance consistency**: `workspace_overrides_path` is exactly the
  complement of "the tenant pin survives the merge" — persona entries are
  dropped iff the workspace pins the path anywhere or rebinds the role key;
  vec sections append tenant entries iff the path is not workspace-pinned.
  "Not overridden" always means "inherits the tenant pin", as the doc
  comment claims.
- **SHA stability**: `compute_meta_spec_set_sha` now hashes the effective
  set's canonical serialization; for a tenant-less workspace the merge is
  content-identical to the stored set (same BTreeMap personas, same vec
  order), so pre-existing SHAs stay valid — confirmed by the sweep tests,
  which assert SHA equality across re-PUTs through the new code path.
- **Blast radius / cross-tenant enumeration**: the `_auth`
  (authenticated-only) shape of `GET /meta-specs/{path}/blast-radius` is
  pre-existing at the base — unchanged by this candidate, which only
  swapped the set-resolution logic inside. Cross-tenant registry
  create/update without scope-tenant containment is likewise the pre-existing
  base pattern (the candidate adds containment for `force` specifically).
  Not findings against this diff.

## Non-blocking observations (recorded, no action required)

- `compute_meta_spec_set_sha` maps a corrupt-set resolution error to the
  default (empty) set and hashes it, while its log line says "treating as
  unbound" — the resulting SHA is non-empty (doc/comment nit). Behavior is
  deterministic, identical for the approval-time and evaluation-time
  producers, and drift-flagging (fail-closed direction); the pre-existing
  code hashed raw corrupt JSON bytes, which had the same property.
- The blast-radius endpoint and `emit_meta_spec_changed` treat an unreadable
  workspace set as absent (`.ok()`/`unwrap_or(false)`) — advisory surfaces
  only; every enforcement/propagation path (`run_tenant_reconciliation`,
  the approval fan-out) fails closed with skip+warn, and that gate is
  mutation-verified above.

## Acceptance criteria

All seven checked and evidenced: Tenant variant (domain + migration +
round-trip test); tenant-set API (GET/PUT with auth tests); inheritance with
override (merge + SHA + GET tests); tenant approval triggers per-workspace
reconciliation (API-flow test, trigger mutation-killed); overriders skipped
(test); force bypasses (test, both trigger surfaces); tests cover
inheritance/override/force — and the killing tests were re-verified by
mutation at this candidate.
