# Review: task-131 — ABAC policy CLI commands

**Reviewer:** Independent review (pi runtime)
**Round:** R1
**Spec ref:** abac-policy-engine.md §CLI
**Comparison base:** 4b9220bca6ccaf8f3a250adc41c40f6bba6727f1
**Candidate reviewed:** 44732bf1c54185b529cc59328835c77bc87116d9 (product commit 05549afd)
**Status:** approved

---

## Findings

None blocking. The implementation is real end-to-end CLI plumbing: every
command goes through the production `GyreClient` reqwest methods against
real HTTP, with request lines, query parameters, JSON bodies, and error
propagation asserted against a real local TCP listener (not a stubbed
transport). Mutation probes confirmed the tests kill behavioral breaks.

## Evidence per acceptance criterion

- **`policy list` with scope/effect columns** — `run_policy_command` →
  `GyreClient::list_policies` (GET /api/v1/policies); table prints
  id/name/scope/scope_id/effect/priority/enabled/built-in.
  `policy_list_flow_sends_scope_filters` asserts the exact request line
  `GET /api/v1/policies?scope=workspace&scope_id=ws-1` over real TCP and
  the bearer-token header.
- **`--scope`/`--workspace-id` filters** — clap `ValueEnum` rejects unknown
  scope values at parse time (tested); `requires = "scope"` on
  `--workspace-id` (tested at parse level) plus a runtime guard that the
  scope is exactly `workspace` (tested at flow level:
  `--scope tenant --workspace-id ws-1` fails before any request is sent).
  Server-side semantics verified against
  `gyre-adapters/src/sqlite/policy.rs::list_by_scope` (scope_id filter /
  IS NULL for all-scope) and `api/policies.rs::ListPoliciesQuery`.
- **`policy create --file policy.yaml`** — serde_yaml definition mirrors
  the server's `CreatePolicyRequest` field-for-field (verified against
  `api/policies.rs`); client-side scope/effect validation before network
  I/O; `policy_create_flow_posts_yaml_body` asserts the exact POST body
  incl. conditions; `policy_create_flow_rejects_reserved_prefix_from_server`
  proves a server 400 (reserved `trust:` prefix) propagates with status and
  message. The reserved-prefix/Admin enforcement correctly stays
  server-side.
- **`policy evaluate`** — POST /api/v1/policies/evaluate with
  `{type}:{id}` flags mapped onto `subject.type/subject.id` and
  `resource.type/resource.id`; the exact JSON body shape is asserted in
  `policy_evaluate_flow_posts_dry_run_request`. Request/response shapes
  verified field-for-field against the server's
  `EvaluateRequest`/`EvaluateResponse` (incl. `PolicyScope`/`PolicyEffect`
  snake_case serialization and `Id` as a plain JSON string). Prints
  decision, matched policy, evaluated count, evaluation time.
- **`policy decisions`** — GET /api/v1/policies/decisions with the
  subject-id filter and limit (`policy_decisions_flow_sends_subject_filter`
  asserts `GET /api/v1/policies/decisions?limit=250&subject_id=worker-42`).
  `--decision`/`--since` are applied client-side to the fetched newest-first
  page (the server endpoint supports subject_id/resource_type/limit only);
  this is documented in `--limit` help and docs/cli.md. Response type
  verified field-for-field against `gyre_domain::PolicyDecision`.
  `filter_decisions` outcome/age matrix unit-tested; malformed
  `--subject`/`--since` rejected before any request (tested).
- **`policy effective`** — GET /api/v1/policies/effective with the
  documented `?subject=&resource_type=` params (exact request line
  asserted; mutation renaming the param to `subject_id` was killed by the
  test). Response type matches the server's `EffectivePermission`.
- **Error handling** — every client method bails with
  `HTTP {status}` + server message; 403 propagation tested
  (`policy_list_flow_surfaces_auth_failure`), 400 propagation tested
  (create reserved-prefix flow).
- **Help text** — verified against the built debug binary: group and all
  five subcommands render usage and per-flag help, including possible
  values and defaults.

## Probes run (evidence: /tmp/stage/review-evidence)

- `cargo test -p gyre-cli policy` — 23 passed, 0 failed.
- `cargo test -p gyre-cli` (full suite) — 178 + 1 passed, 0 failed;
  matches the candidate's claim.
- `cargo fmt -p gyre-cli --check` clean; `cargo clippy -p gyre-cli
  --bins --tests` no warnings (the `--all-targets` spelling is rejected by
  the build wrapper; bin+tests covers all targets of this bin-only crate).
- `scripts/check-arch.sh`, `check-byte-slice-truncation.sh`,
  `check-relative-path-defaults.sh`, `check-inert-enforcement.sh` — all
  pass.
- Mutation kills: (1) removing the `--workspace-id` runtime guard +
  scope_id param → 2 flow tests failed; (2) renaming the effective
  `subject` query param → flow test failed. Both restored; post-restore
  suite green; `git status crates/` clean.

## Deferred to the independent verifier

- `cargo test --all` (final acceptance criterion) — gyre-server build
  exceeds this sandbox's 4 GiB budget. The change touches only gyre-cli,
  docs, and specs; the full gyre-cli suite passes here. The verifier
  should also run the existing server-side `api::policies` tests that own
  the endpoint contracts the CLI calls.

## Non-blocking observations

- The shipped notes state "23 focused tests"; the actual policy-related
  test count is 28 (23 match the `policy` name filter; 5 more
  `parse_subject_ref_*`/`parse_since_duration_*`/`filter_decisions_*`
  tests don't contain "policy" in their names). The understatement is in
  the deflationary direction and the full-suite count (178) is accurate;
  no coverage inflation.
- Table column widths use byte lengths (`String::len`), so multi-byte
  names can misalign columns cosmetically — same pre-existing pattern as
  the other CLI tables; no panic (char-boundary-safe formatting).
