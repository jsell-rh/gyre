# Review: task-129 — ABAC evaluation engine, API, and audit integration

**Candidate:** `8bf04e4b00c35cca57417f91c5dc7b9a78e44936` (base `cd58a166`)
**Verdict:** NOT APPROVED (2 findings — 1 blocking spec-fidelity/enforcement, 1 minor docs)
**Evidence:** `/tmp/stage/review-evidence/task-129-review-round.md`

## What was verified and holds

- **Engine** (`policy_engine::evaluate`): immutable-Deny-first, priority-DESC
  with scope-specificity and Deny-before-Allow tiebreakers, first-match-wins
  with short-circuit, default deny. Sound.
- **Middleware integration**: path/entity resource extraction runs before the
  membership block, so `subject.member_of_resource_workspace` is derived from
  the real membership store against the final addressed workspace, with
  fail-closed `false` on store error. The reserved `gyre-system-token`
  identity mapping is gated on `is_system_principal()` (agent_id "system" AND
  no user identity) and cannot be forged by JWT/API-key subjects.
- **Cache** (§Performance): read-through `CachedPolicyRepository`; every
  non-test policy write goes through the port and invalidates; the one
  out-of-band writer (`apply_trust_transition`) is invalidated at both handler
  call sites; the port documents the contract. Latency benchmark measures real
  `Instant` deltas over the full seeded set.
- **Audit** (§Audit Integration): every evaluation persists a `PolicyDecision`
  with all spec fields; denials at warn + `AbacDenied`, allowances now at
  debug; queryable via `GET /policies/decisions`.
- **API** (§API/§Dry-Run): all 8 endpoints live; dry-run evaluates the real
  seeded set; `/policies/effective` now loads real subject attributes;
  Admin-only policy management enforced by a seeded Deny at 860 with a live
  discriminating test.
- **Probes run here**: `cargo test -p gyre-domain` (507+8 pass), `cargo check
  -p gyre-server` and `--tests` (exit 0), check-arch, check-abac-route-registry,
  check-mem-port-contracts all pass. gyre-server test execution is blocked by
  the stage wrapper and deferred to the verifier (probe list in the evidence
  file).

## Finding 1 (blocking): `agent-repo-scope` built-in is neither enforced nor the spec's policy

The spec table row (`agent-repo-scope | Deny | Agents can't access resources
outside their scoped repo`) is seeded `enabled: false`, so the containment
does not exist — the enabled `agent-scoped-access` Allow (700, role-only
condition, read/write/complete/approve on `*`) lets worker agents access any
repo in their tenant. It also cannot be enabled: the API refuses built-in
updates (400) and the startup drift refresh reverts DB `enabled` edits, so the
shipped claim "enforced the moment it is enabled" is unreachable. And even if
force-enabled, the seeded conditions (`type == agent` AND `repo_scope Exists`,
no `resource.repo_id` comparison) would deny repo-scoped agents everything —
a blanket agent kill-switch, not out-of-scope containment; the spec's
`not_equals $resource.repo_id` dynamic-reference semantics (already proven by
the engine test `spec_example_agent_repo_scope`) were not used. Coverage row
11 marks §Built-In Policies implemented; this row is hollow, and no
spec-lifecycle amendment documents the deviation (goal rule 4).

Fix directions (either is acceptable, but the current middle ground is not):
implement the spec-exact policy (NotEquals `$resource.repo_id`, which fails
closed for repo-less resources) enabled by default, or amend the spec via the
spec lifecycle documenting why agent-repo containment is disabled and correct
the coverage row + shipped claims accordingly.

## Finding 2 (minor): wrong query parameter documented for /policies/effective

docs/api-reference.md:290 documents `?subject={id}` but the handler binds
`subject_id` (api/policies.rs `EffectiveQuery`). Callers following the docs
get the deny-everything fallback the task-129 change was meant to eliminate.

## Verifier must run (blocked here by the stage wrapper)

```
cargo test -p gyre-server --lib abac_middleware
cargo test -p gyre-server --lib policy_engine
cargo test -p gyre-server --lib policy_cache
cargo test -p gyre-server --lib api::policies
cargo test -p gyre-server --lib api::workspaces
cargo test --all
```
