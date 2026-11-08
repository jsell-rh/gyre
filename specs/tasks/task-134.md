---
title: "Define AgentReview and AgentValidation gate types with review protocol"
spec_ref: "agent-gates.md §Part 1"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Types (Extended)"
  - "agent-gates.md §AgentReview Gate"
commits: ["b3678e2762149ba743e63c604bb618e66dd564ab"]
---

## Spec Excerpt

From `agent-gates.md` §Gate Types (Extended) and §AgentReview Gate:

**Five gate types total:**

| Gate Type | What It Does | Blocks Until |
|---|---|---|
| `TestCommand` | Run a shell command (e.g., `cargo test`) | Exit code 0 |
| `LintCommand` | Run a linter (e.g., `cargo clippy`) | Exit code 0 |
| `RequiredApprovals` | Count review approvals | N approvals received |
| **`AgentReview`** | Spawn a review agent with MR diff + spec context | Agent submits Approved decision |
| **`AgentValidation`** | Spawn a validation agent for domain-specific checks | Agent reports pass/fail |

**AgentReview Gate flow:**

1. Forge spawns review agent with: MR diff (full patch), referenced spec (at pinned SHA), MR description + acceptance criteria, review persona (e.g., `personas/security.md`), scoped OIDC token with `review:submit` permission only
2. Agent reviews against the spec and persona criteria
3. Agent submits verdict via Review API: `Approved` or `ChangesRequested` (comments appear on MR like any other review)
4. Forge updates gate status — merge proceeds or blocks
5. Agent torn down after verdict (single-minded agents)

## Implementation Plan

1. **Extend gate type enum in domain:**
   - Add `AgentReview` variant to existing gate type enum
   - Add `AgentValidation` variant
   - `AgentReview` carries: persona slug, required (bool)
   - `AgentValidation` carries: validation_type (string), persona slug, required (bool)

2. **Gate configuration model:**
   - Extend gate CRUD to accept AgentReview/AgentValidation configuration
   - AgentReview config: `{ persona: String, required: bool }`
   - AgentValidation config: `{ validation_type: String, persona: String, required: bool }`
   - Store in existing gates table with type discriminator

3. **Review agent spawn protocol:**
   - When merge processor encounters AgentReview gate:
     - Resolve persona by slug
     - Gather MR context: diff, spec_ref (at pinned SHA), description, acceptance criteria
     - Spawn agent via existing spawn infrastructure with:
       - `agent_type = "review"`
       - Scoped OIDC token: `scope: ["review:submit"]` (read-only, no push)
       - Context: MR diff + spec content + review persona system_prompt
     - Record gate_agent_id for tracking

4. **Review verdict submission:**
   - Gate agent uses existing Review API (`POST /api/v1/merge-requests/:id/reviews`)
   - Forge watches for review from gate agent
   - Map review decision to gate status: Approved → Passed, ChangesRequested → Failed

5. **Gate agent teardown:**
   - After verdict submission, auto-complete the review agent
   - Clean up: revoke JWT, remove ABAC policies

6. **API: extend gate endpoints:**
   - `POST /api/v1/repos/:id/gates` accepts AgentReview/AgentValidation types
   - `GET /api/v1/repos/:id/gates` returns all gate types including new ones
   - Verify routes in `gyre-server/src/api/mod.rs`

## Acceptance Criteria

