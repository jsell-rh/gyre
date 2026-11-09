# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts through §Policy Examples (coverage rows 2-7).
Candidate under review: `0a1a3cd5a35ec6b585831afa23097f67e41a5dff` (round-3 repair head `5327b8daa` + process record), base `fb1f82e31b86f71c4792c9afd6c2b228dff965f2`.
Verdict: **approved** (round 4; rounds 1-3 findings verified fixed).

## Round-3 finding verification

Both prior-round findings are genuinely fixed, each pinned by a killing test
(re-verified by fresh mutation probes at this head — prior-round probe
transcripts did not survive into this stage):

- **Spec-path `resource.id`** — `extract_path_attributes` now derives
  `resource.id` from a `:path` param via `percent_decode` (bounds-checked,
  UTF-8-validated, invalid encoding → unset with a warning, fail closed).
  All `:path` routes in `api/mod.rs` are spec/meta-spec typed, so the
  late-set `resource.id` cannot clobber an `{resource_type}_id`-derived id.
  `spec_resource_id_from_decoded_path_reaches_live_evaluation` runs a Deny at
  810 keyed on `resource.id Equals "system/design.md"` against
  `GET /api/v1/specs/system%2Fdesign.md` (403 fixed / 200 with the defect —
  the raw encoded value would not match, so the deny firing also proves
  decoding) plus a retargeted-Deny control (200), proving the attribute
  carries the addressed spec's path. Mutation probe: disabling the
  population fails both the live test and the unit test
  (`path_attributes_path_param_names_addressed_spec`).
- **`normalize_agent_jwt_attributes` doc/code divergence** — the three
  setters now carry `contains_key` guards matching the documented
  "never overwrites" invariant; pinned by
  `normalize_agent_jwt_attributes_never_overrides_existing_attributes`.
  Mutation probe: removing the guards fails the test.

## Independent checks (evidence: /tmp/stage/review-evidence, INDEX.md)

- `cargo test -p gyre-server --lib` → **1570 passed / 0 failed** at the exact
  candidate (matches the shipped claim); `abac_middleware::` 35, `policy_engine::` 34.
- `cargo test -p gyre-domain` → 8 passed; `cargo test -p gyre-adapters --lib -- policy`
  → 9 passed (incl. built-in delete rejection and CRUD roundtrip).
- **Round-2 anti-spoofing re-probe**: reintroducing an ungated raw
  `merge_jwt_claims` for human subjects (the pre-round-2 defect) fails both
  `workspace_role_claim_cannot_spoof_membership_attribute` and
  `human_persona_claim_cannot_define_agent_attribute` — the allowlist +
  agent-gate enforcement is live, not theater.
- All mechanically-enforced invariant scripts pass at this head: check-arch,
  check-abac-route-registry (frozen duplicate baseline), check-abac-exempt-handlers
  (88), check-fabricated-scope-defaults, check-forged-scope-fields,
  check-forwarded-header-trust, check-inert-enforcement,
  check-scope-literal-defaults, check-mem-port-contracts,
  check-lossy-secret-conversion, check-relative-path-defaults,
  check-byte-slice-truncation, check-fail-open-ref-resolution,
  check-task-commit-attribution. No `scripts/` or frozen exemption files
  changed in the base→candidate diff.

## Spec conformance (rows 2-7)

- **Policy Entity** — `Policy` matches the spec struct verbatim (all fields
  incl. `scope_id: Option<Id>`, `immutable`, `created_by: Id`) plus the
  documented `built_in` for the §Built-In Policies delete guard;
  `PolicyScope` = {Tenant, Workspace, Repo}; `PolicyEffect` = {Allow, Deny};
  `ConditionOp` has all 8 variants; `ConditionValue` = String/StringList/
  Number/Bool (+ `Null` placeholder for `Exists`).
- **PolicyRepository** — CRUD + `list_by_scope` + prefix deletes + decision
  audit; built-in delete guard enforced in code in ALL THREE adapters
  (SQLite, Postgres, mem — `check-mem-port-contracts` green).
- **Evaluation** — immutable Deny first, then priority/scope-specificity/
  Deny-tiebreak ordering, then default deny; type-mismatched conditions and
  unresolvable dynamic references fail closed (`resolve_condition_value`
  returns `None` → condition false; no fabricated `""`).
- **Attributes** — subject identity/membership/team attributes from auth +
  stores (never claim-definable); the four agent-claim names merged only for
  agent subjects; resource identity from the request path (incl. the round-3
  decoded `:path`); entity-lookup attributes from real store lookups;
  `resource.team`/`resource.visibility` honestly disclosed as unpopulated in
  the coverage row (conditions fail closed meanwhile); env attributes from
  real systems (ConnectInfo peer, budget store, forge pause state, clock).
- **Policy Examples** — all five §Policy Examples encoded as verbatim
  evaluation tests with positive/negative controls (incl. the
  `$resource.repo_id` dynamic reference in agent-repo-scope).

No new findings. `cargo test --all` (integration/e2e binaries) and GitHub CI
remain verification/publication gates.
