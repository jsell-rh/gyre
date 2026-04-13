---
title: "Define AgentReview and AgentValidation gate types with review protocol"
spec_ref: "agent-gates.md §Part 1"
depends_on: []
progress: not-started
coverage_sections:
  - "agent-gates.md §Gate Types (Extended)"
  - "agent-gates.md §AgentReview Gate"
commits: []
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
