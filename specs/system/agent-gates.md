# Agents as Gates & Spec-to-Code Binding

## Overview

Two interlocking features that make Gyre's forge uniquely powerful:

1. **Agents as gates** - configurable agent-driven review and validation that blocks merges until agents approve.
2. **Spec-to-code cryptographic binding** - every MR must reference the spec it implements, verified by git SHA. The forge enforces "specs first, always" cryptographically, not by convention.

Together, these create a provable chain: a reviewed spec was implemented by an attested agent, validated by gate agents, and merged only after all parties agreed - with every link cryptographically verifiable.

---

## Part 1: Agents as Gates

### The Problem

Current quality gates support `TestCommand`, `LintCommand`, and `RequiredApprovals`. These are static checks - run a command, count approvals. But the most valuable review in an agentic system is another agent reading the code, checking it against the spec, and making a judgment call. This requires a new gate type: spawn an agent, wait for its verdict, block or allow the merge based on its decision.

### Gate Types (Extended)

| Gate Type | What It Does | Blocks Until |
|---|---|---|
| `TestCommand` | Run a shell command (e.g., `cargo test`) | Exit code 0 |
| `LintCommand` | Run a linter (e.g., `cargo clippy`) | Exit code 0 |
| `RequiredApprovals` | Count review approvals | N approvals received |
| **`AgentReview`** | Spawn a review agent with the MR diff and spec context | Agent submits Approved decision |
| **`AgentValidation`** | Spawn a validation agent that runs domain-specific checks | Agent reports pass/fail |

### AgentReview Gate

When an MR enters the merge queue with an `AgentReview` gate configured:

1. **Forge spawns a review agent** with:
   - The MR diff (full patch)
   - The referenced spec (at the pinned SHA)
   - The MR description and acceptance criteria
   - The review persona (e.g., `personas/security.md` for a security review gate)
   - A scoped OIDC token with `review:submit` permission only

2. **Agent reviews** against the spec and its persona's criteria.

3. **Agent submits a verdict** via the existing Review API:
   - `Approved` - gate passes
   - `ChangesRequested` - gate fails with the agent's feedback
   - Agent's review comments appear on the MR like any other review

4. **Forge updates gate status** and the merge processor proceeds or blocks.

5. **Agent is torn down** after submitting its verdict (single-minded agents).

### Gate Chains (Composable Policies)

Repos configure gate chains as ordered policies:

```toml
# Per-repo gate configuration
[[gates]]
name = "tests"
type = "TestCommand"
command = "cargo test"
required = true

[[gates]]
name = "architecture-lint"
type = "LintCommand"
command = "scripts/check-arch.sh"
required = true

[[gates]]
name = "security-review"
type = "AgentReview"
persona = "personas/security.md"
required = true

[[gates]]
name = "spec-alignment-review"
type = "AgentReview"
persona = "personas/accountability.md"
required = true

[[gates]]
name = "peer-approval"
type = "RequiredApprovals"
min_approvals = 1
required = true
```

**Evaluation order:** All gates run in parallel. The merge processor waits until all required gates pass. If any required gate fails, the MR is blocked and the author agent is notified with the failure details.

**Optional gates:** Gates can be marked `required = false` for advisory-only checks. Their results are recorded but don't block merging.

### Gate Agent Lifecycle

