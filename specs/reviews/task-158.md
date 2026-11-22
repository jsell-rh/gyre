# Review — task-158 (Implement tenant-scope meta-spec changes)

Spec: `specs/system/meta-spec-reconciliation.md` §8 Tenant-Scope Meta-Spec Changes.
Candidate under review: `5b43745f16e6cab4a8dc4fc9e171f5911fd07783` (branch
`pipeline/task-158/3c0f17fc02a744c8b94b617f58af158c-1`, base
`73c504f2b5cfb30dd4670e06a0d90c97a1af03a1`; merge `5d736486`, docs `e3e45bd3`,
process `5b43745f`).
Verdict: **approved** — both durable findings from round `5a3602cb8e834b5788caea63540edcf7`
are closed in the candidate tree; no new defects found.

## Round 2 (flat-route repair round)

Nature of the round: the previous candidate (`da59ef98`) was flagged because the
assigned base had since carried the task-301 flat-route rename, while the §8
approval trigger/tests and the checked-in spec text still named the old
`/api/v1/meta-specs-registry/{id}` prefix. The repair merges the base; because
the §8 product work was already reconciled into main and task-301's verified
checkpoint (`c5a157de`) re-pointed the §8 trigger and tests to the flat routes,
the merged tree resolves to exactly the base tree. Verified independently:

- `git diff 73c504f2..5b43745f` touches **only** `specs/tasks/task-158.md`
  (docs). The product-surface diff (`crates/`, `web/src`, `web/tests`,
  `scripts/`, `docs/`, `specs/system/`, `specs/coverage/`) is **empty**
  (0 bytes) — no Rust behavior differs from the independently verified base,
  so runtime execution is deferred to the verifier (docs-only round; no cold
  compile of unchanged behavior).
- `git rev-list --count 73c504f2..da59ef98` = 0 — the previously reviewed
  candidate is fully contained in the base lineage; the merge dropped nothing.
- All five task-158 product commits (`3c22bfa8`, `381962ab`, `5ef42f5c`,
  `b7eaa775`, `17fc6bf7`) are ancestors of the candidate.

### Finding 1 (approval trigger at flat route) — closed

- `PUT /api/v1/meta-specs/:id` is registered
  (`crates/gyre-server/src/api/mod.rs:984-987`, handler
  `update_meta_spec_registry`) and has an ABAC resolver entry
  (`abac_middleware.rs:783`). The old `/meta-specs-registry` prefix appears
  only inside the test `registry_old_prefix_removed_no_alias`
  (`meta_specs.rs:12613-12643`), which asserts the old prefix 404s — no
  silent alias.
- The §8 force gate is real enforcement (`meta_specs.rs:9105-9125`): 400 when
  `force` is set on a non-Tenant scope, 403 when the caller is not an admin of
  the scope tenant, Internal on a Tenant scope with missing `scope_id`; force
  then propagates into `run_tenant_reconciliation` (`meta_specs.rs:9217-9231`).
- The §8 tests hit the flat routes: `reconciliation.rs:5614/5644` use
  `/api/v1/meta-specs/{ms_id}` for both the approval wave and the force wave;
  zero old-prefix URIs remain in `reconciliation.rs`. Force validation tests:
  `force_rejected_for_non_tenant_scoped_spec` (12738),
  `force_requires_admin_of_the_scope_tenant` (12785).
- The §8 suite asserts real behavior through the router: propagation creates
  exactly one task on the non-overriding workspace (title checked), the
  overriding workspace is skipped, and the force wave reaches the overrider
  (title checked) — killing tests, not structural.

### Finding 2 (spec text) — closed

`specs/system/meta-spec-reconciliation.md` §8 step 4 now reads
"`PUT /api/v1/meta-specs/{id}` when approving a Tenant-scoped entry" — matches
the authoritative requirement input text exactly (direct file comparison).

### Acceptance criteria re-confirmed in the merged tree

- `MetaSpecScope::Tenant` variant (`gyre-domain/src/meta_spec.rs:64`, tenant id
  in `scope_id`; migration `2026-11-07-000059` widened the scope CHECK and
  created `tenant_meta_spec_sets`).
- Tenant meta-spec set managed via `GET/PUT /api/v1/tenants/:id/meta-specs/set`
  (`api/mod.rs:821`, ABAC entry `abac_middleware.rs:605`;
  `TenantMetaSpecSetRepository` port `gyre-ports/src/meta_spec_set.rs:34` with
  mem/SQLite/Postgres adapters).
- Inheritance with override: `resolve_effective_meta_spec_set`
  (`reconciliation.rs:1455`); consumers (workspace set GET, effective-set SHA,
  blast radius with `tenant_inheritance` reason, preview, `meta_spec_changed`
  fan-out) all resolve the effective set.
- Per-workspace reconciliation: `run_tenant_reconciliation`
  (`reconciliation.rs:1562`) with fail-closed override gate; triggers are the
  tenant-set PUT diff and Tenant-scoped registry approval; force on the
  tenant-set PUT reaches `run_tenant_reconciliation` (`meta_specs.rs:508`).
- Coverage matrix row 12 (§8) reads `implemented` with an accurate summary.

### Mechanical checks (run in this review, all exit 0)

- `bash scripts/check-arch.sh` — passed.
- `bash scripts/check-abac-route-registry.sh` — passed.
- `bash scripts/check-task-commit-attribution.sh` — passed (docs/process
  commits are not product surface; the merge commit is excluded by design; the
  five product commits are listed in the frontmatter).
- Evidence under `/tmp/stage/review-evidence/` (git-identity, old-prefix grep,
  route registration, mechanical-check logs).

Working-tree note: the uncommitted modification to `specs/tasks/task-158.md`
(progress → complete, round-notes strip) is the executor's protected-task-text
restoration — the review guard explicitly excludes that file from its hash set;
it is not part of the product diff.

### Verifier must run

(Runtime execution deferred: this round's authored diff is docs-only and the
product surface is byte-identical to the verified base.)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — §8 tenant suite in
  `reconciliation.rs` (propagation, override skip, force, unbound-path gate,
  blast radius, merge semantics, SHA inheritance, effective-set GET, set-PUT
  trigger, fail-closed override gate) and `meta_specs.rs` (tenant-set endpoint,
  flat-route approval + force 400/403, old-prefix no-alias 404s).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` — tenant set repo
  roundtrip + `000059_tenant_meta_spec_sets` migration on a fresh DB.
- Full workspace Clippy all-targets gate (`scripts/check-clippy-diff.py`).
