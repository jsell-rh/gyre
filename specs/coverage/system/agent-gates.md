# Coverage: Agents as Gates & Spec-to-Code Binding

**Spec:** [`system/agent-gates.md`](../../system/agent-gates.md)
**Last audited:** 2026-04-13
**Coverage:** 2/25 (9 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Overview | 2 | n/a | - | Section heading only — no implementable requirement. |
| 2 | Part 1: Agents as Gates | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 4 | Gate Types (Extended) | 3 | task-assigned | task-134 | Need AgentReview and AgentValidation gate type variants. |
| 5 | AgentReview Gate | 3 | task-assigned | task-134 | Review agent spawn protocol, scoped token, verdict submission. |
| 6 | Gate Chains (Composable Policies) | 3 | implemented | task-135 | Ordered chains real: `quality_gates.position` (migration `2026-11-12-000061`), adapters `ORDER BY position, created_at` (SQLite+PG+mem), `create_gate` appends. Chain configuration: `PUT/GET /repos/:id/gate-chain` accepts/returns the spec's `[[gates]]` TOML (mixed types, `required`, `min_approvals`); PUT replaces atomically (validation-before-mutation, 400 leaves chain untouched), same-named gates keep ids, absent gates deleted. Parallel execution pre-existing (enqueue → `trigger_gates_for_mr` → one tokio task per gate); merge processor waits (`check_gates_for_mr` Ok(false) requeue) / blocks+notifies on required failure. Optional gates non-blocking with advisory MR comment (`gate-agent:{id}`) on failure. RequiredApprovals now really counts distinct approvals (was hollow "delegated to merge processor" with no delegate): live evaluation each processor cycle, Running until N distinct approvals, promotion persisted. Marked implemented task-135. |
| 7 | Gate Agent Lifecycle | 3 | implemented | task-135 | Ephemeral/read-only/identity-scoped shipped in task-134 (token revoked on exit incl. crash/timeout; push denied for `review:submit`; scoped JWT exact-match). Attribution NEW: `Review.reviewer_persona` + `Review.gate_id` (migration `2026-11-12-000061`, both adapters + mem), derived server-side in `submit_review` from the scoped token's `task_id` claim (the gate) — body fields cannot forge it; surfaced in ReviewResponse. Context windows captured NEW: the exact bundle the agent sees (persona system prompt, MR diff, spec at pinned SHA, MR title, task description) zstd-compressed into the conversation repository before the subprocess runs, replayable at `GET /api/v1/conversations/:sha`, referenced by a `gate_agent_context` audit event (SHA, gate, persona, MR); validators captured too; capture skipped (logged) when the tenant scope is unresolvable — never fabricated. Marked implemented task-135. |
| 8 | Merge Processor Integration | 3 | task-assigned | task-136 | Gate trigger on MR enqueue, status polling, merge decision logic. |
| 9 | Gate Failure Feedback | 3 | task-assigned | task-137 | Inbox delivery, deferred task creation, escalation protocol. |
| 10 | Part 3: Cryptographic Gate Attestation | 2 | n/a | - | Section heading only — no implementable requirement. |
| 11 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 12 | Merge Attestation Bundle | 3 | task-assigned | task-142 | JSON schema, bundle generation at merge time, per-component signing. |
| 13 | What Gets Signed and By Whom | 3 | task-assigned | task-142 | Multi-party attestation model: forge, gate agents, approver. |
| 14 | Verification | 3 | task-assigned | task-143 | Bundle signature verification against OIDC issuer, offline capability. |
| 15 | Storage | 3 | task-assigned | task-144 | Git note, audit_events, Rekor, AIBOM storage targets. |
| 16 | The Complete Provenance Chain | 3 | n/a | - | Summary diagram — no additional implementable requirement beyond §12-15. |
| 17 | Part 2: Spec-to-Code Cryptographic Binding | 2 | n/a | - | Section heading only — no implementable requirement. |
| 18 | The Problem | 3 | n/a | - | Context/rationale — no implementable requirement. |
| 19 | How It Works | 3 | task-assigned | task-138 | SHA-based linking of spec to code through provenance chain. |
| 20 | The Provenance Chain | 3 | task-assigned | task-138 | 9-step chain from spec authoring through merge. |
| 21 | Spec Approval Ledger | 3 | task-assigned | task-138 | spec_approvals table, ApprovalStatus enum, transitions. |
| 22 | Forge Enforcement Policies | 3 | task-assigned | task-139 | 5 policies: require_spec_ref, require_approved_spec, etc. |
| 23 | What This Prevents | 3 | task-assigned | task-145 | Attack/drift prevention — validation tests for enforcement. |
| 24 | Integration with Agent Gates | 3 | task-assigned | task-139 | Spec-alignment-review gate with accountability persona. |
| 25 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
