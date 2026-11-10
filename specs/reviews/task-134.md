# Review: task-134 — AgentReview and AgentValidation gate types with review protocol

**Reviewer:** Review (independent)
**Round:** R1 (candidate 34b66fe4d)
**Spec ref:** agent-gates.md §Gate Types (Extended), §AgentReview Gate
**Comparison base:** b5a872087899e8bc4a18092ab56990a26e9ca9af
**HEAD reviewed:** 34b66fe4d94894bcb24edb63d84e5674d3c44cfe
**Status:** complete — approved

---

## Findings

None material. The implementation is real production behavior on every
acceptance criterion; the discriminating probes below kill the mutations
they guard against. Evidence retained under `/tmp/stage/review-evidence/`.

### Diff scope

25 files, +2367/−164 vs base: domain `QualityGate.validation_type` +
migration `2026-10-09-000059_gate_validation_type` (next unused sequence;
`check-migration-versions.sh` and `check-migration-sql-portability.sh`
pass), `GitOpsPort::read_file_at_commit` + git2 adapter (unknown SHA is an
error, not `Ok(None)`), SQLite/Postgres quality-gate persistence including
the upsert `.set()` list, gate CRUD (`validation_type` accepted and
returned), `AgentSigningKey::mint_scoped` (jti, scope claim), ABAC
middleware scoped-token route allow-list (exact-match capability
comparison), `git_http` push denial for scoped tokens, `submit_review`
reviewer-identity binding, and the gate executor's real
AgentReview/AgentValidation spawn protocol with teardown.

### Acceptance criteria evidence

- **Gate type variants**: `GateType::AgentReview`/`AgentValidation` exist in
  `gyre-common/src/gate.rs` (pre-existing at base; this task supplies the
  execution semantics that were previously missing) and are now dispatched
  in `run_gate` to real spawn paths.
- **Gate CRUD**: `create_agent_review_gate`/`create_agent_validation_gate`
  exercise POST/GET `/api/v1/repos/:id/gates` including the
  `validation_type` round-trip through the list endpoint;
  `save_upsert_updates_all_config_columns` (SQLite) guards the upsert set
  list (fails on the dropped-column class of drift the checkpoint
  introduced); Postgres row mappings updated symmetrically.
- **Spawn protocol**: `build_review_agent_context` resolves the persona
  nearest-wins (repo → workspace → tenant, workspace-lookup failure skips
  the tenant scope with a log rather than fabricating one), fails the gate
  before minting any token when the persona or MR is unresolvable
  (asserted: no `gate-*` tokens in the kv store), gathers the full diff via
  `git_ops.diff`, reads the spec at the SHA pinned in `spec_ref` via
  `read_file_at_commit` (pre-tip SHA pinned-content test proves the
  reviewer sees the pinned version, not the tip), and carries the MR title
  plus the author agent's task description as the acceptance-criteria
  carrier.
- **Scoped token**: `mint_scoped` emits `review:submit` (reviewers) /
  `validation:report` (validators) with `jti`; TTL derived from
  `gate.timeout_secs + 60` on both mint sites (regression tests assert
  `exp − iat ≥ 700 + 60` for `timeout_secs=700`). Enforcement is layered:
  ABAC route allow-list denies anything outside each capability's routes;
  `git_receive_pack` 403s scoped tokens before repo resolution;
  `submit_review` binds the reviewer identity to the token subject (forged
  `reviewer_agent_id` ignored — the e2e driver posts `forged-id-attempt`
  and the stored review carries `gate-review-*`).
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with
  the review body surfaced in gate output; only the gate's own agent's
  verdict counts; exit-0-without-review fails. Covered by unit tests plus
  two live end-to-end tests (real axum server on loopback, real bare git
  repo, spec pinned at a pre-tip SHA, `review_agent_driver.sh` submitting
  through the live Review API — both ran in this sandbox, not skipped) and
  the always-run oneshot ABAC guard `e2e_review_submission_is_abac_allowed_with_seeded_policies`.
- **Teardown**: token revoked (`kv_remove`) on every exit path — verdict,
  timeout, spawn failure, empty command — asserted by
  `gate_token_revoked_after_review_completes` /
  `gate_token_revoked_after_validation_completes`; revoked tokens fail auth
  with 401 (`revoked_scoped_token_is_rejected`); temp spec/diff files
  removed.

### Mutation probes (this review, source restored and re-verified after each)

1. **ABAC scoped-token allow-list disabled** (`is_reviewer`/`is_validator`
   forced false in `abac_middleware.rs`):
   `scoped_review_token_denied_outside_allowlist` and
   `validation_scoped_token_cannot_submit_review` FAIL (rc=101); the other
   five probe tests still pass because they guard the other enforcement
   layers — exactly the layered-defense shape claimed. Restored; probe
   suite 7/7 again.
2. **Reviewer identity binding disabled** (binding condition forced false
   in `merge_requests.rs`): `scoped_review_token_reviewer_identity_is_bound`
   FAILS with "reviewer identity must be bound to the scoped token
   subject". Restored; tree clean at candidate SHA.

### Test evidence (focused probes, `/tmp/stage/bin/cargo`)

- `cargo test -p gyre-server --lib gate_executor` — 37/37 (e2e pair ran,
  loopback permitted by this sandbox)
- `cargo test -p gyre-server --test task134_review_probe` — 7/7
- `cargo test -p gyre-server --lib` — 1629/1629
- `cargo test -p gyre-server --lib git_http` — 54/54; `merge_requests` —
  33/33; `gates` — 26/26
- `cargo test -p gyre-adapters --lib` — 412/412 (includes the upsert
  round-trip, migration-version guard, and `read_file_at_commit` tests)
- `cargo test -p gyre-domain --lib` — 443/443; `gyre-common` — 120/120
- Mechanical checks all rc=0: check-arch, check-fabricated-scope-defaults
  (freeze tightend 2→1 with the `gate_executor.rs:111` entry removed and
  the production fallback rewritten to skip+log), check-migration-versions,
  check-migration-sql-portability, check-abac-route-registry,
  check-abac-exempt-handlers, check-forwarded-header-trust,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-inert-enforcement, check-unbounded-external-http,
  check-forged-scope-fields, check-relative-path-defaults,
  check-byte-slice-truncation, check-fail-open-ref-resolution,
  check-dead-message-kinds, check-mem-port-contracts, check-mcp-write-tools,
  check-task-commit-attribution.

### Notes (non-blocking)

- The coverage matrix rows for the two assigned sections remain
  `task-assigned`; repo convention has the audit process (dedicated
  `audit(coverage)` commits) flip those statuses, and the task contract
  does not include the matrix update. Not a defect.
- Spec/diff temp files under `std::env::temp_dir()` are created with
  default mode (typically 0644). The content is repo data the configured
  review agent is already entitled to read; a permissive umask on a
  multi-user host could expose it to local users. Hardening nit for a
  future task, not a spec violation.
- The `GitOpsPort::read_file_at_commit` doc does not carry a "Fails if"
  constraint sentence, so the mem adapters' `Ok(None)` stand-ins do not
  violate `check-mem-port-contracts` (rc=0); the gate executor treats
  `Ok(None)` from any adapter as an unresolvable spec_ref and fails closed,
  so the stand-in cannot produce a false pass.

`cargo test --all` and the full mechanical suite are owned by verification
and publication.
