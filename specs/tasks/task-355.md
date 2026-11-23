---
title: "Mandatory provenance at both delivery routes and merge: enforced builtin policy, fail-closed error paths, workload-identity match, merge-time signature checks, ABAC actions"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / boundary-enforcement / 1d3b1257654b"
depends_on: [task-352, task-353, task-357]
progress: not-started
commits: []
---

## Required behavior

§7.2/§8 Phase 3: builtin:require-signed-authorization (immutable Deny, priority 998) is enforced, not just seeded — the push-time attestation-scoped ABAC evaluation becomes enforcing (a Deny decision rejects the push with refs undone, replacing the audit-only 'the gate itself remains audit-only (Phase 1)' behavior in git_http.rs) and is evaluated for every push and merge action by non-system subjects, including chain-less work (the attestation-chain-present unless-attribute no longer short-circuits evaluation; the merge-time gate no longer skips evaluation when no chain is found — 'Deny unless a valid attestation chain exists' applies to chain-less merges; today merge proceeds when '!chain_found && denial.is_none()'). System subjects are exempt per the policy conditions. If literal enforcement on human pushes conflicts with another canonical spec (spec-lifecycle.md's non-blocking human push contract, which the active agent-workflow plan's pr-protection task also preserves), resolve the conflict through the spec lifecycle (propose an amendment) — never by silently weakening the policy or leaving it audit-only; the enforcement ships per the canonical §7.2 text unless and until an amendment is approved. Fail-closed error paths: on the enforcing push and merge paths, chain-store query errors and ABAC-evaluation-unavailability errors REJECT with a named reason (refs undone / merge failed) instead of allowing — the 'Query failures are non-blocking' branch in enforce_push_constraints (constraint_check.rs:304) and the 'push attestation ABAC evaluation unavailable (audit-only, push proceeds)' path (git_http.rs) are removed on enforcing paths; a store outage can no longer silently disable mandatory verification (consistent with §3.4's fail-closed philosophy). jj delivery route: the same pre-accept enforcement — chain verification, constraint evaluation, and attestation ABAC — runs at the server-side jj export delivery route (create_mr_for_completion after jj_bookmark_set + jj_git_export in spawn.rs, which today moves the MR source-branch ref with no receive-pack and therefore no verification), coordinating with the staged source-control plan's jj-export-gate-audit-parity task (planner spec-36fad8fcafe7b73a2b6f2d0e, plan 775ba5f9c68bb733dd2a785caaa58464bb763a7ae1490f85cad7de2a805d5a5e) which refactors gate evaluation and push-audit emission into shared production call points used by both routes — this task's attestation/constraint enforcement rides the same shared call point so both delivery routes evaluate the same enforcement, without duplicating logic that can drift; on rejection the source-branch ref is restored to its pre-export position, no MR is created from the violating commits, and the agent's completion fails naming the reason (source-control.md requires push-time validation regardless of whether the client used git or jj). §6.1 fourth bullet: push and merge verification checks that the acting agent's workload identity matches the key binding in the attestation chain — the chain leaf's DerivedInput key binding (agent identity, task_id, spawned_by) must correspond to the acting agent; mismatch rejects with a named reason (today no push/merge path compares the acting agent to the chain's key binding). §6.2 Phase 5 at boundaries: output signature verification (agent_signature over content_hash, every gate signature over signable_bytes) runs in the merge enforcement path, not only in the GET verification endpoint; invalid signatures fail the merge (today a merge whose chain carries an invalid gate signature succeeds). §6.2/§3.3 action context: the GET /api/v1/repos/:id/attestations/:commit_sha/verification endpoint derives the action (push/merge) from the verification context instead of hardcoding Action::Push (provenance.rs:274), so action-sensitive constraints evaluate correctly for merge commits. §7.2 actions/routes: the attestation resource's actions are realized in the ABAC engine — /api/v1/attestations/verify maps to the verify action (not 'read', abac_middleware.rs:423-427), GET .../attestations/:commit_sha/bundle maps to export, .../verification to verify, .../chain to read; the three repos/:id/attestations/:commit_sha/* routes are removed from scripts/abac-route-registry-exemptions.txt (lines 31-33; shrink only — the file is frozen against growth) and registered in the RouteResourceMapping resolver, with per-handler tenant/workspace authorization where middleware scoping does not apply; the revoke action is registered in the attestation resource's action vocabulary for policy authoring (no revocation endpoint is specced in this document — creating one would be a speculative feature and is out of scope). The already-populated subject attributes (chain_depth, root_signer, constraint_count) feed the enforced evaluation and its decision audit trail.

## Acceptance Criteria

- [ ] A push by a non-system subject with no valid attestation chain is rejected (403, refs undone) with the policy named; a chain-less merge is denied; system-subject actions are exempt.
- [ ] The policy is immutable: an Allow policy cannot override the Deny (same immutability class as builtin:require-human-spec-approval).
- [ ] Fail-closed: a simulated chain-store error (or ABAC evaluation unavailability) on the enforcing push/merge path results in REJECTION with a named reason, not allowance.
- [ ] A jj-workspace agent delivery with no valid chain is rejected at delivery with the source-branch ref restored to its pre-export position and no MR created — not first at merge.
- [ ] A push/merge by an agent whose identity does not match the chain leaf's key binding (different agent or task) is rejected naming the workload-identity mismatch.
- [ ] A merge fails when an agent_signature or gate signature on the chain is invalid; valid signatures pass.
- [ ] The verification endpoint evaluates action-sensitive constraints with the correct action for merge commits (no hardcoded Push).
- [ ] The attestation routes are policy-evaluated (resolver entries with the attestation resource and verify/export actions; the exemption file no longer lists them — its entry count shrinks); a Deny attestation policy blocks an out-of-scope caller on these routes.
- [ ] End-to-end positive path: a task with a valid chain, matching workload identity, and satisfied constraints pushes and merges successfully — legitimate attested work is not falsely rejected.

## Verification intent

At the base: (a) an agent pushes with no chain and is accepted — constraint_check.rs:299 logs 'no attestation chain found for task (Phase 3 enforcement, allowing)' and the push-time attestation ABAC gate is audit-only by its own comment; (b) a chain-less merge proceeds because merge_processor skips evaluation when '!chain_found && denial.is_none()'; (c) simulating a chain-store error (find_by_task returns Err) allows the push ('Query failures are non-blocking', constraint_check.rs:304) and ABAC evaluation unavailability proceeds ('audit-only, push proceeds'); (d) a jj-workspace agent's delivery via create_mr_for_completion runs jj_git_export and creates an MR with no pre-accept verification at all; (e) a merge whose chain carries an invalid gate signature succeeds (Phase 5 output-signature verification only runs in the GET endpoint); (f) the three attestation routes sit in scripts/abac-route-registry-exemptions.txt (lines 31-33) so no middleware policy evaluation runs and the handlers perform no per-handler tenant/workspace authorization; (g) /api/v1/attestations/verify resolves to action 'read' (abac_middleware.rs:423-427); (h) the verification endpoint builds every CEL context with Action::Push (provenance.rs:274), so a constraint like 'action == "merge" ? … : …' misevaluates for merge commits. Workload-identity probe: push agent A's commits while the chain leaf's key binding names agent B (or another task) — at the base the push is accepted (no identity comparison exists in any push/merge path). Each must flip: chain-less non-system push/merge rejected with the policy named, store errors reject, jj delivery rejected with the ref restored and no MR, invalid gate signature fails the merge, routes resolver-registered and policy-evaluated, verify action mapped, correct action per verification context, identity mismatch rejected.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
  {
    "id": "4952c3bd11782374a70c9107",
    "decision": "gap",
    "requirement_quote": "The agent's workload identity matches the key binding in the attestation chain",
    "detail": "Push-time pre-accept and merge-time verification are implemented for three of the four specced checks: the agent's JWT validity is enforced by the standard auth path; chain validity back to a SignedInput and constraint satisfaction are enforced by enforce_push_constraints / enforce_merge_constraints (rejecting on invalid chain, missing signed root, binding failure or constraint failure, with ref undo at push). Missing: no code compares the pushing/merging agent's workload identity to the key binding in the attestation chain — enforcement builds an AgentContext for CEL evaluation but never checks that the chain's leaf agent_id, its DerivedInput key binding, or any agent_signature corresponds to the acting agent; chain membership is only implied by the task lookup. Additionally, in the production flow these checks never run at all because find_by_task never returns the unlinked approval-time root (see §2.1), so every production push takes the 'no attestation chain found ... allowing' branch. Merge-time output-signature (gate/agent) verification also only runs in the GET verification endpoint, not in the merge enforcement path. Two further breaks: (a) the pre-accept boundary only exists on the smart-HTTP receive-pack path — jj-workspace agents deliver ALL work via the server-side jj route (create_mr_for_completion runs jj_bookmark_set + jj_git_export, moving the MR source-branch ref directly in the bare repo, then proceeds to diff/can_merge/MR creation with no receive-pack), so no pre-accept attestation-chain or constraint verification runs for jj-delivered work and an MR is created from unverified commits (source-control.md requires push-time validation 'regardless of whether the client used git or jj'); (b) the verification is fail-open on store errors: when find_by_task returns Err the push is ALLOWED ('Query failures are non-blocking'), and the push-time attestation ABAC error path likewise proceeds ('audit-only, push proceeds'), so a store outage or attacker-induced store error silently disables verification, contradicting §6.1's 'the forge verifies' and the fail-closed philosophy of §3.4.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "let agent_ctx = build_agent_context(state, agent_id, task_id, workspace_id)",
        "detail": "The enforcement path resolves the acting agent's context for CEL evaluation but never compares it against the attestation chain's key binding or leaf agent identity — the workload-identity match bullet has no implementation in any inspected push/merge call path."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "no attestation chain found for task (Phase 3 enforcement, allowing)",
        "detail": "Chain-less pushes are allowed, and due to the task-linkage break (§2.1) no production push ever has a chain — the push-time verification points are unreachable in the production flow despite the code existing."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "no SignedInput found in attestation chain",
        "detail": "When a chain does exist, the signed-root requirement and constraint enforcement are real (rejection strings for push and merge), proving the implemented subset is genuine enforcement, not audit-only."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "jj git export failed — agent work may be invisible to merge checks",
        "detail": "jj-workspace agents deliver work via the server-side jj route (jj_bookmark_set + jj_git_export inside create_mr_for_completion), which moves the MR source-branch ref in the bare repo without any receive-pack — the pre-accept attestation-chain/constraint verification (and stack policy and attestation ABAC) never runs on this delivery path; MRs are created from unverified commits."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "failed to query attestation chain (Phase 3, allowing)",
        "detail": "When find_by_task errors, enforce_push_constraints returns Ok(()) — the push is allowed. The comment above the branch states 'Query failures are non-blocking to avoid false rejections', a fail-open error path that lets any chain-store failure silently disable push-time verification."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "push attestation ABAC evaluation unavailable (audit-only, push proceeds)",
        "detail": "The push-time attestation ABAC error path logs and proceeds — evaluation unavailability does not reject the push, the second fail-open error path on the enforcing boundary."
      }
    ]
  },
  {
    "id": "29f20a71bb7c6f10151e3b87",
    "decision": "gap",
    "requirement_quote": "// Phase 3: Build CEL context from actual output",
    "detail": "The verification algorithm is implemented end-to-end in GET /api/v1/repos/:id/attestations/:commit_sha/verification: Phase 1 chain verification (verify_chain), Phase 2 constraint collection (accumulate_chain_constraints + derive_strategy_constraints + gate constraints), Phase 3 CEL context from the actual diff, Phase 4 fail-closed evaluation, Phase 5 output signature verification (agent_signature over content_hash and every gate signature over signable_bytes with ring Ed25519), plus a commit-signature phase. The push/merge enforcement paths implement phases 1–4 (rejecting on failure) but skip phase 5, which only runs in the verification endpoint. Two deviations, previously dismissed as minor, make this a gap: (1) Phase 3 hardcodes action=Push in the verification endpoint's CEL context instead of deriving push/merge from the verification context — §3.3 defines the context's action as 'push' | 'merge' and §6.2 Phase 3 builds it from context.action, so action-sensitive constraints misevaluate when the endpoint verifies a merge commit (the enforcement paths do set Push/Merge correctly); (2) reachability — the endpoint depends on find_by_commit, which never matches in the production flow because no attestation records a real commit SHA (see §5.3). Per review the action-hardcoding deviation is not wave-through-able without a fix or spec amendment; it is folded into this section's gap.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "quote": "action: gyre_domain::constraint_evaluator::Action::Push,",
        "detail": "The verification endpoint hardcodes the push action in the CEL context; when it verifies a merge commit, action-sensitive constraints evaluate against the wrong action. §3.3/§6.2 require the action to come from the verification context ('push' | 'merge')."
      },
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "quote": "fn verify_output_signatures(chain: &[gyre_common::Attestation]) -> gyre_common::VerificationResult {",
        "detail": "Phase 5 as specced: agent_signature verified over content_hash against the key binding's public key, and each gate signature verified over the canonical signable bytes — real Ed25519 verification, not structural checks."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "no SignedInput found in attestation chain",
        "detail": "The enforcement paths implement phases 1–4 of the same algorithm with real rejections (and set Push/Merge action correctly), so the algorithm is not endpoint-only theater."
      }
    ]
  },
  {
    "id": "290653a126dab6fc9fd71f53",
    "decision": "gap",
    "requirement_quote": "This policy makes authorization provenance **mandatory** for all non-system pushes and merges.",
    "detail": "Three parts of the amendment are real: the attestation resource type exists in the policy engine's evaluations; the new subject attributes (subject.chain_depth, subject.root_signer, subject.constraint_count) are populated from real chain data at both push (evaluate_push_attestation_abac) and merge (evaluate_attestation_abac) and recorded to the decision audit trail; and builtin:require-signed-authorization is seeded as an immutable Deny at priority 998 keyed on subject.attestation_chain_present, with a companion builtin:attested-authorization Allow at 150. But the mandatory semantics is not enforced: at push time the attestation-scoped ABAC evaluation is audit-only by design ('the gate itself remains audit-only (Phase 1)') and only runs inside the branch where a chain was found (where the unless-attribute is set true anyway); at merge time the gate does enforce Deny decisions, but it only evaluates when a chain was located — a chain-less merge skips evaluation entirely and proceeds. Combined with the task-linkage break (§2.1) that means no production work ever carries a locatable chain, authorization provenance is in practice optional for all pushes and merges. The actions verify/export/revoke are also not realized as such: the /api/v1/attestations/verify route maps to action 'read', the repos/:id/attestations/:commit_sha/{bundle,chain,verification} routes are in the frozen ABAC route-registry exemption file (no middleware policy evaluation, and their handlers perform no per-handler tenant/workspace authorization), and no attestation revoke action exists. Additionally, even a fully enforced push-time gate would not cover jj-workspace agents, whose work is delivered via the server-side jj export route (no receive-pack, hence no push-boundary policy evaluation at all — see §6.1); the mandatory semantics must extend to that delivery path for 'all non-system pushes' to hold.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/abac_middleware.rs",
        "quote": "name: \"builtin:require-signed-authorization\".to_string(),",
        "detail": "The immutable priority-998 Deny policy is seeded with the unless-attestation-chain-present conditions — the policy object exists as specced."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "The gate itself remains audit-only (Phase 1): the result is logged and",
        "detail": "Push-time attestation ABAC evaluation is never enforced, and it only runs when a chain was located (attestation_chain_present=true), so the Deny can never fire on chain-less pushes."
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "quote": "if !chain_found && denial.is_none() {",
        "detail": "Merge-time gate: when no chain is found the evaluation is skipped entirely and the merge proceeds — 'Deny unless a valid attestation chain exists' never applies to chain-less merges."
      }
    ]
  },
  {
    "id": "43f8e2cad645c41a4bcdc0a2",
    "decision": "gap",
    "requirement_quote": "- Push and merge are rejected if the attestation chain is invalid or constraints fail.",
    "detail": "The rejection machinery is real: enforce_push_constraints runs synchronously in the git-receive path before the response, undoes ref updates and returns 403 on invalid chains, missing signed roots, context-binding failures or constraint failures (emitting PushRejected and GitPushRejected audit events); enforce_merge_constraints fails the merge queue entry the same way; derived-input production code exists for delegation; and the legacy bundle is still produced. Two failures keep the phase from being satisfied: (1) 'Enable builtin:require-signed-authorization' is only seeded — the policy's mandatory semantics is not enforced (push-time attestation ABAC is audit-only and merge-time evaluation is skipped when no chain exists), so chain-less pushes and merges pass; (2) due to the task-linkage break (§2.1: the approval-time root is stored with empty task_id and never linked), find_by_task never returns a chain in the production flow, so even the implemented rejections are unreachable for real work — every production push takes the allow branch. Three further failures: (3) the rejection paths are fail-open on store errors — enforce_push_constraints returns Ok(()) when find_by_task errors ('Query failures are non-blocking') and the push-time attestation ABAC error path proceeds ('audit-only, push proceeds'), so a store outage silently disables the 'mandatory' enforcement; (4) the enforcement only covers the smart-HTTP receive-pack path — jj-workspace agents deliver work via the server-side jj export route with no pre-accept verification at all (see §6.1), so 'push and merge are rejected' does not hold for jj-delivered work until merge time; (5) on a PostgreSQL deployment every provenance store operation bails (§5.4 stub), so with fail-open error handling PG deployments are structurally exempt from this phase's enforcement.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "no attestation chain found for task (Phase 3 enforcement, allowing)",
        "detail": "Chain-less work passes by design ('graceful degradation'), and because of the §2.1 linkage break no production task ever has a locatable chain — the rejections this phase promises never fire in the production flow."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "The gate itself remains audit-only (Phase 1): the result is logged and",
        "detail": "The builtin:require-signed-authorization policy is seeded but its push-time evaluation is audit-only, so 'enable' in the enforcing sense did not happen."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "no SignedInput found in attestation chain",
        "detail": "The rejection code itself is genuine enforcement (invalid chain, missing root, binding and constraint failures all reject with ref undo), proving the gap is reachability and chain-mandatory semantics, not missing rejection logic."
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "quote": "failed to query attestation chain (Phase 3, allowing)",
        "detail": "Fail-open on the enforcing path: a chain-store error results in the push being allowed, not rejected — 'rejected if the attestation chain is invalid' is not upheld when the chain cannot be queried."
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "push attestation ABAC evaluation unavailable (audit-only, push proceeds)",
        "detail": "The push-time attestation ABAC error path also fails open — evaluation unavailability logs and proceeds rather than rejecting."
      }
    ]
  }
]