- [ ] AgentReview and AgentValidation gate type variants defined
- [ ] Gate CRUD accepts and stores AgentReview configuration (persona, required)
- [ ] Review agent spawn protocol: persona resolution, MR context gathering, scoped token
- [ ] Review agent receives MR diff, spec at pinned SHA, persona system_prompt
- [ ] Agent's OIDC token scoped to `review:submit` only
- [ ] Review verdict maps to gate status (Approved→Passed, ChangesRequested→Failed)
- [ ] Gate agent auto-teardown after verdict
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` Part 1 §Gate Types through §AgentReview Gate. Existing gate implementation: `gyre-server/src/api/gates.rs`, gate types in domain. Merge request reviews: `gyre-server/src/api/merge_requests.rs` (submit_review, list_reviews). Agent spawn: `gyre-server/src/api/spawn.rs`. Gate routes: `GET/POST /api/v1/repos/:id/gates` registered in `gyre-server/src/api/mod.rs` at line ~152. Persona resolution: `gyre-server/src/api/personas.rs` (resolve_persona). JWT minting: `gyre-server/src/auth.rs` (mint_with_workload). Check migration numbering: currently at 000049.

## Shipped

Both `agent-gates.md` coverage sections are implemented with real, production
behavior (product commits `42d79754`…`1b40aacb`, recovered across interrupted
runs and completed across assignments, including the review-F1/F2/F3 repairs
and the review-round TTL repair `72c5bb03`).

- **Gate types**: `AgentReview` and `AgentValidation` variants in
  `gyre-common`'s `GateType`; `QualityGate` carries `persona` (existing) and
  the new `validation_type` column (migration `2026-10-09-000056`), persisted
  by both SQLite and Postgres adapters with an upsert round-trip test. Gate
  CRUD (`api/gates.rs`) accepts/returns both types and `validation_type`
  (tests: `create_agent_review_gate`, `create_agent_validation_gate`).
- **Review spawn protocol** (`gate_executor.rs`): persona resolved
  nearest-wins (repo → workspace → tenant, no fabricated tenant scope);
  unresolvable persona/spec_ref fails the gate before any process spawns.
  MR context gathered for real: full diff via `git_ops.diff`, spec content at
  the pinned SHA via a new `read_file_at_commit` port (unknown SHA is an
  error, not a silent None), MR title, and task description (acceptance
  criteria carrier) via MR → author agent → task.
- **Scoped token**: `AgentSigningKey::mint_scoped` mints a JWT with
  `scope: review:submit` only (review agents) or `validation:report`
  (validation agents, review F2 — validators have no review-submission
  capability). Enforcement is real and layered, all exact-match capability
  comparison (review F3): `git_http` denies push (403 read-only), the ABAC
  middleware allow-lists each capability's routes only (403 outside;
  validators are read-only MR context), and `submit_review` binds the
  reviewer identity to the token subject (forged `reviewer_agent_id` in the
  body is ignored). Covered by `tests/task134_review_probe.rs` (7 passed) —
  oneshot router calls, no listener required.
- **Token TTL tracks the configured gate window** (review repair `72c5bb03`):
  both mint sites derive the JWT TTL from
  `gate.timeout_secs.unwrap_or(AGENT_GATE_TIMEOUT_SECS) + 60` via the shared
  `gate_token_ttl_secs` helper. Previously the TTL was fixed at 360s while
  the process timeout honored per-gate `timeout_secs`, so a verdict
  submitted after t=360s in a `timeout_secs > 360` gate hit a 401 (expired
  JWT) and the gate falsely failed with "exited without submitting a
  review". Regression tests on both mint paths
  (`review_token_ttl_covers_configured_gate_timeout`,
  `validation_token_ttl_covers_configured_gate_timeout`) assert the minted
  token's `exp − iat` covers a configured `timeout_secs=700` window; both
  fail against the previous fixed-TTL code (verified by mutation).
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the
  agent's feedback surfaced in gate output; only the gate's own agent's
  verdict counts; exit-0-without-review fails ("cannot determine state" is
  not "state is fine"). Unit tests plus two end-to-end tests that bind a real
  axum server, a real bare git repo with a spec pinned at a pre-tip SHA, and
  drive the fixture `review_agent_driver.sh` through the live Review API.
  Both e2e tests seed builtin ABAC policies (review F1: `build_state` wires
  an empty policy store; unseeded default-deny made the driver's review POST
  403 on loopback-capable hosts); `e2e_review_submission_is_abac_allowed_`
  `with_seeded_policies` guards that regression without needing loopback.
- **Teardown**: token revoked (`kv_remove`) on every exit path (verdict,
  timeout, spawn failure) — asserted by tests; temp spec/diff files removed.
- **AgentValidation**: spawns the configured validation agent with
  `GYRE_VALIDATION_TYPE` delivered and attributed in output; pass/fail from
  the exit code; no-command fails closed.

### Test evidence

Rebase-repair round (branch
`pipeline/task-134/dc745ac59e0d4cce98c897c7169aeef3-1`, merge commit
`eacee766a`, resolving the merge of the task-134 candidate `9248f6e0`
with base `c5deca80`). No product behavior changed: the four conflicted
files were resolved preserving both sides' intent — the candidate's
`read_file_at_commit` port method and `validation_type` gate field
against base's expanded `revert_commit` docs and base-added
`QualityGate` test literals (two in `api/recovery.rs` completed with
`validation_type: None` after the auto-merge left them uncompilable).
The `fabricated-scope-defaults-exemptions.txt` conflict resolved to the
truth of the merged tree: the auth.rs entry renumbered 773→837 (same
pre-existing, test-pinned site — `jwt_missing_tenant_id_defaults_to_default`
— shifted by the candidate's `mint_scoped` additions), and the
gate_executor.rs:111 entry deleted because the candidate's gate-executor
rewrite genuinely fixed that site (skip-and-log on unresolvable MR);
FROZEN_EXEMPTION_COUNT lowered 4→2 to match the remaining entries. Also
unwrapped the discarded `seed_builtin_policies` Results at the task-134
test call sites (must-use lint; a seeding failure now fails the test
instead of silently default-denying).

All focused suites re-ran green on the merged tree: gate_executor 37/37
(including both TTL regression tests and both e2e review tests),
task134_review_probe 7/7, and the merge-touched surfaces
api::gates + api::merge_requests + api::mr_timeline + merge_processor +
api::recovery + git_http 163/163, git_integration 16/16. Mechanical
probes pass on the merged tree: check-fabricated-scope-defaults,
check-arch, check-mem-port-contracts, check-migration-versions,
check-dead-message-kinds, check-scope-literal-defaults; the exemption
freeze adds no entries relative to either merge parent. The earlier TTL
mutation check (both regression tests FAIL against the pre-fix fixed-TTL
code) is documented in the prior round's evidence. `cargo test --all` and
the full mechanical-check suite are owned by verification and
publication.