- Gate agents are **ephemeral and single-purpose** - spawned for one review, torn down after.
- Gate agents get **read-only access** to the repo (they can't push, only review).
- Gate agents are **identity-scoped** - their OIDC token only permits review submission, not code changes.
- Gate agent reviews are **attributed** - the review shows which gate persona produced it.
- Gate agent **context windows are captured** for audit, like any other agent.

### Merge Processor Integration

```
MR enters merge queue
  -> Forge triggers all configured gates in parallel
    -> TestCommand gates run shell commands
    -> LintCommand gates run linters
    -> AgentReview gates spawn review agents
    -> RequiredApprovals gates check review count
  -> Merge processor polls gate status every cycle
    -> All passed: proceed with merge
    -> Any failed: block, notify author agent with failure details
    -> Any pending/running: requeue, check next cycle
  -> On merge: record which gates passed, with gate agent identities
```

---

## Part 2: Spec-to-Code Cryptographic Binding

### The Problem

"Specs first, always" is a design principle, but without enforcement it's just a suggestion. Agents can claim to implement a spec, implement something different, and the spec reference is just a string in a commit message. There's no cryptographic proof that:
- The spec existed when the code was written
- The spec was approved before implementation began
- The code was actually reviewed against that specific spec version
- The spec hasn't changed since the code was written

### How It Works

Every spec is a file in git. Every file in git has a SHA. Gyre uses these SHAs to create a verifiable chain from spec to merged code.

### The Provenance Chain

```
1. SPEC AUTHORED
   specs/system/identity-security.md is committed
   git SHA: abc123

2. SPEC APPROVED
   A reviewer (human or agent) signs the spec version:
   POST /api/v1/specs/approve
   {
     "path": "specs/system/identity-security.md",
     "sha": "abc123",
     "approver": "user:jsell",
     "signature": "<sigstore signature>"
   }
   Approval is recorded in the spec-approvals ledger.

3. TASK CREATED
   Task references the approved spec:
   {
     "title": "Implement OIDC identity provider",
     "spec_ref": "specs/system/identity-security.md@abc123",
     ...
   }

4. AGENT DISPATCHED
   Manager spawns an agent with the spec reference.
   Agent's context includes the spec content at that exact SHA.

5. COMMITS PRODUCED
   Every commit includes spec provenance in agent_commits:
   {
     "commit_sha": "def456",
     "spec_ref": "specs/system/identity-security.md@abc123",
     "task_id": "TASK-007",
     "agent_id": "worker-42"
   }

6. MR CREATED
   MR references the spec:
   {
     "spec_ref": "specs/system/identity-security.md@abc123",
     "title": "Implement OIDC identity provider"
   }

7. GATE AGENT REVIEWS
   AgentReview gate receives:
   - The MR diff
   - The spec at SHA abc123 (not HEAD - the pinned version)
   Gate agent verifies the code matches the spec it claims to implement.

8. FORGE VALIDATES AT MERGE
   Before merging, the forge checks:
   - spec_ref SHA abc123 exists in the repo
   - That SHA has an approval signature
   - The approval hasn't been revoked
   - The spec hasn't been superseded (optional: warn if spec HEAD != abc123)

9. MERGED
   The merge commit records the full chain:
   spec@abc123 (approved) -> task -> agent -> commits -> reviewed -> merged
```

### Spec Approval Ledger

The forge maintains a ledger of spec approvals:

```
spec_approvals table:
  id              TEXT PRIMARY KEY
  spec_path       TEXT NOT NULL        -- e.g., "specs/system/identity-security.md"
  spec_sha        TEXT NOT NULL        -- git blob SHA at approval time
  approver_id     TEXT NOT NULL        -- user or agent who approved
  signature       TEXT                 -- Sigstore signature (optional but recommended)
  approved_at     INTEGER NOT NULL
  revoked_at      INTEGER             -- NULL unless revoked
  revoked_by      TEXT
  revocation_reason TEXT
```

- Approvals are **immutable** - once recorded, they can be revoked but not modified.
- Revocation requires a reason and is audited.
- Multiple approvals can exist for the same spec (different versions).

### Forge Enforcement Policies

| Policy | Effect |
|---|---|
| `require_spec_ref` | MRs must include a spec_ref. MRs without one are rejected. |
| `require_approved_spec` | The referenced spec SHA must have an approval. Unapproved specs block merge. |
| `warn_stale_spec` | If spec HEAD has moved past the referenced SHA, warn but allow. The code may implement an older version. |
| `require_current_spec` | The referenced spec SHA must match spec HEAD. Forces re-review if spec changed. |
| `require_signed_approval` | Spec approvals must include a Sigstore signature. |

Policies are configurable per repository.

### What This Prevents

| Attack/Drift | How Binding Prevents It |
|---|---|
| Code without a spec | MR rejected: no spec_ref |
| Code referencing a nonexistent spec | MR rejected: SHA not found |
| Code referencing an unapproved spec | MR rejected: no approval for that SHA |
| Spec changed after code was written | Warning or rejection (policy-dependent) |
| Agent implements wrong spec version | Gate agent reviews against pinned SHA, catches mismatch |
| Retroactive spec modification | Approval is on a specific SHA. Changing the spec creates a new SHA requiring new approval. |
| Spec approved by unauthorized person | Approval ledger records approver identity; ABAC controls who can approve |

### Integration with Agent Gates

The `spec-alignment-review` gate type (using the Accountability agent persona) is specifically designed for this:

1. Gate agent receives the MR diff AND the spec at the pinned SHA
2. Agent compares: does the code actually implement what the spec describes?
3. Agent checks: are there spec requirements that the code doesn't address?
4. Agent submits Approved or ChangesRequested with specific findings

This turns the Accountability agent from a periodic patrol into a real-time merge gate.

---

## Relationship to Existing Specs

- **Source Control** (`source-control.md`): MRs gain a `spec_ref` field. Merge queue respects gate chain results.
- **Identity & Security** (`identity-security.md`): Gate agents use scoped OIDC tokens. Spec approvals can be Sigstore-signed.
- **Supply Chain** (`supply-chain.md`): AIBOM includes spec provenance chain. The spec SHA is part of the commit attestation.
- **Forge Advantages** (`forge-advantages.md`): Agent gates and spec binding are only possible because Gyre owns the forge. External forges can't spawn agents at merge time or verify spec SHAs at the protocol level.
- **Design Principles** (`design-principles.md`): "Specs first, always" becomes cryptographically enforced, not just a convention.
- **Personas** (`personas/accountability.md`, `personas/security.md`): These personas become gate agents, not just patrol agents.
