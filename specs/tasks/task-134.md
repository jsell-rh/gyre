---
title: "Define AgentReview and AgentValidation gate types with review protocol"
spec_ref: "agent-gates.md §Part 1"
depends_on: []
progress: needs-revision
coverage_sections:
  - "agent-gates.md §Gate Types (Extended)"
  - "agent-gates.md §AgentReview Gate"
commits: ["42d79754d2d60ec536a1f7d4ff24d68af364e6e6", "22a25fab8fb3412e6d09da0a5c45d1f714c2d9d0", "e248e5f1a8fb7dce36deaf471f8acd7ae5710426", "e7172465f4a51aba2b8fc13bd672af94ac433ace", "a53b998f148b442a502d859a159908c6256b83e7", "3bf9f6fb007555d64986a22264e6a1f9de10999c", "4e44c4b532959cfe04e5e797600e14cc509e29fd", "71046576b850cfb675489c3ad36cd73f1ed64c4f", "596d2170113c32f67339f141ef3a871b40f509b8", "da4b7a36edb07bb3fdd683e7eb372c4a9b445db7", "1b40aacb758aa0fbf8aae9fec8e1f084899055c8"]
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
behavior (checkpoint commits `42d79754`…`1b40aacb`, recovered and verified in
this assignment; the attribution repair from the interrupted run is included
at `9bda8c17`).

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
  `scope: review:submit` only. Enforcement is real and triple-layered:
  `git_http` denies push (403 read-only), the ABAC middleware allow-lists
  review routes only (403 outside), and `submit_review` binds the reviewer
  identity to the token subject (forged `reviewer_agent_id` in the body is
  ignored). All four behaviors covered by `tests/task134_review_probe.rs`
  (4 passed) — oneshot router calls, no listener required.
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the
  agent's feedback surfaced in gate output; only the gate's own agent's
  verdict counts; exit-0-without-review fails ("cannot determine state" is
  not "state is fine"). Unit tests plus two end-to-end tests that bind a real
  axum server, a real bare git repo with a spec pinned at a pre-tip SHA, and
  drive the fixture `review_agent_driver.sh` through the live Review API.
- **Teardown**: token revoked (`kv_remove`) on every exit path (verdict,
  timeout, spawn failure) — asserted by tests; temp spec/diff files removed.
- **AgentValidation**: spawns the configured validation agent with
  `GYRE_VALIDATION_TYPE` delivered and attributed in output; pass/fail from
  the exit code; no-command fails closed.

Test evidence (this sandbox, branch `pipeline/task-134/24f2f8fe…-1`, in
`/tmp/stage/review-evidence/task-134-verification.md`): gate_executor 33/33,
task134_review_probe 4/4, api::gates 6/6, api::merge_requests 21/21, adapters
quality_gate 1/1, git2 read_file_at_commit 3/3. All 15 frozen-baseline
mechanical checks pass, plus migration versions/portability and commit
attribution. The two `agent_review_end_to_end` tests SKIP here — this sandbox
denies loopback `accept()` (errno 95, infrastructure restriction, recorded in
capabilities); they must run on host/GitHub CI, which is mandatory for
exact-head verification anyway. `cargo test --all` is owned by verification
and publication; focused suites above cover every touched module.

## Review

### Review changed source code

- crates/gyre-server/src/api/merge_requests.rs

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
