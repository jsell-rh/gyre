# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts → §Policy Examples (coverage rows 2–7).
Candidate: `043f6485db10f70d35d7cd7ab692edfa3659a7c1` (base `65f71e657025677bc39bb349a214ad4099439065`).
This is round 5; rounds 1–4 findings (parent repo-id leak, JWT claim spoofing, spec-path
resource.id, MR/agents parent-id leak) were repaired in commits d779f1e8, 8d5494e9, 5327b8da,
16e740051 and are re-verified below.

Verdict: **approved** (with deferred verifier probes listed at the end).

## Evidence

All commands run in this review sandbox; logs and artifacts under
`/tmp/stage/review-evidence/` (note: the shared `/tmp/gyre-build` target dir is owned by the
build wrapper).

- `cargo test -p gyre-domain policy` → **6 passed** (trust-policy field-level assertions).
- `cargo test -p gyre-adapters sqlite::policy` → **4 passed** (CRUD roundtrip preserving every
  field incl. conditions JSON, delete-rejects-built-in with survival assertion,
  list_by_scope filtering, delete_by_name_prefix scope isolation) against real SQLite
  (NamedTempFile).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (4m06s).
- Invariant scripts, all exit 0: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mem-port-contracts.sh`, `check-mcp-write-tools.sh`, `check-forwarded-header-trust.sh`,
  `check-forged-scope-fields.sh`, `check-scope-literal-defaults.sh`,
  `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`,
  `check-inert-enforcement.sh`.
- `check-task-commit-attribution.sh` → **OK against the committed candidate state** (exit 0;
  log `attribution-committed-state.log`). It FAILs only against the working tree because the
  review harness pre-staged a reset of `specs/tasks/task-128.md` to `commits: []` — the
  committed frontmatter at `043f6485` lists all six product-surface SHAs.
- **Registry-wide parent-id probe** (independent, verbatim-code): the executor's wrapper blocks
  gyre-server test compilation (4 GiB budget), so I extracted the candidate's
  `extract_path_attributes` / `prefix_entity_type` / `has_child_segments_after_id` /
  `path_params` / `percent_decode` byte-for-byte (sed line ranges 589–601, 603–607, 624–656,
  687–793, 795–801; documented substitutions: `pub fn`, `probe_warn!` for `tracing::warn!`,
  `use super::*`) plus the full 220-entry route registry (mechanically parsed) into a standalone
  crate (`review-evidence/probe/`) and replayed the round-4 invariant in both directions:
  **PASS** — 206 non-exempt routes, 45 parent-scoped child routes leak no parent `:id` into
  `resource.id`, 75 entity routes still surface `:id`, and the three round-4 flagged routes
  (`/merge-requests/:id/gates`, `/merge-requests/:id/dependencies`, `/agents/:id/messages`)
  leave `resource.id` unset while the ack child route still surfaces `:message_id` and
  `:path` still percent-decodes. **Mutation check**: reverting the guard to the pre-fix
  prefix-enumerated form makes the probe FAIL on exactly those three routes
  (`probe-mutation.log`), proving killing power; candidate code restored and re-passed, and
  `diff` confirms the probe's module is byte-identical to the candidate source.

## Verified against the task contract

- **Policy entity** (`gyre-domain/src/policy.rs`): every spec field present, including
  `immutable`, `scope: PolicyScope`, `scope_id: Option<Id>`, `created_by: Id`; `PolicyScope` =
  {Tenant, Workspace, Repo}; `PolicyEffect` = {Allow, Deny}; `ConditionOp` has all 8 variants;
  `ConditionValue` = String/StringList/Number/Bool (+`Null` as the value-side placeholder for
  `Exists` — spec-compatible extension).
- **Evaluation engine** (`gyre-server/src/policy_engine.rs`): immutable Deny evaluated before
  all priority-based policies; then priority desc → scope specificity (repo > workspace >
  tenant) → Deny-before-Allow tie-break; first match wins; default deny. All five §Policy
  Examples encoded as verbatim evaluation tests (spec_example_*), including the
  `agent-repo-scope` dynamic-reference example in both directions.
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation time preserving the
  referenced attribute's type (Single/List/Number/Bool); unresolvable references fail the
  condition (fail closed) — mutation-verified per round-3's repair note.
- **PolicyRepository port** (`gyre-ports/src/policy.rs`): CRUD + `list_by_scope` +
  prefix/scope deletes + decision audit; SQLite adapter round-trips conditions/actions/
  resource_types as JSON (existing `policies` table; `immutable` added by migration
  000028 at base); the built-in delete guard is enforced in code in BOTH adapters
  (`check-mem-port-contracts.sh` clean).
- **Attribute extraction** (abac_middleware): subject type/id/global_role/tenant_id from the
  auth extractor; persona/stack_hash (`wl_stack_hash` normalized)/attestation_level/repo_scope
  from agent JWT claims, gated on `subject.type == "agent"` and allowlisted to exactly those
  four names (`merge_agent_jwt_claims`) — membership-sourced names (workspace_role,
  workspace_ids, team_ids) are never claim-definable, with live killing tests
  (`workspace_role_claim_cannot_spoof_membership_attribute`,
  `human_persona_claim_cannot_define_agent_attribute`, `viewer_with_admin_claim_still_trips_notin_deny`);
  workspace_ids/team_ids/workspace_role from the real membership/team stores; resource
  id/workspace_id/repo_id from the request path (parent-id suppression verified by probe);
  resource tenant_id/owner/approval_status from real store lookups (spec ledger, personas,
  tasks, agents, MRs, teams) with fail-closed on lookup failure; env.ip from ConnectInfo
  socket peer (never forwarded headers); env.budget_remaining from the real budget config +
  usage stores; env.main_health from the real merge-queue pause state; env.time/action/
  resource.type injected by the evaluator. `resource.team`/`resource.visibility` are
  unpopulated and **honestly disclosed** in the coverage row (no entity type carries those
  fields; conditions on them fail closed) — not fakery.
- **Coverage rows 2–7** flipped to `implemented` (not `verified`) with per-attribute evidence
  notes; SUMMARY totals consistent.
- No production fakery found: no stubs, no bail-out stands-ins, no audit-only enforcement on
  specced enforcement paths, no hardcoded values standing in for lookups.

## Non-blocking observations (recorded for completeness)

- `resource.team` / `resource.visibility` have no source entity field anywhere in the domain
  yet; extraction arms set them when the fields exist. Fail-closed in the meantime.
- `spawn_agent_core` maps Kubernetes compute targets to attestation level 1 until the runtime
  reports pod workload attestations (disclosed in-code, deferred to task-129).
- Coverage row 3 says "32 abac_middleware tests"; the file now carries 41 (round-4 additions) —
  stale in the conservative direction.
- `evaluate()` clones the attribute context per request; documented, small map, no hot loop.

## Deferred verifier probes (blocked in this sandbox by the build wrapper's 4 GiB rule,
not by any code defect)

1. `cargo test --all` (acceptance criterion "cargo test --all passes").
2. `cargo test -p gyre-server --lib policy_engine::` — 34 engine tests (condition ops,
   dynamic references, immutable-deny precedence, all five spec examples).
3. `cargo test -p gyre-server --lib abac_middleware` — 41 live-middleware tests
   (spoofing kills, parent-id live reproductions, registry-wide invariant, entity/env
   attribute reach-live-evaluation tests).
4. `cargo test -p gyre-server --lib merge_processor` (evaluate_merge_abac scope filter was
   retyped for `Option<Id>`).
5. `cargo clippy --all-targets` and `cargo fmt --check` (round-4 additions were held to the
   frozen duplicate-literal count; a clippy pass on the new consts is the residual risk).
