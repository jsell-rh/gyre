# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts–§Policy Examples
(coverage rows 2–7). Candidate `baf2f28533e879eca5a15ab3b17d1629e350614b`
(base `d31906c5a16ab8045ab435dfad2bd7dc1ce3f616`), 47 commits. This is
review round 6; rounds 1–5 findings (fail-open dynamic-ref resolution,
JWT-claim attribute spoofing, spec-path resource.id, normalize-guard,
no-target attestation_level) are all repaired in the candidate and
re-verified below.

Verdict: **approved** (independent evidence supports the task contract; no
blocking findings). Full probe log: `/tmp/stage/review-evidence/summary.md`.

## What was verified with live probes (this round)

- **Policy entity matches spec** (`gyre-domain/src/policy.rs`): every field
  including `scope_id: Option<Id>`, `immutable`, `created_by: Id`;
  `PolicyScope = {Tenant, Workspace, Repo}`, `PolicyEffect = {Allow, Deny}`,
  `Condition`/`ConditionOp` (all 8 variants)/`ConditionValue`
  (String/StringList/Number/Bool). `cargo test -p gyre-domain` → pass.
- **Evaluation engine** — the candidate's real `policy_engine.rs` was
  compiled verbatim in a standalone probe crate (gyre-server codegen exceeds
  this sandbox's 4 GiB budget; the probe avoids it) and its full unit suite
  ran: **34/34 pass**, including all five §Policy Examples encoded verbatim,
  deny-before-allow tie-break, immutable-deny-first, scope-specificity
  ordering, default deny.
- **Dynamic references** (`$resource.*`/`$subject.*`) resolve at evaluation
  time preserving the referenced attribute's type (string/list/number/bool),
  and unresolvable references fail the condition. Mutation probe: replacing
  the resolver's `None` with a fabricated `""` (fail-open) makes
  `dynamic_reference_unresolvable_fails_closed` fail — the test genuinely
  discriminates, it is not self-confirming.
- **Attribute-model spoofing containment** (round-2 repair re-verified):
  `merge_agent_jwt_claims` is allowlisted to the four spec "Agent OIDC claim"
  names (persona, stack_hash, attestation_level, repo_scope), gated on
  `subject.type == "agent"`, and insert-only. Mutation probe: removing the
  allowlist makes both claim-spoofing tests fail. The unrestricted
  `merge_jwt_claims` survives only on the dry-run `POST /policies/evaluate`
  what-if path, where caller-supplied hypotheticals are the point.
- **Port + adapters**: `PolicyRepository` (CRUD + `list_by_scope` +
  `delete_by_name_prefix*`) with SQLite, Postgres, and mem adapters.
  `cargo test -p gyre-adapters --lib policy` → 13/13 pass, including
  `delete_rejects_built_in_policy` (asserts the policy survives a rejected
  delete) and full-field CRUD roundtrip. The built-in delete guard is now
  enforced in code in BOTH SQL adapters (mem-port-contract parity;
  `check-mem-port-contracts.sh` passes). No new migration was needed — the
  `policies` table with JSON conditions/actions/resource_types predates the
  task and the schema is unchanged.
- **Round-5 repair (no-target attestation)**: spawn's mint site now passes
  `Some(map_or(1u8, ...))`, so every spawn-minted worker JWT carries
  `attestation_level` (no-target local subprocess = 1). The regression test
  decodes the minted token with the server's own signing key and asserts
  `Some(1)`; since a resolved Container/Ssh target would mint 3/2, the
  assertion can only pass through the no-target branch. Kubernetes maps to 1
  with an inline task-129 pointer — documented, not silent.
- **Type checking**: `cargo check -p gyre-server` → clean.
- **Mechanical gates at HEAD** (all exit 0): check-arch,
  check-abac-route-registry, check-abac-exempt-handlers,
  check-mem-port-contracts, check-migration-versions, check-mcp-write-tools,
  check-forged-scope-fields, check-forwarded-header-trust (env.ip comes from
  `ConnectInfo`, never forwarded headers), check-scope-literal-defaults,
  check-inert-enforcement, check-unbounded-external-http,
  check-in-memory-state-stores, check-lossy-secret-conversion,
  check-fabricated-scope-defaults, check-fail-open-ref-resolution,
  check-relative-path-defaults, check-byte-slice-truncation,
  check-dead-message-kinds, check-migration-sql-portability,
  check-task-commit-attribution (all 7 task-labeled commits recorded in the
  candidate's frontmatter; an apparent failure was the stage harness's
  working-tree revert of the task file, not the committed candidate).
- **Coverage rows 2–7** flipped to `implemented` with per-row evidence notes
  that match what was probed above (including the honest disclosure that
  `resource.team`/`resource.visibility` remain unpopulated because no entity
  carries those fields yet, with fail-closed semantics meanwhile).

## Deferred to the verifier (sandbox build-budget restriction, not defects)

gyre-server test codegen is blocked in this review sandbox (4 GiB cap). The
verifier must run:

- `cargo test -p gyre-server --lib policy_engine`
- `cargo test -p gyre-server --lib abac_middleware`
- `cargo test -p gyre-server --lib spawn_without_compute_target_mints_attestation_level_one`
- `cargo test --all` (task acceptance criterion), plus clippy and the web suite

The engine-logic subset of these was executed here via the verbatim-source
probe crate (34/34) with two mutation kills; the remaining risk is
integration-level only.

## Non-blocking observations

- `orchestrator_persona(Worker)` returns the slug `"worker"`, which is not
  one of the four built-in persona entities. The claim is a tier label for
  ABAC `subject.persona`, not a store-validated persona binding; worker-tier
  spawns via `spawn_orchestrator` resolve their prompt persona separately
  (fail-closed `resolve_orchestrator_persona`). Cosmetic at most.
- `merge_jwt_claims` (unrestricted) remains reachable only from the dry-run
  evaluate endpoint; its doc comment states the constraint. Fine as shipped.
