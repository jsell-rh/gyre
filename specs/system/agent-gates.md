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
    -> Any failed: notify author, create task if session ended
    -> Any pending/running: requeue, check next cycle
  -> On merge: produce signed merge attestation bundle
```

### Gate Failure Feedback

When a gate fails, the forge must get feedback to the right agent as fast as possible. Speed matters -- a gate failure is a Ralph loop event, not a separate workflow. See [agent-runtime.md](agent-runtime.md) §1 for the canonical agent lifecycle definition.

#### Agent Inbox Delivery

The forge delivers a `GateResult` message to the author agent's **inbox**:

```json
{
  "type": "GateResult",
  "mr_id": "MR-042",
  "gate_name": "security-review",
  "gate_type": "AgentReview",
  "status": "Failed",
  "output": "ChangesRequested: SQL query in handlers.rs:47 uses string interpolation...",
  "spec_ref": "specs/system/identity-security.md@abc123",
  "attempt": 2,
  "gate_agent_id": "security-gate-7"
}
```

In the Ralph loop model, the agent's session has typically ended by the time gates run. The message is delivered to the agent's inbox and read by the next session, which addresses the feedback, re-pushes, and gates re-run. For single-shot (non-loop) agents with a live session, the message is also broadcast via WebSocket for immediate reaction.

#### Deferred: Task Creation

If the author agent has converged, been stopped, or the gate has failed 3+ times on the same MR, the forge creates a task:

```
title: "Gate failure: {gate_name} on MR #{mr_id} (attempt {n})"
status: Backlog
priority: High
labels: ["gate-failure", "auto-created"]
description: |
  Gate '{gate_name}' failed on MR #{mr_id}.

  Failure output:
  {gate output}

  Spec reference: {spec_ref}
  Attempts: {n}
  Author agent: {agent_id} (session ended)

  Fix the issue and re-push to trigger gate re-evaluation.
