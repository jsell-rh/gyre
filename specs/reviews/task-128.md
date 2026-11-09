# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy
Language, §Policy Entity, §Conditions, §Policy Examples.
Candidate under review: `a6b8c10c7fe8fc6c498ed1d692d5e0c81dc73c4f` (base
`fefde2946f3a0af1f212588f54591516fee0f7b2`). This is the review round after
the implementation's own repair rounds 1 (d779f1e89, path-attribute leakage)
and 2 (8d5494e94, JWT-claim spoofing of membership-sourced subject
attributes).

Verdict: **approved**.

## What was verified (independent evidence, transcripts under /tmp/stage/review-evidence/)

### Spec-conformance of the entity and condition model

- `gyre-domain/src/policy.rs`: `Policy` carries every spec field (id, name,
  description, scope, `scope_id: Option<Id>`, priority, effect, conditions,
  actions, resource_types, enabled, immutable, created_by, created_at,
  updated_at) plus `built_in` (required by the spec's §Built-In Policies
  delete guard). `PolicyScope` = {Tenant, Workspace, Repo};
  `PolicyEffect` = {Allow, Deny}; `ConditionOp` has all 8 spec variants;
  `ConditionValue` = {String, StringList, Number, Bool} plus `Null` as the
  value-side placeholder for `Exists`. Matches the spec structurally,
  field-for-field.
