# Review: task-158 — Tenant-scope meta-spec changes

**Reviewer:** Independent review (pi runtime)
**Round:** R1 (candidate da59ef981c33cf71c952a8a970620c3e856d2a2c)
**Spec ref:** meta-spec-reconciliation.md §8 Tenant-Scope Meta-Spec Changes
**Comparison base:** cba416b46a73d3568f745d7b4eae49a0700a90a6
**Status:** complete — approved

---

## Verdict

Approved. All seven acceptance criteria are implemented in production code,
wired through real ports/adapters/storage, with adversarial tests that call
production functions and assert real side effects. Evidence under
`/tmp/stage/review-evidence/` (see README.md there for full command/exit
transcripts).

## Acceptance-criteria evidence

### MetaSpecScope includes Tenant variant

`gyre-domain/src/meta_spec.rs`: `MetaSpecScope::Tenant` added with
`as_str`/`parse` support; tenant id rides `scope_id` (mirrors the Workspace
design, so the enum stays a plain string column). Migration
`2026-11-07-000059_tenant_meta_spec_sets` widens the `meta_specs.scope`
CHECK via a portable three-table rebuild (parent copied before children so
FK checks pass on both backends; `check-migration-sql-portability.sh` exit 0)
and creates `tenant_meta_spec_sets`. Runtime proof:
`sqlite::meta_spec::tests::tenant_scoped_meta_spec_round_trip` passes on a
fresh migrated SQLite DB (re-run this review, exit 0).

### Tenant-level meta-spec sets managed via API

`GET/PUT /api/v1/tenants/{id}/meta-specs/set` (`api/meta_specs.rs`), backed
by the new `TenantMetaSpecSetRepository` port (`gyre-ports/meta_spec_set.rs`)
with mem, SQLite, and PostgreSQL adapters (separate table from workspace
sets — a tenant and workspace sharing an id string cannot shadow each
other; adapter test `tenant_set_is_independent_of_workspace_set` pins it).
Route registered in the ABAC resolver (`abac_middleware.rs:455`) with the
same resource mapping as the workspace set route; per-handler
authorization: Admin role + caller-tenant containment, existence 404.
Tests: roundtrip (asserts persistence through the port), developer-role
403, cross-tenant 403 (GET and PUT), unknown-tenant 404.

### Inheritance with override support

`resolve_effective_meta_spec_set` / `merge_meta_spec_sets`
(`reconciliation.rs`): tenant defaults merged with workspace overrides —
workspace pin wins on conflict, persona keys rebound by the workspace drop
the tenant pin. `workspace_overrides_path` is provably consistent with the
merge (a path not overridden is bound exactly as the tenant binds it).
Every consumer resolves the effective set: workspace set GET/PUT responses,
`compute_meta_spec_set_sha` (§9 constraint input, §10 sweep input;
byte-identical serialization for tenant-less workspaces, so recorded SHAs
survive the upgrade), the blast-radius endpoint, preview blast radius,
`emit_meta_spec_changed` fan-out, and prompt assembly band 1 (required
Tenant-scoped specs for the task workspace's tenant).

### Tenant meta-spec approval triggers per-workspace reconciliation

`update_meta_spec_registry`: approval of a Tenant-scoped entry fans out via
`run_tenant_reconciliation`, which lists the tenant's workspaces
(`list_by_tenant`) and runs one §6 `run_reconciliation` wave per
non-overriding workspace. A second trigger is the tenant-set PUT diff
(`diff_tenant_meta_spec_set`; removals are not triggers). API test
`tenant_spec_approval_triggers_per_workspace_reconciliation` drives the
whole flow over HTTP (create → tenant-set PUT → approval → force re-approval)
and asserts the per-workspace task counts and titles.

### Overriding workspaces skipped

`workspace_overrides_path` gate; `tenant_reconciliation_skips_overriding_workspace`
asserts the overriding workspace gets zero tasks while its sibling
reconciles. The gate fails closed: a workspace whose own set cannot be read
(storage error or corrupt JSON) is skipped and counted
(`tenant_reconciliation_fails_closed_on_unreadable_workspace_set` — tested
with force=true, the strongest case), never decoded as "overrides nothing".

### Force flag bypasses workspace rollout policies

`force` on the tenant-set PUT and on Tenant-scoped registry approval
bypasses the override skip. Authorization is real: 400 on non-Tenant
scopes (validated before the update persists), 403 when the caller is not
an admin of the scope tenant, PUT-set force additionally behind Admin +
tenant containment. `tenant_reconciliation_force_bypasses_workspace_override`
and the force leg of the approval test prove propagation past the override.

### Tests

The §8 suite (12 tests in `reconciliation.rs`, 7 in `meta_specs.rs`) drives
production entry points (HTTP handlers, `run_tenant_reconciliation`) and
asserts real persisted state (`state.tasks`, response bodies). The
fail-closed tests are adversarial (corrupt-JSON store, force=true), and
`tenant_set_put_propagates_approved_pins_to_inheriting_workspaces` is a
documented kill-test for the set-PUT trigger (a fresh workspace is added
before the removal-only re-PUT so dedup cannot mask a spurious trigger).

## Spec lifecycle note

Spec §8 was amended (documented in the diff): the never-shipped
`POST /api/v1/tenant/meta-spec-rollout` waves sketch is replaced by the
binding-model realization (tenant set + inheritance + override skip +
force), preserving §8's four numbered steps verbatim. This matches the
task's own Implementation Plan and acceptance criteria (which never
included a rollout-plan endpoint or waves) and the goal's rule to amend
outdated specs rather than ship contradicting code. Staged/selective
rollout shapes map onto the binding model (incremental set updates;
override-skip), documented in the amended section. Coverage matrix row 12
updated to `implemented` with an accurate description of what shipped.

## Probes run this review

- 26 standing mechanical gate scripts: all exit 0 against the candidate
  (attribution checked against the candidate's committed file content; the
  working-tree task file was reverted to base by the controller, restored
  unchanged after probing).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib`: exit 0.
- `cargo test -p gyre-adapters --lib meta_spec_set` / `meta_spec::`: 9 and
  7 passed on fresh migrated SQLite DBs (migration 000059 exercised).
- `cargo test -p gyre-domain`: 474 passed.
- Prior-round durable finding (trailing whitespace in the task record,
  the only failure of the full gate suite at 96c3304ce) verified fixed by
  the candidate's final commit; task-158 product files verified untouched
  since the last product commit (17fc6bf7a) — only other-task merges and
  process commits followed.

## Non-blocking observations (pre-existing at base, not introduced here)

- `GET /api/v1/meta-specs/{path}/blast-radius` enumerates workspaces/repos
  across all tenants to any authenticated agent. Identical posture at the
  base; the task only added inheritance semantics and reason labels.
  Worth a follow-up task if cross-tenant enumeration is considered a leak.
- Six awk-based test-quality scripts (self-confirming, mirrored-logic,
  assertionless, verification-completeness, early-return-side-effects,
  transactional-side-effects) use gawk-only syntax and cannot run under
  this sandbox's mawk; manual assessment of the §8 tests found no
  self-confirming or assertionless patterns. Verifier may re-run under
  gawk.

## Verifier must run

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` (§8 suite + meta-spec
  API tests; full list in /tmp/stage/review-evidence/README.md)
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib`
- rustfmt + clippy changed-line gates