```

The repo orchestrator picks this up in its next OBSERVE cycle and dispatches a new agent.

#### Escalation

- **Security gate failure:** The Security agent persona is notified in addition to the author. Critical security findings escalate to the Overseer (human).
- **3+ consecutive failures on the same gate:** The repo orchestrator is notified directly. This is a signal that the task may need re-scoping or the spec may be ambiguous.
- **All gates failed:** The MR is removed from the merge queue. A task is created to reassess the approach.

---

## Part 3: Cryptographic Gate Attestation

### The Problem

Gate results are currently just database records. There's no cryptographic proof that gates actually ran and passed. An admin with database access could mark a gate as passed without it running. A compromised merge processor could skip gate checks.

### Merge Attestation Bundle

When all gates pass and a merge executes, the forge produces a **merge attestation bundle** - a signed document attached to the merge commit:

```json
{
  "merge_attestation_version": "1.0",
  "mr_id": "MR-042",
  "merge_commit_sha": "789abc",
  "merged_at": "2026-03-20T14:30:00Z",
  "spec_ref": "specs/system/identity-security.md@abc123",
  "spec_approval": {
    "approver": "user:jsell",
    "approved_at": "2026-03-19T10:00:00Z",
    "signature": "<sigstore signature on spec approval>"
  },
  "author": {
    "agent_id": "worker-42",
    "oidc_sub": "agent:worker-42",
    "stack_attestation": "sha256:stack-fingerprint...",
    "attestation_level": 3,
    "conversation_sha": "sha256:..."
  },
  "gates": [
    {
      "name": "tests",
      "type": "TestCommand",
      "status": "Passed",
      "output_hash": "sha256:...",
      "started_at": "2026-03-20T14:20:00Z",
      "finished_at": "2026-03-20T14:22:00Z",
      "signed_by": "forge",
      "signature": "<sigstore signature by forge OIDC identity>"
    },
    {
      "name": "architecture-lint",
      "type": "LintCommand",
      "status": "Passed",
      "output_hash": "sha256:...",
      "started_at": "2026-03-20T14:20:00Z",
      "finished_at": "2026-03-20T14:20:30Z",
      "signed_by": "forge",
      "signature": "<sigstore signature by forge OIDC identity>"
    },
    {
      "name": "security-review",
      "type": "AgentReview",
      "status": "Passed",
      "reviewer_agent_id": "security-gate-7",
      "reviewer_oidc_sub": "agent:security-gate-7",
      "reviewer_stack_attestation": "sha256:...",
      "review_decision": "Approved",
      "review_body_hash": "sha256:...",
      "signed_by": "agent:security-gate-7",
      "signature": "<sigstore signature by gate agent's OIDC identity>"
    },
    {
      "name": "spec-alignment-review",
      "type": "AgentReview",
      "status": "Passed",
      "reviewer_agent_id": "accountability-gate-3",
      "reviewer_oidc_sub": "agent:accountability-gate-3",
      "reviewer_stack_attestation": "sha256:...",
      "review_decision": "Approved",
      "review_body_hash": "sha256:...",
      "signed_by": "agent:accountability-gate-3",
      "signature": "<sigstore signature by gate agent's OIDC identity>"
    }
  ],
  "bundle_signature": "<sigstore signature by forge over entire bundle>"
}
```

### What Gets Signed and By Whom

| Component | Signed By | Proves |
|---|---|---|
| Spec approval | Approver (human or agent) | This version of the spec was reviewed and approved |
| Author commits | Author agent (Sigstore/OIDC) | This agent produced this code with this stack |
| TestCommand/LintCommand results | Forge (Sigstore/OIDC) | The forge ran this command and it passed |
| AgentReview results | Gate agent (Sigstore/OIDC) | This agent reviewed the code and approved it |
| Merge attestation bundle | Forge (Sigstore/OIDC) | All of the above is true and the merge was legitimate |

Each signer uses their own OIDC identity. The forge signs with its server identity. Gate agents sign with their scoped OIDC tokens. The result is a **multi-party attestation** where no single entity can forge the complete bundle.

### Verification

Anyone who trusts Gyre's OIDC issuer can verify the entire bundle:

1. Verify bundle_signature against Gyre's OIDC issuer
2. For each gate, verify its signature against the signer's OIDC identity
3. Verify the spec approval signature
4. Verify the author's commit signatures via Rekor transparency log
5. Check that all gate results reference the same MR and spec

This can be done offline, after the fact, by auditors, compliance teams, or other Gyre instances (federation).

### Storage

Merge attestation bundles are stored:
- As a **git note** on the merge commit (accessible via `git notes show`)
- In the **audit_events** table for queryability
- In **Rekor** transparency log for non-repudiation
- In the **AIBOM** for each release that includes the merge commit

### The Complete Provenance Chain

```
spec@SHA (approved, signed by approver)
  -> task (references spec@SHA)
    -> agent dispatched (OIDC identity, stack attested)
      -> commits (Sigstore signed by agent)
        -> gate: tests passed (signed by forge)
        -> gate: lint passed (signed by forge)
        -> gate: security agent approved (signed by gate agent)
        -> gate: accountability agent approved (signed by gate agent)
      -> merge attestation bundle (signed by forge, contains all above)
        -> merge commit (bundle attached as git note)
          -> release AIBOM (includes all bundles)
```

Every link is cryptographic. Every signer is independently verifiable. No single point of trust.

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
  approved_at     INTEGER             -- NULL when status is Pending
  revoked_at      INTEGER             -- NULL unless revoked
  revoked_by      TEXT
  revocation_reason TEXT
  rejected_at     INTEGER             -- NULL unless rejected (per human-system-interface.md §8)
  rejected_by     TEXT
  rejected_reason TEXT
```

**ApprovalStatus enum:**
```rust
pub enum ApprovalStatus {
    Pending,    // no timestamp columns set
    Approved,   // approved_at is set
    Revoked,    // revoked_at is set (post-merge withdrawal)
    Rejected,   // rejected_at is set (pre-merge decline)
}
```
Status is derived from which timestamp column is non-null. Mutual exclusivity: the handler clears all other timestamp columns when setting a new status. Only one timestamp is non-null at any time.

- Approval records support status transitions (Pending → Approved → Revoked, or Pending → Rejected). The handler clears other timestamp columns on transition to maintain mutual exclusivity. The *audit trail* of who approved/rejected and when is preserved via the timestamp values.
- Revocation requires a reason and is audited.
- Rejection closes the associated MR (per `human-system-interface.md` §8).
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