- `PolicyRepository` port (create/find_by_id/list/list_by_scope/update/delete/
  delete_by_name_prefix/delete_by_name_prefix_and_scope_id/record_decision/
  list_decisions) with a real SQLite adapter (Diesel against the `policies`
  table, conditions/actions/resource_types as JSON) and a mem adapter; the
  port contract "cannot delete built-in policy" is enforced in code in BOTH
  adapters (SQLite `delete` re-checks `built_in` inside the txn;
  `check-mem-port-contracts.sh` passes). Adapter roundtrip test
  (`policy_crud_roundtrip_preserves_fields`) proves the pre-existing schema
  covers all fields — no new migration was needed (the `policies` table and
  the `immutable` column predate the base; the candidate added no migration
  files, and `check-migration-versions.sh` passes; the duplicate 000056/000057
  sequence numbers visible in `ls` are pre-existing base debt, present in the
  base tree, and the repo's version check accepts them).
- Evaluation engine (`policy_engine::evaluate`): immutable Deny first, then
  priority order with scope specificity (repo > workspace > tenant) and
  Deny-before-Allow tie-break, then default deny. Condition matching is
  strictly typed (no cross-type coercion that could accidentally match);
  missing attributes and unmatched type combinations evaluate to false
  (fail closed).
- All five §Policy Examples are encoded as verbatim evaluation tests with
  positive and negative controls (`spec_example_agent_repo_scope` including
  the `$resource.repo_id` dynamic reference, `spec_example_persona_management`,
  `spec_example_viewer_no_spawn`, `spec_example_gate_approved_persona`,
  `spec_example_default_deny`) — checked against the spec YAML line by line.

### Round-2 fix (claim spoofing) — code read + mutation-verified

- Live-path claim merging is `merge_agent_jwt_claims`: allowlisted to exactly
  the four spec agent-claim names (persona, stack_hash, attestation_level,
  repo_scope), insert-only, and the whole merge+normalize block in
  `abac_middleware` is gated on `subject.type == "agent"` (so a human OIDC
  token's claims can never define persona/attestation_level either).
  Membership-sourced names (workspace_role, workspace_ids, team_ids) are not
  in the allowlist at all — the membership/team stores are their only source.
- The generic insert-only `merge_jwt_claims` has exactly one remaining
  caller: the dry-run evaluation API (`api/policies.rs::evaluate_policy`),
  which builds a fresh hypothetical context and makes no access decision.
- Killing tests are real end-to-end tower oneshot tests through the live
  middleware with test-signed JWTs, real state stores, and policies placed
  above the builtin role allows (non-vacuous controls).

### Mutation probes (all transcripts in /tmp/stage/review-evidence/)

- **A (pre-fix ungated generic merge)** — replaced the gated
  `merge_agent_jwt_claims` block with an ungated `merge_jwt_claims` call:
  `workspace_role_claim_cannot_spoof_membership_attribute` and
  `human_persona_claim_cannot_define_agent_attribute` FAIL (200 instead of
  403). Killed.
- **B (post-extraction overwrite of workspace_role from a claim)** —
  `viewer_with_admin_claim_still_trips_notin_deny` FAILS (200 instead of
  403). Killed.
- **C (fail-open dynamic-reference resolver fabricating `""`)** —
  `dynamic_reference_unresolvable_fails_closed` FAILS. Killed.
- **D (round-1 repos-prefix guard removed, `resource_type != "repo"` →
  `false`)** — `repos_parented_collection_route_does_not_leak_parent_repo_id`
  FAILS. Killed.
- All four mutants were reverted; the working tree was restored byte-clean
  (`git status` empty, HEAD `a6b8c10c`) and the six killing tests re-run green
  on the restored tree.

### Test runs (candidate `a6b8c10c`)

- `cargo test -p gyre-server --lib` → **1516 passed / 0 failed**
  (includes `abac_middleware` 32, `policy_engine` 34).
- `cargo test -p gyre-server` (all targets: lib + the 9 integration binaries)
  → **1516 + 66 + 21 + 3 + 1 + 1 + 15 + 16 + 38 + 8 passed / 0 failed**.
- `cargo test -p gyre-adapters` → **387 passed / 0 failed / 12 ignored**
  (includes the 4 new sqlite policy tests).
- `cargo test -p gyre-domain` → 444 passed / 0 failed.
- Full workspace `cargo test --all` is left to verification (the review
  wrapper rejects workspace-wide invocations); every package the candidate
  touched was run per-package and is green.

### Mechanical invariants at the candidate

`check-abac-route-registry` (registered routes resolve; duplicate count at
frozen baseline), `check-arch`, `check-mem-port-contracts`,
`check-fabricated-scope-defaults`, `check-forged-scope-fields`,
`check-forwarded-header-trust`, `check-inert-enforcement`,
`check-scope-literal-defaults`, `check-migration-versions` — all pass. The
exemption-file edits in the diff (`abac-exempt-handlers-exemptions.txt`,
`inert-enforcement-exemptions.txt`, `scope-literal-defaults-exemptions.txt`)
are pure line-number shifts from code motion above the exempt sites — no new
exemption entries.

### Coverage honesty

Rows 2-7 of `specs/coverage/system/abac-policy-engine.md` flipped to
`implemented` with evidence notes that match what I independently verified.
Row 3 explicitly records `resource.team` and `resource.visibility` as
UNPOPULATED (no entity type carries those fields; conditions on them fail
closed) instead of overstating the entity-lookup extraction — the round-2
process finding is repaired.

## Notes (non-blocking)

- `normalize_agent_jwt_attributes`'s doc comment says "Never overwrites an
  attribute already present" while the code uses `ctx.set` unconditionally.
  In practice only claim-sourced names gated behind the agent check are
  affected (no trusted value exists for those keys when it runs, and the
  only prior writer is the allowlisted merge itself), so this is a doc
  inaccuracy, not a spoofing channel. Cosmetic; not worth a revision round.
- `mint_scoped` falls back to a UUID token if JWT minting fails
  (`api/spawn.rs`) — pre-existing pattern (the fallback predates the
  candidate), and the UUID token still authenticates via the registered
  `agent_tokens` path. Not a candidate regression.
- Duplicate migration sequence numbers 000056/000057 exist in the base and
  candidate alike (pre-existing debt outside this task's scope; the repo's
  own `check-migration-versions.sh` passes as-is).
