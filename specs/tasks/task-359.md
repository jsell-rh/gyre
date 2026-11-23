---
title: "Publish reviewed spec backlog for specs/system/authorization-provenance.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/authorization-provenance.md",
  "plan_id": "fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0",
  "audit": {
    "base": "973068a86ab3154b6cba448fda9369125326cab7",
    "requirements": "1d3b1257654badc9e1907878bd60498d5675621e8f55c21bd5549781caa66509",
    "sections": [
      {
        "id": "3ce5cd2c6688b57ea19500d6",
        "decision": "context",
        "requirement_quote": "Cryptographic proof that work was authorized, constrained to what was authorized, and verifiable without trusting the platform.",
        "detail": "Title and one-line thesis. Rationale-only framing; the implementable requirements live in the numbered sections, each audited separately below. Substantial machinery exists (types in gyre-common/src/attestation.rs, CEL evaluator, chain store, push/merge enforcement paths), but several wiring breaks documented under §2.1, §5.3, §6.1 and §7.2 mean the end-to-end guarantee is not yet realized in production flows.",
        "evidence": []
      },
      {
        "id": "f85d9d307bece0ea03ae60e7",
        "decision": "context",
        "requirement_quote": "Every link depends on trusting the server to have enforced the chain correctly.",
        "detail": "Problem statement / motivation. Describes the pre-existing referential chain and three gaps (output constraints, delegation provenance, constraint propagation). No separate implementable requirement beyond what §2–§4 specify; those are audited under their own headings.",
        "evidence": []
      },
      {
        "id": "5697db537a0ff07c9ec49322",
        "decision": "context",
        "requirement_quote": "The platform assembles and routes attestations, but cannot forge them.",
        "detail": "Design rationale: decomposition into provenance and constraints. The concrete properties are specced in §1–§6 and audited there. Note the 'cannot forge' property is only as strong as the enforcement wiring; the user-signed SignedInput signature is genuinely verified with real Ed25519 (git_http.rs), but chain-less work currently passes (see §7.2 / §8 Phase 3 findings).",
        "evidence": []
      },
      {
        "id": "e6d6c91df4a1ab869c63baca",
        "decision": "context",
        "requirement_quote": "1 Trust Model",
        "detail": "Parent heading for the trust model. The section carries no body text of its own; the anchor registry, tenant scoping and immutable-policy requirements are §1.1's and the residual-risk rationale is §1.2's, both audited under their own headings.",
        "evidence": []
      },
      {
        "id": "341b443dab3fbc6dd3b16513",
        "decision": "context",
        "requirement_quote": "Even with this design, a compromised platform can:",
        "detail": "Threat-model rationale (residual risk list). Explains why context binding (§2.4) and offline verification (§6.3) exist; those are audited under their own headings. The 'cannot forge a user's signature' claim rests on the real Ed25519 verification of SignedInput signatures, which is implemented; the replay/withholding mitigations are audited at §2.4/§6.3.",
        "evidence": []
      },
      {
        "id": "c8cc53eed6f4c0ee89a4aac2",
        "decision": "context",
        "requirement_quote": "Every chain of work authorization begins with a human signing a **Signed Input** — a cryptographic authorization that binds a spec approval to a set of output constraints.",
        "detail": "Parent introduction for the signed input. Concrete requirements (creation trigger, structure, key binding, context binding) are in §2.1–§2.4 and audited there.",
        "evidence": []
      },
      {
        "id": "cbe44c43454c20f4586799a5",
        "decision": "context",
        "requirement_quote": "each sub-agent's authorization is a **Derived Input** — a new authorization cryptographically linked to its parent.",
        "detail": "Parent introduction for delegation provenance. Structure, creation triggers, additive propagation, chain verification, key binding and depth limits are §4.1–§4.6 and audited there.",
        "evidence": []
      },
      {
        "id": "e4d4045d0916d50ac3edc193",
        "decision": "context",
        "requirement_quote": "An attestation packages the input (signed or derived), the output, and the verification result into a single record.",
        "detail": "Parent introduction. Note the shipped Attestation struct packages input, output and metadata (gyre-common/src/attestation.rs) — it does not carry a verification-result field; §5.1's own structure definition matches the shipped shape, and verification results are handled under §6.4 (audited as a gap there).",
        "evidence": []
      },
      {
        "id": "22bca3d0c8888a2ef7f763b1",
        "decision": "context",
        "requirement_quote": "6 Verification",
        "detail": "Parent heading for verification. The section carries no body text of its own; the push-time and merge-time verification points are §6.1's, the algorithm §6.2's, offline verification §6.3's and result storage §6.4's — audited under those headings.",
        "evidence": []
      },
      {
        "id": "4b5c9ade29faa49cf761cfe6",
        "decision": "context",
        "requirement_quote": "7 Integration with Existing Systems",
        "detail": "Parent heading for the integration amendments. The section carries no body text of its own; the seven amendments (§7.1–§7.7) are audited under their own headings.",
        "evidence": []
      },
      {
        "id": "96837c9c486a2da76036169b",
        "decision": "context",
        "requirement_quote": "Authorization provenance is introduced incrementally:",
        "detail": "Parent introduction for the migration plan. Each phase is a separate heading audited individually (Phase 1–Phase 4 below).",
        "evidence": []
      },
      {
        "id": "23cb6aa80ea2cc06222ac4b6",
        "decision": "context",
        "requirement_quote": "Agent commits code outside spec scope | Explicit or strategy-implied path constraints fail at push time |",
        "detail": "Summary table mapping attacks to prevention mechanisms; it restates guarantees specified elsewhere rather than adding requirements. Most rows map to implemented mechanisms (path constraints, signed inputs, additive propagation, fail-closed CEL). Rows depending on unimplemented pieces are captured as gaps under their owning sections: gate-signature rows rest on merge-time output-signature verification that only runs in the GET verification endpoint (§6.1/§6.2), and the mandatory-chain rows rest on the unenforced builtin policy (§7.2, §8 Phase 3).",
        "evidence": []
      },
      {
        "id": "4a057d6eb16efb53922120ed",
        "decision": "context",
        "requirement_quote": "Merge attestation bundle subsumed by attestation chain (§5.2). Gate agents produce individually signed `GateAttestation` records. Spec approval produces `SignedInput`.",
        "detail": "Cross-reference list to other canonical specs. Each referenced amendment is audited under this spec's own headings (§5.2, §7.1–§7.7). No standalone requirement in this section.",
        "evidence": []
      },
      {
        "id": "80fa5d551228a894b5a85132",
        "decision": "gap",
        "requirement_quote": "The platform cannot add, remove, or modify trust anchors without a human admin action — this is enforced by an immutable Deny ABAC policy (same pattern as `builtin:require-human-spec-approval`).",
        "detail": "TrustAnchor types, a tenant-scoped CRUD repository (SQLite + mem), and /api/v1/tenants/:id/trust-anchors endpoints exist, and modification is gated on the Admin role plus a tenant match. However the spec's enforcement mechanism — an immutable Deny ABAC policy that (like builtin:require-human-spec-approval) denies non-human subjects — is not implemented: protection is an RBAC role check in the handler, so any principal holding the Admin role (including an agent principal) can add, remove or rotate anchors, and no builtin policy guards the trust-anchor resource. Additionally, anchor-level `constraints` (§3.2 reference in the TrustAnchor struct) are stored and returned by the CRUD API but never evaluated: derive_strategy_constraints/collect_all_constraints take only InputContent, workspace trust level and gate constraints — no code path loads a TrustAnchor's constraints into the evaluated constraint set. 'A workspace inherits its tenant's anchors' is only exercised by the §6.3 bundle export (list_by_tenant via the workspace's tenant); verification never resolves key_binding.trust_anchor_id against the anchor registry.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/trust_anchors.rs",
            "quote": "\"trust anchor management requires Admin role\".to_string(),",
            "detail": "All five trust-anchor endpoints gate on require_admin (UserRole::Admin) plus a tenant_id equality check. This is handler-level RBAC, not the specced immutable Deny ABAC policy; grep of m34_builtin_policies and the policy engine shows no builtin policy for trust-anchor resources."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "pub fn collect_all_constraints(",
            "detail": "The evaluated constraint set is the union of explicit, strategy-implied and gate constraints only. TrustAnchor.constraints are accepted and stored by the CRUD API but no production call path adds them to the evaluated set — the anchor-level constraints of §1.1/§3.2 are dead data."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/trust_anchor.rs",
            "quote": "impl TrustAnchorRepository for SqliteStorage {",
            "detail": "Real tenant-scoped SQLite persistence for trust anchors exists (create/find/list/update/delete), so the registry itself is real storage, not a stub — the gap is the enforcement mechanism and the unevaluated anchor constraints."
          }
        ]
      },
      {
        "id": "0a001bfe1c6ccfa6bd7e4cb3",
        "decision": "gap",
        "requirement_quote": "the approval now produces a Signed Input that travels with every task derived from that spec.",
        "detail": "Spec approval does produce a real SignedInput when the approver has an active KeyBinding and supplies user_content_signature (specs.rs): the InputContent is built (spec registry SHA, workspace/repo from the ledger, persona constraints, meta_spec_set_sha, scope), the user's Ed25519 signature over SHA256(content) is verified against the KeyBinding public key, and the root attestation is saved to the chain store. But it does NOT travel with tasks: the root attestation is stored with metadata.task_id = \"\" and commit_sha = \"\", and no production code ever re-saves or links it to a task. Every downstream consumer looks chains up by task_id (spawn's create_derived_input_for_agent, push/merge enforcement in constraint_check.rs, gate_executor) or by commit_sha (merge ABAC gate, git-note attachment, verification endpoints), so in the production flow the root is unreachable: derived inputs are never created ('no attestation chain for task'), push enforcement finds no chain and allows, and the verification/bundle/chain endpoints 404. The wiring step that associates the approval-time SignedInput with tasks derived from the spec is missing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "task_id: String::new(), // No task yet at approval time.",
            "detail": "The root SignedInput attestation is persisted with an empty task_id. The only production save sites for chain attestations are this root (empty task_id), spawn's derived nodes (real task_id, but creation is gated on find_by_task which never returns the root), and gate_executor's leaf update — nothing links the root to a task."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "no attestation chain for task — skipping DerivedInput creation",
            "detail": "create_derived_input_for_agent looks up the parent chain exclusively via chain_attestations.find_by_task(task_id). Because the root is stored with task_id \"\", this returns empty for every real task and derived-input creation is skipped in the production flow."
          },
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "no attestation chain found for task (Phase 3 enforcement, allowing)",
            "detail": "Push-time enforcement also keys on find_by_task; with the root unlinked, no chain is ever found, so the enforcement path takes the allow branch for every production push. Tests pass only because they insert attestations whose task_id matches directly (make_attestation in this file's test module)."
          }
        ]
      },
      {
        "id": "7a3622bf08c8755b8d31b952",
        "decision": "satisfied",
        "requirement_quote": "spec_sha:             string          -- spec registry SHA at approval time",
        "detail": "The InputContent structure is fully implemented in gyre-common (spec_path, spec_sha, workspace_id, repo_id, persona_constraints as structured PersonaRef, meta_spec_set_sha, scope as ScopeConstraint with allowed/forbidden globs). spec_sha is populated with the registry SHA (content blob SHA plus meta-spec binding set) — the same SHA the approval ledger records — and meta_spec_set_sha is populated from the workspace's effective required+approved meta-spec set at approval time, failing closed on resolution errors. persona_constraints are built from the approval persona; scope comes from the request (defaulting to empty allowed/forbidden).",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct InputContent {",
            "detail": "All specced InputContent fields exist as production types, including the structured PersonaRef and ScopeConstraint; serde round-trips are covered in the type tests."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "spec_sha: registry_sha.clone(),",
            "detail": "Approval populates spec_sha with the registry SHA (blob SHA plus meta-spec binding set), with an explicit comment that resolve_task_binding_context feeds the newest approval event's SHA into the §2.4 binding comparison so the two recorded SHAs agree."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let meta_spec_set_sha = crate::reconciliation::meta_spec_snapshot_sha(",
            "detail": "meta_spec_set_sha is computed from the workspace's current effective required+approved meta-spec set at approval time (task-058), with fail-closed error handling on resolution failures."
          }
        ]
      },
      {
        "id": "dcf557ae5dbc627aab292307",
        "decision": "gap",
        "requirement_quote": "The `platform_countersign` proves the binding was registered at a specific time with a valid IdP session. It does NOT make the platform a trust root — verification checks the user's signature against the bound public key and the IdP's trust anchor, not the platform's countersignature.",
        "detail": "The KeyBinding exchange exists and much of it is real: POST /api/v1/auth/key-binding requires an authenticated session, rejects non-32-byte keys, cryptographically verifies an Ed25519 proof-of-possession, countersigns with the real server key, caps TTL at 24h, persists via the tenant-scoped KeyBindingRepository, and expiry/revocation are enforced downstream. However the section's core anti-forgery property is not implemented. Spec lifecycle step 3 requires the CLIENT to construct the KeyBinding document and sign it with the private key, and the section explicitly denies that the countersignature makes the platform a trust root. At the base, user_signature is verified only over the raw public key bytes as proof-of-possession; user_identity, issuer, trust_anchor_id, issued_at and expires_at are server-assigned (derived from the auth context and server clock) and are covered ONLY by the platform countersign. Consequence: a compromised server — the exact adversary this spec models (§1) — can rewrite a stored binding's identity fields and re-countersign with the platform key it holds, so identity attribution of an authorization root rests entirely on the platform, precisely the authority the section denies; offline verifiers (§6.3) have no user-signed identity claims to check. This was previously classified satisfied with the deviation dismissed as preserving the binding's security properties; per review that dismissal is retracted and the section is reclassified as a gap. Correcting it requires the client to construct and sign the full binding document (public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at) and binding verification to check the user's document signature and resolve the trust anchor, treating the countersignature strictly as a timestamp witness.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/key_binding.rs",
            "quote": "The user signs the public key bytes as a proof-of-possession.",
            "detail": "The only user signature verified by the exchange is over the raw public key bytes; none of the binding document's identity fields (user_identity, issuer, trust_anchor_id, validity) are covered by a user signature, so spec lifecycle step 3 (client constructs and signs the KeyBinding document) is not implemented."
          },
          {
            "file": "crates/gyre-server/src/api/key_binding.rs",
            "quote": "let user_identity = derive_user_identity(&auth);",
            "detail": "user_identity (and issuer, from jwt_config/base_url, and the issued_at/expires_at timestamps) are server-assigned during the exchange rather than client-constructed claims signed by the user's key."
          },
          {
            "file": "crates/gyre-server/src/api/key_binding.rs",
            "quote": "let platform_countersign = state.agent_signing_key.sign_bytes(&binding_bytes);",
            "detail": "The only signature covering the identity fields of the countersigned binding JSON is the platform countersignature made with the server-held key — a compromised server can rewrite those fields and re-countersign, making the platform the de facto trust root for binding identity, which the section explicitly forbids."
          }
        ]
      },
      {
        "id": "cb15e6513133b14ad14bf017",
        "decision": "satisfied",
        "requirement_quote": "Prevents replay of an old authorization after a task has been reassigned.",
        "detail": "All four context bindings are enforced on the enforcement paths. git_http::verify_context_binding compares the signed root InputContent against the actual push/merge target: repo_id and workspace_id equality (mismatch rejects), spec_sha against the task's currently approved SHA (newest non-revoked approval event, falling back to the ledger SHA; empty signed SHA fails), and expected_generation against the task's current persisted generation. valid_until and KeyBinding expiry are enforced in verify_attestation_audit_only, which is part of the chain result that enforce_push_constraints/enforce_merge_constraints reject on. Task reassignment bumps the deployment generation (spawn and task-update paths), invalidating SignedInputs pinned to a previous generation. verify_context_binding is invoked in both enforce_push_constraints and enforce_merge_constraints, with rejections returning 'push rejected: attestation context binding failed' / 'merge blocked: ...'. Caveat: these enforcement paths only run when a chain is found by task_id, which in the production flow never happens (see §2.1 gap) — the binding logic itself is real and tested with genuinely signed inputs.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "pub(crate) fn verify_context_binding(",
            "detail": "Real implementation of all four §2.4 bindings (repo_id, workspace_id, spec_sha, expected_generation) with fail-closed semantics and per-binding VerificationResult children naming the failing comparison."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "task.generation = task.generation.saturating_add(1);",
            "detail": "Spawning onto a task assigned to a different agent bumps the deployment generation, so SignedInputs pinned to the previous generation fail the expected_generation binding (same bump exists in the task-update and MCP paths)."
          },
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "no SignedInput found in attestation chain",
            "detail": "Enforcement paths reject when a chain exists but has no signed root; the §2.4 binding check runs immediately after chain verification in both enforce_push_constraints and enforce_merge_constraints."
          }
        ]
      },
      {
        "id": "c637650c54d416ec16c76be1",
        "decision": "satisfied",
        "requirement_quote": "An output constraint is a named CEL predicate that the actual output (diff, committed files) must satisfy at verification time.",
        "detail": "The definition is implemented for real: OutputConstraint{name, expression} is evaluated by an actual CEL interpreter (cel_interpreter crate) in gyre-domain/src/constraint_evaluator.rs, against a context built from the platform-computed git diff — not self-reported agent data. The evaluator is wired into push-time and merge-time enforcement and the verification endpoint. Details of structure, sources, context and fail-closed semantics are audited under §3.1–§3.4.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct OutputConstraint {",
            "detail": "The named-CEL-predicate production type (name + expression), used by every constraint source."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "pub fn evaluate_constraint(",
            "detail": "Real CEL evaluation: Program::compile + execute against the built context; non-boolean results are errors. Not a structural or regex stand-in."
          }
        ]
      },
      {
        "id": "e82399ff9d96204861482983",
        "decision": "satisfied",
        "requirement_quote": "  name:         string      -- human-readable description",
        "detail": "OutputConstraint{name: String, expression: String} exists as specified in gyre-common, is carried in SignedInput.output_constraints, DerivedInput.output_constraints, GateConstraint.constraint and TrustAnchor.constraints, and is evaluated by the real CEL evaluator.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct OutputConstraint {",
            "detail": "Structure matches the spec exactly (human-readable name + CEL expression); serde round-trip tests cover both fields."
          }
        ]
      },
      {
        "id": "7b1be341c378350402ce6f9b",
        "decision": "satisfied",
        "requirement_quote": "Constraints come from three sources. All are additive — the full constraint set is the union of all three:",
        "detail": "The union machinery is implemented: collect_all_constraints concatenates explicit, strategy-implied and gate constraints, and both enforcement paths and the verification endpoint feed the union into the single fail-closed evaluation. Sources 1 and 2 are produced in production (approval-time explicit constraints; verification-time strategy derivation). Source 3's production emission is missing — no gate ever attaches a GateConstraint — which is accounted as a gap under the Source 3 heading rather than double-counted here.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "pub fn collect_all_constraints(",
            "detail": "The additive union of the three sources (explicit + strategy-implied + gate), used by enforce_push_constraints, enforce_merge_constraints, evaluate_* mirrors and the verification endpoint."
          }
        ]
      },
      {
        "id": "95b262ac772dc34c9597249e",
        "decision": "satisfied",
        "requirement_quote": "The human approver includes constraints in the `SignedInput` at spec approval time.",
        "detail": "POST /api/v1/specs/:path/approve accepts output_constraints (Vec<OutputConstraint>) and they are placed on the SignedInput, covered by the user's Ed25519 signature over the InputContent (which is extended at verification time against the same key). Because the constraints ride inside the signed SignedInput, changing them invalidates the signature. The ConstraintEditor UI (§7.6) lets the approver author them, and the dry-run endpoint validates/evaluates them pre-approval.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "pub output_constraints: Vec<gyre_common::OutputConstraint>,",
            "detail": "The approval request carries explicit user constraints (spec §7.1 amendment shape)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "output_constraints: req.output_constraints.clone(),",
            "detail": "The constraints are embedded in the SignedInput that the user's key signs — they travel with the authorization and are accumulated at evaluation time."
          }
        ]
      },
      {
        "id": "a1805adaa6c213bf0e31fbfb",
        "decision": "satisfied",
        "requirement_quote": "The platform derives constraints mechanically from the `InputContent` at verification time.",
        "detail": "derive_strategy_constraints implements every listed derivation: persona membership (input.persona_constraints.exists(p, p.name == agent.persona) — a membership form so multiple personas are satisfiable), meta-spec set equality (agent.meta_spec_set_sha == input.meta_spec_set_sha), scope path constraints (allowed globs as an any-match over changed files via glob_to_regex, forbidden globs always enforced as negations), supervised-workspace attestation level >= 2, and repo stack policy attestation level >= N. They are re-derived at verification time from the live workspace/stack-policy state — not cached at signing time — in both push and merge enforcement and the verification endpoint. The agent context they compare against is built from real state (agent record, workspace required meta-spec set with fail-closed resolution, workload attestation KV).",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "input.persona_constraints.exists(p, p.name == agent.persona)",
            "detail": "Persona strategy-implied constraint as specced (membership form)."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "agent.meta_spec_set_sha == input.meta_spec_set_sha",
            "detail": "Meta-spec set strategy-implied constraint as specced; the agent side is computed from the workspace's current effective registry set at verification time."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "strategy: attestation level must be >= {level}",
            "detail": "Attestation-level policy constraint derived from the repo's stack policy (task-059), fed from get_repo_required_attestation_level's real KV read."
          }
        ]
      },
      {
        "id": "3a1de6328a8b4ff8639ece89",
        "decision": "gap",
        "requirement_quote": "A gate agent that reviews code can attach constraints that subsequent gates or the merge processor must verify:",
        "detail": "The GateConstraint type, its inclusion in GateAttestation.constraint, and its consumption are real: gate constraints are collected from chain nodes and evaluated at push/merge time and in the verification endpoint, additively with the other sources. But no production gate ever produces one. The only production site that builds GateAttestation records (gate_executor's attach path) sets constraint: None with the comment that gate constraints are 'attached by review/validation agents', and no review/validation agent path constructs a GateConstraint — grep shows the only non-type constructions of GateConstraint are in test modules (constraint_evaluator, merge_processor tests). The signed-by-gate-agent emission half of Source 3 is therefore unimplemented; only the evaluation half exists.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "constraint: None, // Gate constraints are attached by review/validation agents",
            "detail": "The sole production GateAttestation constructor leaves the gate constraint empty; no other production code path creates a GateConstraint (all other constructions are in #[cfg(test)] modules), so gates can never tighten the constraint set as §3.2 Source 3 requires."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "if !chain_found && denial.is_none() {",
            "detail": "Merge-time consumption of gate constraints exists (accumulate_chain_constraints collects GateAttestation.constraint into the evaluated set), confirming the gap is on the production emission side only."
          }
        ]
      },
      {
        "id": "039e53ce4abbadab618385ed",
        "decision": "satisfied",
        "requirement_quote": "The `output` fields are computed from the actual git diff at verification time — not self-reported by the agent.",
        "detail": "The CEL context is built by the platform from real git state: compute_push_diff / compute_commit_diff shell out to git (-C repo diff --name-status / --shortstat / log) to build changed/added/deleted files, insertion/deletion stats and the commit message; the agent context is resolved from the agent record, JWT-derived persona KV, workspace meta-spec set and workload attestation; target from repo state; action push/merge. build_cel_context serializes input/output/agent/target/action into the CEL Context exactly as §3.3 structures them.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "async fn compute_push_diff(",
            "detail": "Platform-side diff computation via real git invocations (name-status, shortstat, log) — the output fields are never taken from agent self-report."
          },
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct InputContent {",
            "detail": "Input side of the context is the signed InputContent, serialized into the CEL context alongside output/agent/target/action."
          }
        ]
      },
      {
        "id": "10c092a2ae67584aaf526985",
        "decision": "satisfied",
        "requirement_quote": "CEL evaluation errors (malformed expressions, type errors, missing fields) are treated as failures.",
        "detail": "evaluate_all implements sequential first-failure-stops semantics: each constraint is compiled and executed with the real CEL interpreter; parse errors, evaluation errors and non-boolean results all produce invalid VerificationResults ('constraint evaluation error'), and Ok(false) produces 'constraint failed'. There is no error-to-allow path — the Err arm returns immediately with valid:false. Enforcement callers convert invalid results into push/merge rejections; even context-build failures reject ('failed to build CEL context').",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "Fail-closed: evaluation errors are treated as failures (§3.4).",
            "detail": "The Err arm of evaluate_all returns an invalid result immediately — evaluation errors reject, never allow."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "pub fn evaluate_constraint(",
            "detail": "Compile and execute both map errors to Err; a non-Bool value is an error, so a constraint cannot accidentally pass by evaluating to a non-boolean."
          }
        ]
      },
      {
        "id": "1518995c27410cfe6a697020",
        "decision": "satisfied",
        "requirement_quote": "  parent_ref:       bytes           -- content hash of the parent attestation",
        "detail": "DerivedInput exists as specified: parent_ref (bytes), preconditions (CEL predicate strings), update (CEL expression string), output_constraints (additive), signature, and the full key binding embedded for offline verification. Produced (in principle) by spawn's create_derived_input_for_agent with the spawner's signature over the derivation content. Whether the creation path ever fires in production is a §4.2 concern (it does not, due to the task-linkage break), but the structure itself is complete and persisted.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct DerivedInput {",
            "detail": "All specced fields present as production types, with the KeyBinding embedded rather than referenced for self-contained offline verification."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let derived_input = gyre_common::DerivedInput {",
            "detail": "Production constructor: parent_ref computed as SHA-256 over the parent attestation, signed with the spawner's key, spawner's KeyBinding attached."
          }
        ]
      },
      {
        "id": "5bf9cf12933a9a01581e530f",
        "decision": "gap",
        "requirement_quote": "The new approval's authorization is derived from the prior approval's `SignedInput`, carrying forward the prior's constraints.",
        "detail": "Of the three creation triggers: (1) task decomposition is implemented in code (create_derived_input_for_agent signs a DerivedInput from the task's chain leaf with the spawner's key) but never fires in the production flow because find_by_task never returns the approval-time root (task_id=\"\" — see §2.1 gap); (3) re-spawn shares the same spawn path and would derive from the same parent chain when a chain exists. (2) spec update is not implemented at all: when a spec is modified and re-approved, specs.rs constructs a brand-new independent SignedInput from the new content — nothing loads the prior approval's SignedInput, derives from it, or carries forward its constraints; a re-approver who omits a previously-signed constraint silently drops it (the §9 'spec revision silently drops security constraint' prevention therefore does not hold).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let signed_input = gyre_common::SignedInput {",
            "detail": "The approval path always constructs a fresh root SignedInput; there is no code that loads a prior approval's SignedInput, creates a DerivedInput from it, or unions the prior's output_constraints into the new authorization."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "no attestation chain for task — skipping DerivedInput creation",
            "detail": "The decomposition trigger (trigger 1) is gated on find_by_task, which returns nothing for the unlinked root, so derived inputs are never created in the production flow despite the code existing."
          }
        ]
      },
      {
        "id": "99c42d99fd21f75f3a70f5b0",
        "decision": "satisfied",
        "requirement_quote": "A derived input can **add** constraints (e.g., an orchestrator narrows a sub-agent's scope to a subset of files), but can never **remove** constraints established by the parent.",
        "detail": "Accumulation is concatenate-only: accumulate_chain_constraints extends the explicit constraint list with the root SignedInput's constraints and then appends each DerivedInput's output_constraints (never replaces), plus gate constraints from every node. The union then goes through collect_all_constraints. There is no code path that subtracts or filters parent constraints. Spawn's derived inputs are created with empty additional constraints (inherit-only).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "explicit_constraints.extend(di.output_constraints.iter().cloned());",
            "detail": "Derived constraints are appended to the accumulated set ('Additive only: append derived constraints' per the adjacent comment) — the derivation function concatenates, never replaces."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "preconditions: vec![],",
            "detail": "Spawn-created derived inputs carry no additional constraints (inherit-only), the conservative end of the additive-only rule."
          }
        ]
      },
      {
        "id": "74593d79542d482cc5567984",
        "decision": "gap",
        "requirement_quote": "// 4. Evaluate preconditions against parent's verified state",
        "detail": "The implemented chain verification covers part of the algorithm: per-node Ed25519 signature verification over the signed/derivation content hash, key-binding expiry, valid_until, depth limit, presence of a root SignedInput, and structural parent linkage (non-empty parent_ref plus a lower-depth node in the chain). Missing steps: (2) verify_key_binding against the trust anchor — the key binding's trust_anchor_id/issuer are never resolved against the TrustAnchor registry or JWKS, and the platform countersignature is never verified; (4) preconditions are never evaluated against the parent's state (spawn creates them empty and no verifier executes them); (5) the update expression is never evaluated; (6) derived content is never recomputed and compared to claimed content. Parent linkage is only structural: verify_chain checks that some lower-depth node exists, explicitly not matching parent_ref content hashes to parent IDs — and the production parent_ref (SHA-256 of the parent attestation JSON) does not equal the parent's stored id (uuid for derived nodes, SHA-256-of-SignedInput for roots), so load_chain's id-based walk cannot traverse production-created links at all.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let linkage_valid = has_parent_ref && has_parent_at_lower_depth;",
            "detail": "Linkage verification is structural only — the adjacent comment states 'we verify structural consistency (non-empty, depth ordering) rather than trying to match content hashes to IDs'; steps 4–6 of the specced algorithm (preconditions, update expression, derived-content hash match) have no implementation anywhere in the verifier."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "UnparsedPublicKey::new(&signature::ED25519, &signed.key_binding.public_key);",
            "detail": "Signature verification uses the key embedded in the attestation; step 2 (verify_key_binding — resolving trust_anchor_id against the trust-anchor registry / JWKS, checking the countersignature) is absent from all inspected verification call paths."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "preconditions: vec![],",
            "detail": "Production derived inputs carry no preconditions and no verifier evaluates the preconditions or update fields, so those CEL programs are dead spec surface."
          }
        ]
      },
      {
        "id": "7910f63c3be5b0641f51c47b",
        "decision": "gap",
        "requirement_quote": "The `spawned_by` field is critical — it links the orchestrator's key binding back to the human who approved the original spec, even though the orchestrator (not the human) is the immediate signer.",
        "detail": "Orchestrators do sign derived inputs with a workload-bound key (spawn stores a per-agent Ed25519 keypair and a KeyBinding with user_identity 'agent:<id>', issuer = platform base URL, trust_anchor_id 'gyre-oidc', expiry tied to the agent JWT TTL). But the KeyBinding type has no workload claims: no task_id, spawned_by, stack_hash or persona fields exist on it (the spec's OrchestratorKeyBinding extension), so the derivation signature does not cryptographically bind the orchestrator's task, spawning human, stack or persona. Those attributes live only in the unverified AttestationMetadata/AgentContext. The spawner's KeyBinding is also frequently synthesized at spawn time from the spawner's public key with empty signature/countersignature fields when no stored binding exists.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/key_binding.rs",
            "quote": "pub struct KeyBinding {",
            "detail": "The KeyBinding struct carries only public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at, user_signature, platform_countersign — none of the specced workload claims (task_id, spawned_by, stack_hash, persona) exist on the type."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "user_signature: vec![],       // workload-bound — no user signature",
            "detail": "The child agent's workload KeyBinding (and the synthesized root-agent fallback binding) are constructed with empty signature and countersignature fields — the binding attests nothing beyond key possession."
          }
        ]
      },
      {
        "id": "827a7cc2ad6eaf0f8e262900",
        "decision": "gap",
        "requirement_quote": "The strategy-implied constraint `chain.depth <= N` is derived from workspace configuration.",
        "detail": "The hard safety-net limit is implemented: verify_chain clamps max_depth to 10, per-node verification rejects depth > 10, and spawn refuses to create derived inputs at depth >= 10. But no workspace-configuration-derived depth limit exists: the verification endpoint hardcodes 'let max_depth = 10u32; // Will be configurable per workspace in future', no strategy-implied chain.depth CEL constraint is derived anywhere in derive_strategy_constraints, and workspace trust level (Supervised vs Autonomous) does not influence chain depth.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let effective_max = max_depth.min(10);",
            "detail": "The hard limit of 10 is enforced regardless of the configurable parameter (the safety net of the third bullet)."
          },
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "let max_depth = 10u32; // Will be configurable per workspace in future",
            "detail": "The workspace-configurable depth limit and the trust-level-derived chain.depth constraint are explicitly deferred — no production code derives a per-workspace N or a chain.depth strategy constraint."
          }
        ]
      },
      {
        "id": "7298cfa0831c074e5e21aa9e",
        "decision": "gap",
        "requirement_quote": "  id:               string          -- content-addressable hash of the attestation",
        "detail": "The Attestation record structure is real and produced in production: input (tagged Signed/Derived), AttestationOutput (content_hash/commit_sha/agent_signature/gate_results), AttestationMetadata (created_at/workspace_id/repo_id/task_id/agent_id/chain_depth) all exist as production types persisted through the chain store, and GateAttestations are genuinely produced and signed at gate execution (signed over canonical signable_bytes with the platform key, appended to the chain leaf, re-persisted, written as a git note). agent_signature is optional ('if capable') and always None in production — accounted under §7.4. The gap is the id field's own requirement: 'content-addressable hash of the attestation'. Approval-time roots satisfy it (SHA-256 over the serialized SignedInput), but spawn-created derived nodes set id to a random UUID (uuid::Uuid::new_v4()) — a random UUID is not content-addressable: identical derivations get different ids, and no verifier can recompute or validate a derived node's id offline. This also breaks chain traversal semantics (§4.4): parent_ref (SHA-256 of the parent attestation) does not equal the parent's stored UUID id, so the id-based load_chain walk cannot traverse production-created links (audited under §4.4). The id-field requirement is split out of the otherwise-satisfied structure as this section's gap.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "id: uuid::Uuid::new_v4().to_string(),",
            "detail": "Spawn-created derived attestation nodes use a random UUID as the id — not a content-addressable hash. Identical derivations produce different ids and no offline verifier can recompute the id, contradicting the §5.1 field spec."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let hash = ring::digest::digest(&ring::digest::SHA256, &att_bytes);",
            "detail": "Approval-time roots ARE content-addressed (SHA-256 over the serialized SignedInput), showing the deviation is specific to spawn-created derived nodes, not a uniform id scheme."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "let mut gate_attestation = GateAttestation {",
            "detail": "The remainder of §5.1's structure is real production code: per-gate signed GateAttestation records are constructed at gate execution and signed via the shared signable_bytes() helper, so the gap is confined to the id field's content-addressing requirement."
          }
        ]
      },
      {
        "id": "acda07a63c3a25e9c32461e5",
        "decision": "gap",
        "requirement_quote": "| `conversation_sha` | `AttestationOutput` extended field (unchanged) |",
        "detail": "The backward-compatibility retention is real: the legacy AttestationBundle/MergeAttestation types and the AttestationRepository port are kept, the bundle carries a Phase-4 deprecation notice, and the merge processor performs the dual-write (chain attestation note when a chain is locatable, legacy bundle note otherwise). But the mapping table that defines the subsumption is not fully realized, and the subsumption statement itself is not implemented. Two rows of the table map `conversation_sha` and `completion_summary` to 'AttestationOutput extended field (unchanged)' — the shipped AttestationOutput has no such extended fields (only content_hash, commit_sha, agent_signature, gate_results); these values are constructed only on the legacy MergeAttestation and never enter any chain attestation, so any consumer that stops producing the legacy bundle loses them. And 'The merge attestation becomes the final attestation in a chain' is unrealized: merge appends no attestation node to the chain — it only attaches the pre-existing chain (located via find_by_commit or the author-agent task fallback) as a git note on the merge commit, while the legacy bundle remains the merge-time record stored in the attestation store. The subsumption is therefore structural only for the rows that are realized (spec_ref/spec_sha at the chain root, per-node signing keys, gate results per node); the conversation_sha/completion_summary rows and the final merge node are missing behavior.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct AttestationOutput {",
            "detail": "The shipped AttestationOutput contains exactly content_hash, commit_sha, agent_signature and gate_results — there are no conversation_sha or completion_summary 'extended fields', so the two mapping rows that promise them ('AttestationOutput extended field (unchanged)') have no realization in the chain format."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "completion_summary: None,",
            "detail": "conversation_sha and completion_summary are carried only by the legacy MergeAttestation constructed at merge time (crates/gyre-server/src/merge_processor.rs, the crate::attestation::MergeAttestation literal); no chain attestation node ever carries them."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "// During the dual-write period, we write EITHER the chain attestation",
            "detail": "Merge appends no final attestation node to the chain: the chain note path only re-attaches the pre-existing chain (found via find_by_commit or the author agent's task) as a git note on the merge commit; the merge-time record stored in the attestation store is still the legacy bundle, so 'the merge attestation becomes the final attestation in a chain' is not implemented."
          },
          {
            "file": "crates/gyre-domain/src/attestation.rs",
            "quote": "**Deprecated (Phase 4):** This legacy format is subsumed by the chain attestation",
            "detail": "The deprecation notice directing consumers to the chain format is retained — the retention half of the section is genuinely implemented; the gap is the unrealized mapping rows and the missing final merge node."
          }
        ]
      },
      {
        "id": "c49cf3ba98e72736cfcb8bee",
        "decision": "gap",
        "requirement_quote": "2. **Git notes** (`refs/notes/attestations`) — the chain attestation is attached to the relevant commit",
        "detail": "All three storage locations have real implementations, but the git-notes and commit-indexed paths are unreachable for real commits: (1) the attestation store is real on SQLite and mem (tenant isolation, task/commit/repo indexes, parent_ref chain walking), but the PgStorage adapter is a bail! stub (see §5.4), so on a PostgreSQL deployment location 1 fails entirely; (2) attach_chain_attestation_note writes the chain JSON to refs/notes/attestations and read helpers parse it back — but it only runs when chain_attestations.find_by_commit(sha) or the task fallback finds an attestation, and no production code ever records a real commit_sha on an attestation (approval stores commit_sha: \"\" 'No commit yet', spawn stores commit_sha: String::new(), gate_executor preserves the leaf's empty value), so no chain is ever attached to any real commit and the legacy-bundle fallback fires instead; (3) AIBOM aggregation includes chain_attestation per commit, reading the same never-matching store. The DB location is genuinely implemented; the commit-binding wiring that locations 2 and 3 depend on is missing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/attestation.rs",
            "quote": "pub const CHAIN_ATTESTATION_NOTES_REF: &str = \"refs/notes/attestations\";",
            "detail": "Real git-notes writer/reader for chain attestations under the specced ref exists (attach/read helpers invoke git notes)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "commit_sha: String::new(), // No commit yet at approval time.",
            "detail": "The approval-time root is stored with an empty commit_sha and nothing later updates it when the work is committed."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "commit_sha: String::new(),",
            "detail": "Derived attestations are also created with empty commit_sha; with no production save path ever setting a real commit SHA, find_by_commit (merge-time note attachment, merge ABAC gate, verification/bundle/chain endpoints) never matches a real commit."
          }
        ]
      },
      {
        "id": "1a1c2218815dfe88c38a9789",
        "decision": "gap",
        "requirement_quote": "pub trait ChainAttestationRepository: Send + Sync {",
        "detail": "The port trait exists verbatim with all six specced methods (save, find_by_id, load_chain walking parent_ref, find_by_task, find_by_commit, find_by_repo with time range). Two of the three adapters are real: SQLite (Diesel, tenant-scoped, upsert, parent-walk with loop guard) and the in-memory adapter wired into AppState. The third is not: the PgStorage implementation is a self-described stub in which every method of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository calls anyhow::bail!. The store! wiring in gyre-server/src/lib.rs selects PgStorage whenever GYRE_DATABASE_URL is a postgres:// URL — a documented production backend (docs/server-config.md) — so on a PostgreSQL deployment every provenance store operation fails. Combined with the fail-open error handling in the enforcement path ('Query failures are non-blocking', constraint_check.rs), provenance silently degrades to nothing on PG, and once boundary enforcement makes provenance mandatory this either bricks PG deployments or — if the fail-open paths survive — exempts them from the mandatory policy. The prior audit's claim that Postgres is a 'real adapter' was factually false and is corrected here; per the goal's rule 1 a bail!() stub is an open gap.",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/chain_attestation.rs",
            "quote": "pub trait ChainAttestationRepository: Send + Sync {",
            "detail": "The port trait with the exact §5.4 method set."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/chain_attestation.rs",
            "quote": "impl ChainAttestationRepository for SqliteStorage {",
            "detail": "Full SQLite implementation: content-addressed upsert, tenant isolation on every query, parent_ref chain walk with a 100-node loop guard, task/commit/repo/time-range indexes — real storage, not a stub."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
            "quote": "Stub PgStorage implementations for authorization provenance port traits.",
            "detail": "The file header self-describes as a stub with 'Full implementation deferred to future milestone' — this contradicts the prior audit's claim of a real Postgres adapter."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
            "quote": "anyhow::bail!(\"ChainAttestationRepository not implemented for PgStorage\")",
            "detail": "Every method of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository bails in this file (16 bail! calls total); no PgStorage provenance operation can succeed."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "chain_attestations: store!(",
            "detail": "The chain attestation store is wired through the store! macro, whose first branch is 'if let Some(ref d) = pg_db' — PgStorage is selected whenever a postgres:// GYRE_DATABASE_URL is configured, so a documented production backend routes all provenance storage to the bail! stub."
          }
        ]
      },
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
        "id": "bb664ae221456d242e8ae921",
        "decision": "gap",
        "requirement_quote": "- Attached to releases alongside the AIBOM",
        "detail": "The export route is implemented with the specced path and assembles a real VerificationBundle — attestation chain (load_chain), the tenant's trust anchors (workspace → tenant → list_by_tenant), the actual git diff of the commit (git diff commit^..commit), and the assembly timestamp. Two of the section's requirements are not met. (1) Reachability: the endpoint is keyed by find_by_commit, and no production path ever records a real commit SHA on an attestation (approval and spawn both store empty commit_sha and nothing updates them), so for every real commit the endpoint returns 404 'no attestation found for commit' — the export machinery is unreachable for production commits. (2) 'Attached to releases alongside the AIBOM': not implemented anywhere — the release artifact path (GET /api/v1/repos/:id/aibom?format=release) embeds per-commit chain_attestation (the chain JSON) but never the VerificationBundle (chain + trust anchors + git diff + timestamp); the bundle is assembled only inside the export endpoint and no release artifact or release-preparation path (POST /api/v1/release/prepare) carries or references it. External compliance verification of such a bundle is performed by external tools outside platform scope, but attachability to releases is an in-platform, implementable requirement with no implementation. Both the commit-binding wiring and the release attachment are missing behavior.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "/api/v1/repos/:id/attestations/:commit_sha/bundle",
            "detail": "The specced export route is registered — the export half of the section has a real endpoint."
          },
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "no attestation found for commit {commit_sha}",
            "detail": "The 404 branch taken whenever find_by_commit misses — which, since no production save path ever sets a real commit_sha on an attestation, is every real commit; the bundle is never exportable for production work."
          },
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "let bundle = serde_json::json!({",
            "detail": "The VerificationBundle (attestation_chain + trust_anchors + git_diff + timestamp) is assembled only inside this export endpoint — the only place the bundle exists at all."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "pub struct ReleaseCommit {",
            "detail": "The release artifact's per-commit entry carries only chain_attestation (chain JSON: attestation_id, chain_depth, chain nodes) — it never includes the VerificationBundle (trust anchors, git diff, timestamp), so 'attached to releases alongside the AIBOM' has no implementation in the release path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "commit_sha: String::new(),",
            "detail": "Derived attestations (like the approval root) carry an empty commit_sha and are never updated with the pushed commit, so the commit index the export depends on is never populated in production."
          }
        ]
      },
      {
        "id": "f3d63f8ba82d318a407da9e3",
        "decision": "gap",
        "requirement_quote": "- Attached to the attestation record",
        "detail": "Every verification does produce a VerificationResult tree (label/valid/message/children) — the push/merge enforcement paths and the GET .../verification endpoint build real trees, and constraint children name the failing constraint. But of the three storage requirements: 'Logged to the audit system' is only satisfied as tracing structured logs (category/event fields), not as AuditEvent records in the audit store (no attestation.* AuditEventType variants exist); 'Attached to the attestation record' is not implemented — verification results are never persisted to the attestation or any store, only returned in the HTTP response; 'Queryable via GET .../verification' is implemented (the endpoint recomputes the tree on demand rather than returning a stored one).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "no attestation found for commit {commit_sha}",
            "detail": "The verification endpoint recomputes the result tree on each request; no code path stores a VerificationResult against the attestation record."
          },
          {
            "file": "crates/gyre-domain/src/audit.rs",
            "quote": "Custom(String),",
            "detail": "The AuditEventType enum ends at supply-chain/custom — there are no attestation.created/verified/constraint_failed/chain_invalid or key_binding.* variants, so provenance verification results cannot be recorded in the audit event store."
          },
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct VerificationResult {",
            "detail": "The recursive result-tree type exists and is produced by every verification path; the gap is purely its persistence/audit recording."
          }
        ]
      },
      {
        "id": "234004cc5b4e4a3d78c95ccc",
        "decision": "gap",
        "requirement_quote": "**Key exchange:** The approval endpoint requires the client to have a valid `KeyBinding` (§2.3).",
        "detail": "The amendment's shape is implemented: POST /api/v1/specs/:path/approve accepts output_constraints and scope, verifies the user's Ed25519 signature over the InputContent against the active KeyBinding's public key (rejecting invalid signatures), and stores the SignedInput as the chain root. When output_constraints/scope are omitted, the SignedInput is created with only strategy-implied constraints, as specced. But the KeyBinding requirement is not enforced: when the approver has no active KeyBinding, or has one but omits user_content_signature, the approval still succeeds and no SignedInput is created (debug-log skip). The spec says the endpoint requires a valid KeyBinding and the client must establish one first; the implementation lets unconstrained, unsigned approvals through, which is exactly the 'authorization created with no cryptographic root' case the chain-mandatory policy (§7.2) then fails to catch.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "no active KeyBinding for approver — skipping SignedInput creation (Phase 1)",
            "detail": "The approval succeeds without a KeyBinding — the SignedInput is silently skipped rather than the request being rejected, so the 'requires the client to have a valid KeyBinding' clause is not enforced."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let signed_input = gyre_common::SignedInput {",
            "detail": "When a binding and user signature are present, a genuine user-signed SignedInput is constructed and stored — the amendment's positive path is real."
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
        "id": "d181208ee20b618ef25389b3",
        "decision": "gap",
        "requirement_quote": "The stack attestation is no longer a separate check — it is one constraint among many, evaluated by the same verification algorithm.",
        "detail": "Partially implemented. The attestation-level half is unified: derive_strategy_constraints emits 'agent.attestation_level >= N' from the repo's real stack policy (KV repo_stack_policies), evaluated by the common CEL pipeline at push and merge. The AIBOM half is done: per-commit chain_attestation replaces the flat stack_attestation field. But the stack-hash half remains a separate check: the CEL constraint set never includes a stack-hash equality from gyre-stack.lock (no 'agent.stack_hash ==' constraint is derived anywhere); stack attestation is still enforced by the separate enforce_stack_policy path with its own Block/Warn modes, min-attestation-level rejection and lockfile-drift detection outside the constraint evaluation algorithm. The two verification paths are therefore not unified into one algorithm as the amendment requires.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "async fn enforce_stack_policy(",
            "detail": "Stack attestation enforcement remains a separate push-time check (min level with Block/Warn modes, gyre-stack.lock drift) outside the CEL constraint pipeline — the unification the amendment mandates is not carried out for the stack-hash constraint."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "strategy: attestation level must be >= {level}",
            "detail": "The attestation-level constraint from repo stack policy IS unified into the CEL constraint set (the partially-implemented half)."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "pub chain_attestation: Option<serde_json::Value>,",
            "detail": "AIBOM carries the full attestation chain per commit, replacing the flat stack_attestation field as specced."
          }
        ]
      },
      {
        "id": "e5da488abf98ee7cd64ea91f",
        "decision": "gap",
        "requirement_quote": "4. The `DerivedInput` and `KeyBinding` are injected into the agent's environment (new).",
        "detail": "Steps 1–3 of the amendment are implemented: the OIDC JWT is minted as before; spawn generates a real per-agent Ed25519 keypair and stores a workload KeyBinding; and create_derived_input_for_agent builds a spawner-signed DerivedInput and persists a chain node (though in the production flow it never fires because find_by_task cannot see the approval-time root — see §2.1/§4.2). Steps 4–5 are not implemented: the DerivedInput and KeyBinding are stored in server-side KV namespaces (agent_derived_inputs, agent_key_bindings, agent_signing_keys) and are never injected into the agent's environment (no env var or protocol delivery of the binding/derived input to the container), and no production path has the agent sign its output attestation at push time — every production construction site of AttestationOutput sets agent_signature: None, so the child's key is generated and stored but never used to sign work output.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "kv_set(\"agent_derived_inputs\", agent_id, di_json)",
            "detail": "The DerivedInput is stored in a server-side KV namespace, not injected into the agent's environment — no env var/protocol delivery of the derived input or key binding to the spawned container exists."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "commit_sha: String::new(),",
            "detail": "The derived attestation (and every other production attestation output) is created with agent_signature absent/None — grep shows no production site sets agent_signature, so step 5 (agent signs its output at push time) has no implementation."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let derived_input = gyre_common::DerivedInput {",
            "detail": "Steps 2–3 are real: a workload key is generated and a spawner-signed DerivedInput is constructed and persisted when a chain is visible."
          }
        ]
      },
      {
        "id": "224e258ab12870575d7fe7bd",
        "decision": "satisfied",
        "requirement_quote": "Tier: **Event** (signed, TTL). Constraint violations are broadcast to the workspace and directed to the author agent.",
        "detail": "MessageKind::ConstraintViolation exists in the common message catalog with Event tier (the same tier family as the other signed event kinds) and an AG-UI event type mapping. The payload schema matches the specced fields (attestation_id, constraint_name, expression, context_snapshot, action, agent_id, repo_id, timestamp) — emit_constraint_violations builds exactly that JSON, broadcasts to Destination::Workspace and directs a copy to Destination::Agent(author), from both the audit-only and enforcing push/merge paths. Human operators receive priority-2 inbox notifications (create_violation_notifications) for workspace Admin/Developer/Owner members, matching 'priority 2 — high but not critical'.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/message.rs",
            "quote": "ConstraintViolation,",
            "detail": "The message kind exists and is classified Event tier in the tier mapping (the signed, TTL event family)."
          },
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "Destination::Agent(Id::new(agent_id))",
            "detail": "Violations are directed to the author agent in addition to the workspace broadcast, as specced."
          },
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "/// Create priority-2 Inbox notifications for constraint violations (§7.5).",
            "detail": "Human-operator inbox notifications at priority 2, with the full expression/message/task context in the notification body."
          }
        ]
      },
      {
        "id": "59873e07f74dcd542130bf41",
        "decision": "gap",
        "requirement_quote": "- The approver can add explicit constraints via a CEL expression editor with autocomplete for available context fields.",
        "detail": "The spec-approval UI gains constraint editing via web/src/lib/ConstraintEditor.svelte, wired into the approval flow (DetailPanel 'Approve with Constraints'): strategy-implied constraints are fetched read-only from the server (GET /api/v1/constraints/strategy), explicit constraints are entered as name+CEL-expression rows, scope constraints are entered as allowed/forbidden glob lists, and a Dry Run button calls POST /api/v1/constraints/dry-run which compiles/validates the expressions and evaluates them against the current repo state, reporting per-constraint pass/fail. The Explorer side is implemented by ProvenanceChain.svelte (used in DetailPanel), rendering the commit's attestation chain as a directed graph from GET /api/v1/repos/:id/attestations/:commit_sha/chain with per-node signer identity, constraint count, chain depth, verification status and failed-constraint highlighting. However two features the amendment explicitly names are missing: the CEL editor is a plain text input with an example placeholder — no autocomplete for available context fields — and scope constraints are comma-separated glob text inputs rather than a file-tree picker. These are specced features of the approval UI named in the amendment, not presentation-level details; per the goal's rule 4 they must be implemented or the spec amended. Previously classified satisfied by calling the deviations presentation-level; per review that classification is retracted and the section is a gap.",
        "evidence": [
          {
            "file": "web/src/lib/ConstraintEditor.svelte",
            "quote": "placeholder='CEL expression, e.g. output.changed_files.all(f, f.startsWith(\"src/\"))'",
            "detail": "The CEL expression editor is a plain text input with an example placeholder — there is no autocomplete for available context fields as §7.6 requires."
          },
          {
            "file": "web/src/lib/ConstraintEditor.svelte",
            "quote": "Restrict which files the agent may modify (comma-separated glob patterns).",
            "detail": "Scope constraints are entered as comma-separated glob text inputs (allowed/forbidden), not the file-tree picker with glob patterns the amendment specifies."
          },
          {
            "file": "web/src/lib/ConstraintEditor.svelte",
            "quote": "async function dryRun() {",
            "detail": "The implemented half: the dry-run button evaluates the assembled constraint set (explicit + scope) against the repo state via the server endpoint and surfaces per-constraint issues."
          },
          {
            "file": "web/src/components/ProvenanceChain.svelte",
            "quote": "/api/v1/repos/${repoId}/attestations/${commitSha}/chain",
            "detail": "Provenance chain visualization fetches the directed-graph chain data for the commit and renders nodes with signer identity, constraint count and verification status."
          }
        ]
      },
      {
        "id": "c8e81723fe471db62eb85bbc",
        "decision": "gap",
        "requirement_quote": "All events include the full `VerificationResult` tree for forensic analysis.",
        "detail": "All seven event names are emitted at the right places, but as structured tracing logs only: attestation.created (approval, spawn derivation, gate attestation), attestation.verified / attestation.constraint_failed (constraint result logging), attestation.chain_invalid (chain and context-binding failures), key_binding.created / key_binding.revoked (key-binding endpoints) — each with category ('Provenance'/'Identity') and event fields. None of them are recorded through the audit system the amendment references: the AuditEvent/AuditEventType machinery (state.audit.record, SIEM forwarding, /api/v1/audit endpoints) has no attestation.* or key_binding.* variants, so these events never reach the audit store or SIEM stream. And no event includes the full VerificationResult tree — the log lines carry scalar fields (constraint counts, messages), and the spec's requirement that every event carry the tree for forensics is unmet.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "event = \"attestation.verified\",",
            "detail": "The attestation.verified event is emitted as a tracing log with scalar fields only — no VerificationResult tree is attached to the event."
          },
          {
            "file": "crates/gyre-domain/src/audit.rs",
            "quote": "Custom(String),",
            "detail": "The audit event type enum has no attestation.* or key_binding.* variants, so provenance/identity events cannot be recorded into the audit store that observability.md defines (total auditing, SIEM forwarding) — they exist only in tracing output."
          }
        ]
      },
      {
        "id": "67eacb60171e6cedff82c482",
        "decision": "gap",
        "requirement_quote": "- Produce `SignedInput` on every spec approval.",
        "detail": "Four of the five Phase 1 bullets are done: the KeyBinding exchange is implemented (POST /api/v1/auth/key-binding with proof-of-possession, countersignature, storage, TTL cap); attestation chains are stored alongside the existing AttestationBundle (chain store + dual-write at merge); verification runs in audit-only mode (verify_attestation_audit_only, evaluate_push_constraints) that logs and never rejects; and the push flow is otherwise unchanged. The remaining bullet is not met as stated: SignedInput is not produced on every spec approval — production is conditional on the approver having an active KeyBinding AND supplying user_content_signature; otherwise the approval succeeds with no SignedInput (silent skip, see §7.1). Since Phase 1 is explicitly non-enforcing the machinery matches the phase's spirit, but 'every spec approval' is a literal requirement the code does not satisfy.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "no active KeyBinding for approver — skipping SignedInput creation (Phase 1)",
            "detail": "Approvals without an active KeyBinding (or without user_content_signature) produce no SignedInput — the 'every spec approval' bullet is conditional in production."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "pub(crate) fn verify_attestation_audit_only(",
            "detail": "Audit-only verification is genuinely non-enforcing (logs VerificationResult trees; rejection lives in the separate Phase 3 path), matching the phase's non-enforcing bullets."
          }
        ]
      },
      {
        "id": "ba395b2bbd4c1ca5a831b3b8",
        "decision": "satisfied",
        "requirement_quote": "- Derive and log strategy-implied constraints at push and merge time.",
        "detail": "Phase 2 is fully implemented: evaluate_push_constraints and evaluate_merge_constraints derive strategy-implied constraints from the signed InputContent plus live workspace/stack-policy state, evaluate them against the real diff, and log results without rejecting; violations surface in the message bus (ConstraintViolation events, workspace + author agent) and as priority-2 human inbox notifications; humans can add explicit constraints at approval time (approve endpoint + ConstraintEditor UI with read-only strategy display); and the paths remain audit-only by design (enforcement lives in the parallel enforce_* functions).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "no attestation chain found for task (Phase 3 enforcement, allowing)",
            "detail": "The audit-only evaluate_* mirrors log and emit without rejecting (the enforcement-mode:ok annotations mark the intentional non-blocking behavior), fulfilling the phase's non-enforcing contract."
          },
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "Destination::Agent(Id::new(agent_id))",
            "detail": "Violations surface on the message bus and in human notifications during the audit-only phase, as the bullet requires."
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
      },
      {
        "id": "1f7628ebe440a75c8f32588b",
        "decision": "gap",
        "requirement_quote": "- Gate agents produce `GateConstraint` records.",
        "detail": "Mixed: chain verification at push and merge time exists (verify_chain + constraint evaluation in both enforce paths); the AIBOM is extended with full chain attestations per commit; and orchestrator DerivedInput production code exists — but it never fires in the production flow (find_by_task cannot see the unlinked root, §2.1/§4.2). 'Gate agents produce GateConstraint records' is not implemented at all: the only production GateAttestation constructor sets constraint: None and no review/validation agent path creates a GateConstraint (all other constructions are tests). 'Legacy AttestationBundle deprecated' is not done either — the legacy bundle is still generated at every merge and written as the git-note fallback whenever no chain is found (which is always, in production).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "constraint: None, // Gate constraints are attached by review/validation agents",
            "detail": "No production gate emits a GateConstraint — the phase's explicit bullet is unimplemented (only the consumption side exists)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "// During the dual-write period, we write EITHER the chain attestation",
            "detail": "The legacy AttestationBundle is still produced at merge and used as the git-note fallback — not deprecated, though this is consistent with the chain never being findable by commit in production."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "no attestation chain for task — skipping DerivedInput creation",
            "detail": "Orchestrator → agent DerivedInput production never fires in the production flow because the parent chain is unreachable by task lookup."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "80fa5d551228a894b5a85132",
        "decision": "gap",
        "requirement_quote": "The platform cannot add, remove, or modify trust anchors without a human admin action — this is enforced by an immutable Deny ABAC policy (same pattern as `builtin:require-human-spec-approval`).",
        "detail": "TrustAnchor types, a tenant-scoped CRUD repository (SQLite + mem), and /api/v1/tenants/:id/trust-anchors endpoints exist, and modification is gated on the Admin role plus a tenant match. However the spec's enforcement mechanism — an immutable Deny ABAC policy that (like builtin:require-human-spec-approval) denies non-human subjects — is not implemented: protection is an RBAC role check in the handler, so any principal holding the Admin role (including an agent principal) can add, remove or rotate anchors, and no builtin policy guards the trust-anchor resource. Additionally, anchor-level `constraints` (§3.2 reference in the TrustAnchor struct) are stored and returned by the CRUD API but never evaluated: derive_strategy_constraints/collect_all_constraints take only InputContent, workspace trust level and gate constraints — no code path loads a TrustAnchor's constraints into the evaluated constraint set. 'A workspace inherits its tenant's anchors' is only exercised by the §6.3 bundle export (list_by_tenant via the workspace's tenant); verification never resolves key_binding.trust_anchor_id against the anchor registry.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/trust_anchors.rs",
            "quote": "\"trust anchor management requires Admin role\".to_string(),",
            "detail": "All five trust-anchor endpoints gate on require_admin (UserRole::Admin) plus a tenant_id equality check. This is handler-level RBAC, not the specced immutable Deny ABAC policy; grep of m34_builtin_policies and the policy engine shows no builtin policy for trust-anchor resources."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "pub fn collect_all_constraints(",
            "detail": "The evaluated constraint set is the union of explicit, strategy-implied and gate constraints only. TrustAnchor.constraints are accepted and stored by the CRUD API but no production call path adds them to the evaluated set — the anchor-level constraints of §1.1/§3.2 are dead data."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/trust_anchor.rs",
            "quote": "impl TrustAnchorRepository for SqliteStorage {",
            "detail": "Real tenant-scoped SQLite persistence for trust anchors exists (create/find/list/update/delete), so the registry itself is real storage, not a stub — the gap is the enforcement mechanism and the unevaluated anchor constraints."
          }
        ]
      },
      {
        "id": "0a001bfe1c6ccfa6bd7e4cb3",
        "decision": "gap",
        "requirement_quote": "the approval now produces a Signed Input that travels with every task derived from that spec.",
        "detail": "Spec approval does produce a real SignedInput when the approver has an active KeyBinding and supplies user_content_signature (specs.rs): the InputContent is built (spec registry SHA, workspace/repo from the ledger, persona constraints, meta_spec_set_sha, scope), the user's Ed25519 signature over SHA256(content) is verified against the KeyBinding public key, and the root attestation is saved to the chain store. But it does NOT travel with tasks: the root attestation is stored with metadata.task_id = \"\" and commit_sha = \"\", and no production code ever re-saves or links it to a task. Every downstream consumer looks chains up by task_id (spawn's create_derived_input_for_agent, push/merge enforcement in constraint_check.rs, gate_executor) or by commit_sha (merge ABAC gate, git-note attachment, verification endpoints), so in the production flow the root is unreachable: derived inputs are never created ('no attestation chain for task'), push enforcement finds no chain and allows, and the verification/bundle/chain endpoints 404. The wiring step that associates the approval-time SignedInput with tasks derived from the spec is missing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "task_id: String::new(), // No task yet at approval time.",
            "detail": "The root SignedInput attestation is persisted with an empty task_id. The only production save sites for chain attestations are this root (empty task_id), spawn's derived nodes (real task_id, but creation is gated on find_by_task which never returns the root), and gate_executor's leaf update — nothing links the root to a task."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "no attestation chain for task — skipping DerivedInput creation",
            "detail": "create_derived_input_for_agent looks up the parent chain exclusively via chain_attestations.find_by_task(task_id). Because the root is stored with task_id \"\", this returns empty for every real task and derived-input creation is skipped in the production flow."
          },
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "no attestation chain found for task (Phase 3 enforcement, allowing)",
            "detail": "Push-time enforcement also keys on find_by_task; with the root unlinked, no chain is ever found, so the enforcement path takes the allow branch for every production push. Tests pass only because they insert attestations whose task_id matches directly (make_attestation in this file's test module)."
          }
        ]
      },
      {
        "id": "dcf557ae5dbc627aab292307",
        "decision": "gap",
        "requirement_quote": "The `platform_countersign` proves the binding was registered at a specific time with a valid IdP session. It does NOT make the platform a trust root — verification checks the user's signature against the bound public key and the IdP's trust anchor, not the platform's countersignature.",
        "detail": "The KeyBinding exchange exists and much of it is real: POST /api/v1/auth/key-binding requires an authenticated session, rejects non-32-byte keys, cryptographically verifies an Ed25519 proof-of-possession, countersigns with the real server key, caps TTL at 24h, persists via the tenant-scoped KeyBindingRepository, and expiry/revocation are enforced downstream. However the section's core anti-forgery property is not implemented. Spec lifecycle step 3 requires the CLIENT to construct the KeyBinding document and sign it with the private key, and the section explicitly denies that the countersignature makes the platform a trust root. At the base, user_signature is verified only over the raw public key bytes as proof-of-possession; user_identity, issuer, trust_anchor_id, issued_at and expires_at are server-assigned (derived from the auth context and server clock) and are covered ONLY by the platform countersign. Consequence: a compromised server — the exact adversary this spec models (§1) — can rewrite a stored binding's identity fields and re-countersign with the platform key it holds, so identity attribution of an authorization root rests entirely on the platform, precisely the authority the section denies; offline verifiers (§6.3) have no user-signed identity claims to check. This was previously classified satisfied with the deviation dismissed as preserving the binding's security properties; per review that dismissal is retracted and the section is reclassified as a gap. Correcting it requires the client to construct and sign the full binding document (public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at) and binding verification to check the user's document signature and resolve the trust anchor, treating the countersignature strictly as a timestamp witness.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/key_binding.rs",
            "quote": "The user signs the public key bytes as a proof-of-possession.",
            "detail": "The only user signature verified by the exchange is over the raw public key bytes; none of the binding document's identity fields (user_identity, issuer, trust_anchor_id, validity) are covered by a user signature, so spec lifecycle step 3 (client constructs and signs the KeyBinding document) is not implemented."
          },
          {
            "file": "crates/gyre-server/src/api/key_binding.rs",
            "quote": "let user_identity = derive_user_identity(&auth);",
            "detail": "user_identity (and issuer, from jwt_config/base_url, and the issued_at/expires_at timestamps) are server-assigned during the exchange rather than client-constructed claims signed by the user's key."
          },
          {
            "file": "crates/gyre-server/src/api/key_binding.rs",
            "quote": "let platform_countersign = state.agent_signing_key.sign_bytes(&binding_bytes);",
            "detail": "The only signature covering the identity fields of the countersigned binding JSON is the platform countersignature made with the server-held key — a compromised server can rewrite those fields and re-countersign, making the platform the de facto trust root for binding identity, which the section explicitly forbids."
          }
        ]
      },
      {
        "id": "3a1de6328a8b4ff8639ece89",
        "decision": "gap",
        "requirement_quote": "A gate agent that reviews code can attach constraints that subsequent gates or the merge processor must verify:",
        "detail": "The GateConstraint type, its inclusion in GateAttestation.constraint, and its consumption are real: gate constraints are collected from chain nodes and evaluated at push/merge time and in the verification endpoint, additively with the other sources. But no production gate ever produces one. The only production site that builds GateAttestation records (gate_executor's attach path) sets constraint: None with the comment that gate constraints are 'attached by review/validation agents', and no review/validation agent path constructs a GateConstraint — grep shows the only non-type constructions of GateConstraint are in test modules (constraint_evaluator, merge_processor tests). The signed-by-gate-agent emission half of Source 3 is therefore unimplemented; only the evaluation half exists.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "constraint: None, // Gate constraints are attached by review/validation agents",
            "detail": "The sole production GateAttestation constructor leaves the gate constraint empty; no other production code path creates a GateConstraint (all other constructions are in #[cfg(test)] modules), so gates can never tighten the constraint set as §3.2 Source 3 requires."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "if !chain_found && denial.is_none() {",
            "detail": "Merge-time consumption of gate constraints exists (accumulate_chain_constraints collects GateAttestation.constraint into the evaluated set), confirming the gap is on the production emission side only."
          }
        ]
      },
      {
        "id": "5bf9cf12933a9a01581e530f",
        "decision": "gap",
        "requirement_quote": "The new approval's authorization is derived from the prior approval's `SignedInput`, carrying forward the prior's constraints.",
        "detail": "Of the three creation triggers: (1) task decomposition is implemented in code (create_derived_input_for_agent signs a DerivedInput from the task's chain leaf with the spawner's key) but never fires in the production flow because find_by_task never returns the approval-time root (task_id=\"\" — see §2.1 gap); (3) re-spawn shares the same spawn path and would derive from the same parent chain when a chain exists. (2) spec update is not implemented at all: when a spec is modified and re-approved, specs.rs constructs a brand-new independent SignedInput from the new content — nothing loads the prior approval's SignedInput, derives from it, or carries forward its constraints; a re-approver who omits a previously-signed constraint silently drops it (the §9 'spec revision silently drops security constraint' prevention therefore does not hold).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let signed_input = gyre_common::SignedInput {",
            "detail": "The approval path always constructs a fresh root SignedInput; there is no code that loads a prior approval's SignedInput, creates a DerivedInput from it, or unions the prior's output_constraints into the new authorization."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "no attestation chain for task — skipping DerivedInput creation",
            "detail": "The decomposition trigger (trigger 1) is gated on find_by_task, which returns nothing for the unlinked root, so derived inputs are never created in the production flow despite the code existing."
          }
        ]
      },
      {
        "id": "74593d79542d482cc5567984",
        "decision": "gap",
        "requirement_quote": "// 4. Evaluate preconditions against parent's verified state",
        "detail": "The implemented chain verification covers part of the algorithm: per-node Ed25519 signature verification over the signed/derivation content hash, key-binding expiry, valid_until, depth limit, presence of a root SignedInput, and structural parent linkage (non-empty parent_ref plus a lower-depth node in the chain). Missing steps: (2) verify_key_binding against the trust anchor — the key binding's trust_anchor_id/issuer are never resolved against the TrustAnchor registry or JWKS, and the platform countersignature is never verified; (4) preconditions are never evaluated against the parent's state (spawn creates them empty and no verifier executes them); (5) the update expression is never evaluated; (6) derived content is never recomputed and compared to claimed content. Parent linkage is only structural: verify_chain checks that some lower-depth node exists, explicitly not matching parent_ref content hashes to parent IDs — and the production parent_ref (SHA-256 of the parent attestation JSON) does not equal the parent's stored id (uuid for derived nodes, SHA-256-of-SignedInput for roots), so load_chain's id-based walk cannot traverse production-created links at all.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let linkage_valid = has_parent_ref && has_parent_at_lower_depth;",
            "detail": "Linkage verification is structural only — the adjacent comment states 'we verify structural consistency (non-empty, depth ordering) rather than trying to match content hashes to IDs'; steps 4–6 of the specced algorithm (preconditions, update expression, derived-content hash match) have no implementation anywhere in the verifier."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "UnparsedPublicKey::new(&signature::ED25519, &signed.key_binding.public_key);",
            "detail": "Signature verification uses the key embedded in the attestation; step 2 (verify_key_binding — resolving trust_anchor_id against the trust-anchor registry / JWKS, checking the countersignature) is absent from all inspected verification call paths."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "preconditions: vec![],",
            "detail": "Production derived inputs carry no preconditions and no verifier evaluates the preconditions or update fields, so those CEL programs are dead spec surface."
          }
        ]
      },
      {
        "id": "7910f63c3be5b0641f51c47b",
        "decision": "gap",
        "requirement_quote": "The `spawned_by` field is critical — it links the orchestrator's key binding back to the human who approved the original spec, even though the orchestrator (not the human) is the immediate signer.",
        "detail": "Orchestrators do sign derived inputs with a workload-bound key (spawn stores a per-agent Ed25519 keypair and a KeyBinding with user_identity 'agent:<id>', issuer = platform base URL, trust_anchor_id 'gyre-oidc', expiry tied to the agent JWT TTL). But the KeyBinding type has no workload claims: no task_id, spawned_by, stack_hash or persona fields exist on it (the spec's OrchestratorKeyBinding extension), so the derivation signature does not cryptographically bind the orchestrator's task, spawning human, stack or persona. Those attributes live only in the unverified AttestationMetadata/AgentContext. The spawner's KeyBinding is also frequently synthesized at spawn time from the spawner's public key with empty signature/countersignature fields when no stored binding exists.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/key_binding.rs",
            "quote": "pub struct KeyBinding {",
            "detail": "The KeyBinding struct carries only public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at, user_signature, platform_countersign — none of the specced workload claims (task_id, spawned_by, stack_hash, persona) exist on the type."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "user_signature: vec![],       // workload-bound — no user signature",
            "detail": "The child agent's workload KeyBinding (and the synthesized root-agent fallback binding) are constructed with empty signature and countersignature fields — the binding attests nothing beyond key possession."
          }
        ]
      },
      {
        "id": "827a7cc2ad6eaf0f8e262900",
        "decision": "gap",
        "requirement_quote": "The strategy-implied constraint `chain.depth <= N` is derived from workspace configuration.",
        "detail": "The hard safety-net limit is implemented: verify_chain clamps max_depth to 10, per-node verification rejects depth > 10, and spawn refuses to create derived inputs at depth >= 10. But no workspace-configuration-derived depth limit exists: the verification endpoint hardcodes 'let max_depth = 10u32; // Will be configurable per workspace in future', no strategy-implied chain.depth CEL constraint is derived anywhere in derive_strategy_constraints, and workspace trust level (Supervised vs Autonomous) does not influence chain depth.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "let effective_max = max_depth.min(10);",
            "detail": "The hard limit of 10 is enforced regardless of the configurable parameter (the safety net of the third bullet)."
          },
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "let max_depth = 10u32; // Will be configurable per workspace in future",
            "detail": "The workspace-configurable depth limit and the trust-level-derived chain.depth constraint are explicitly deferred — no production code derives a per-workspace N or a chain.depth strategy constraint."
          }
        ]
      },
      {
        "id": "7298cfa0831c074e5e21aa9e",
        "decision": "gap",
        "requirement_quote": "  id:               string          -- content-addressable hash of the attestation",
        "detail": "The Attestation record structure is real and produced in production: input (tagged Signed/Derived), AttestationOutput (content_hash/commit_sha/agent_signature/gate_results), AttestationMetadata (created_at/workspace_id/repo_id/task_id/agent_id/chain_depth) all exist as production types persisted through the chain store, and GateAttestations are genuinely produced and signed at gate execution (signed over canonical signable_bytes with the platform key, appended to the chain leaf, re-persisted, written as a git note). agent_signature is optional ('if capable') and always None in production — accounted under §7.4. The gap is the id field's own requirement: 'content-addressable hash of the attestation'. Approval-time roots satisfy it (SHA-256 over the serialized SignedInput), but spawn-created derived nodes set id to a random UUID (uuid::Uuid::new_v4()) — a random UUID is not content-addressable: identical derivations get different ids, and no verifier can recompute or validate a derived node's id offline. This also breaks chain traversal semantics (§4.4): parent_ref (SHA-256 of the parent attestation) does not equal the parent's stored UUID id, so the id-based load_chain walk cannot traverse production-created links (audited under §4.4). The id-field requirement is split out of the otherwise-satisfied structure as this section's gap.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "id: uuid::Uuid::new_v4().to_string(),",
            "detail": "Spawn-created derived attestation nodes use a random UUID as the id — not a content-addressable hash. Identical derivations produce different ids and no offline verifier can recompute the id, contradicting the §5.1 field spec."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let hash = ring::digest::digest(&ring::digest::SHA256, &att_bytes);",
            "detail": "Approval-time roots ARE content-addressed (SHA-256 over the serialized SignedInput), showing the deviation is specific to spawn-created derived nodes, not a uniform id scheme."
          },
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "let mut gate_attestation = GateAttestation {",
            "detail": "The remainder of §5.1's structure is real production code: per-gate signed GateAttestation records are constructed at gate execution and signed via the shared signable_bytes() helper, so the gap is confined to the id field's content-addressing requirement."
          }
        ]
      },
      {
        "id": "acda07a63c3a25e9c32461e5",
        "decision": "gap",
        "requirement_quote": "| `conversation_sha` | `AttestationOutput` extended field (unchanged) |",
        "detail": "The backward-compatibility retention is real: the legacy AttestationBundle/MergeAttestation types and the AttestationRepository port are kept, the bundle carries a Phase-4 deprecation notice, and the merge processor performs the dual-write (chain attestation note when a chain is locatable, legacy bundle note otherwise). But the mapping table that defines the subsumption is not fully realized, and the subsumption statement itself is not implemented. Two rows of the table map `conversation_sha` and `completion_summary` to 'AttestationOutput extended field (unchanged)' — the shipped AttestationOutput has no such extended fields (only content_hash, commit_sha, agent_signature, gate_results); these values are constructed only on the legacy MergeAttestation and never enter any chain attestation, so any consumer that stops producing the legacy bundle loses them. And 'The merge attestation becomes the final attestation in a chain' is unrealized: merge appends no attestation node to the chain — it only attaches the pre-existing chain (located via find_by_commit or the author-agent task fallback) as a git note on the merge commit, while the legacy bundle remains the merge-time record stored in the attestation store. The subsumption is therefore structural only for the rows that are realized (spec_ref/spec_sha at the chain root, per-node signing keys, gate results per node); the conversation_sha/completion_summary rows and the final merge node are missing behavior.",
        "evidence": [
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct AttestationOutput {",
            "detail": "The shipped AttestationOutput contains exactly content_hash, commit_sha, agent_signature and gate_results — there are no conversation_sha or completion_summary 'extended fields', so the two mapping rows that promise them ('AttestationOutput extended field (unchanged)') have no realization in the chain format."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "completion_summary: None,",
            "detail": "conversation_sha and completion_summary are carried only by the legacy MergeAttestation constructed at merge time (crates/gyre-server/src/merge_processor.rs, the crate::attestation::MergeAttestation literal); no chain attestation node ever carries them."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "// During the dual-write period, we write EITHER the chain attestation",
            "detail": "Merge appends no final attestation node to the chain: the chain note path only re-attaches the pre-existing chain (found via find_by_commit or the author agent's task) as a git note on the merge commit; the merge-time record stored in the attestation store is still the legacy bundle, so 'the merge attestation becomes the final attestation in a chain' is not implemented."
          },
          {
            "file": "crates/gyre-domain/src/attestation.rs",
            "quote": "**Deprecated (Phase 4):** This legacy format is subsumed by the chain attestation",
            "detail": "The deprecation notice directing consumers to the chain format is retained — the retention half of the section is genuinely implemented; the gap is the unrealized mapping rows and the missing final merge node."
          }
        ]
      },
      {
        "id": "c49cf3ba98e72736cfcb8bee",
        "decision": "gap",
        "requirement_quote": "2. **Git notes** (`refs/notes/attestations`) — the chain attestation is attached to the relevant commit",
        "detail": "All three storage locations have real implementations, but the git-notes and commit-indexed paths are unreachable for real commits: (1) the attestation store is real on SQLite and mem (tenant isolation, task/commit/repo indexes, parent_ref chain walking), but the PgStorage adapter is a bail! stub (see §5.4), so on a PostgreSQL deployment location 1 fails entirely; (2) attach_chain_attestation_note writes the chain JSON to refs/notes/attestations and read helpers parse it back — but it only runs when chain_attestations.find_by_commit(sha) or the task fallback finds an attestation, and no production code ever records a real commit_sha on an attestation (approval stores commit_sha: \"\" 'No commit yet', spawn stores commit_sha: String::new(), gate_executor preserves the leaf's empty value), so no chain is ever attached to any real commit and the legacy-bundle fallback fires instead; (3) AIBOM aggregation includes chain_attestation per commit, reading the same never-matching store. The DB location is genuinely implemented; the commit-binding wiring that locations 2 and 3 depend on is missing.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/attestation.rs",
            "quote": "pub const CHAIN_ATTESTATION_NOTES_REF: &str = \"refs/notes/attestations\";",
            "detail": "Real git-notes writer/reader for chain attestations under the specced ref exists (attach/read helpers invoke git notes)."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "commit_sha: String::new(), // No commit yet at approval time.",
            "detail": "The approval-time root is stored with an empty commit_sha and nothing later updates it when the work is committed."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "commit_sha: String::new(),",
            "detail": "Derived attestations are also created with empty commit_sha; with no production save path ever setting a real commit SHA, find_by_commit (merge-time note attachment, merge ABAC gate, verification/bundle/chain endpoints) never matches a real commit."
          }
        ]
      },
      {
        "id": "1a1c2218815dfe88c38a9789",
        "decision": "gap",
        "requirement_quote": "pub trait ChainAttestationRepository: Send + Sync {",
        "detail": "The port trait exists verbatim with all six specced methods (save, find_by_id, load_chain walking parent_ref, find_by_task, find_by_commit, find_by_repo with time range). Two of the three adapters are real: SQLite (Diesel, tenant-scoped, upsert, parent-walk with loop guard) and the in-memory adapter wired into AppState. The third is not: the PgStorage implementation is a self-described stub in which every method of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository calls anyhow::bail!. The store! wiring in gyre-server/src/lib.rs selects PgStorage whenever GYRE_DATABASE_URL is a postgres:// URL — a documented production backend (docs/server-config.md) — so on a PostgreSQL deployment every provenance store operation fails. Combined with the fail-open error handling in the enforcement path ('Query failures are non-blocking', constraint_check.rs), provenance silently degrades to nothing on PG, and once boundary enforcement makes provenance mandatory this either bricks PG deployments or — if the fail-open paths survive — exempts them from the mandatory policy. The prior audit's claim that Postgres is a 'real adapter' was factually false and is corrected here; per the goal's rule 1 a bail!() stub is an open gap.",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/chain_attestation.rs",
            "quote": "pub trait ChainAttestationRepository: Send + Sync {",
            "detail": "The port trait with the exact §5.4 method set."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/chain_attestation.rs",
            "quote": "impl ChainAttestationRepository for SqliteStorage {",
            "detail": "Full SQLite implementation: content-addressed upsert, tenant isolation on every query, parent_ref chain walk with a 100-node loop guard, task/commit/repo/time-range indexes — real storage, not a stub."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
            "quote": "Stub PgStorage implementations for authorization provenance port traits.",
            "detail": "The file header self-describes as a stub with 'Full implementation deferred to future milestone' — this contradicts the prior audit's claim of a real Postgres adapter."
          },
          {
            "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
            "quote": "anyhow::bail!(\"ChainAttestationRepository not implemented for PgStorage\")",
            "detail": "Every method of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository bails in this file (16 bail! calls total); no PgStorage provenance operation can succeed."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "chain_attestations: store!(",
            "detail": "The chain attestation store is wired through the store! macro, whose first branch is 'if let Some(ref d) = pg_db' — PgStorage is selected whenever a postgres:// GYRE_DATABASE_URL is configured, so a documented production backend routes all provenance storage to the bail! stub."
          }
        ]
      },
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
        "id": "bb664ae221456d242e8ae921",
        "decision": "gap",
        "requirement_quote": "- Attached to releases alongside the AIBOM",
        "detail": "The export route is implemented with the specced path and assembles a real VerificationBundle — attestation chain (load_chain), the tenant's trust anchors (workspace → tenant → list_by_tenant), the actual git diff of the commit (git diff commit^..commit), and the assembly timestamp. Two of the section's requirements are not met. (1) Reachability: the endpoint is keyed by find_by_commit, and no production path ever records a real commit SHA on an attestation (approval and spawn both store empty commit_sha and nothing updates them), so for every real commit the endpoint returns 404 'no attestation found for commit' — the export machinery is unreachable for production commits. (2) 'Attached to releases alongside the AIBOM': not implemented anywhere — the release artifact path (GET /api/v1/repos/:id/aibom?format=release) embeds per-commit chain_attestation (the chain JSON) but never the VerificationBundle (chain + trust anchors + git diff + timestamp); the bundle is assembled only inside the export endpoint and no release artifact or release-preparation path (POST /api/v1/release/prepare) carries or references it. External compliance verification of such a bundle is performed by external tools outside platform scope, but attachability to releases is an in-platform, implementable requirement with no implementation. Both the commit-binding wiring and the release attachment are missing behavior.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "/api/v1/repos/:id/attestations/:commit_sha/bundle",
            "detail": "The specced export route is registered — the export half of the section has a real endpoint."
          },
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "no attestation found for commit {commit_sha}",
            "detail": "The 404 branch taken whenever find_by_commit misses — which, since no production save path ever sets a real commit_sha on an attestation, is every real commit; the bundle is never exportable for production work."
          },
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "let bundle = serde_json::json!({",
            "detail": "The VerificationBundle (attestation_chain + trust_anchors + git_diff + timestamp) is assembled only inside this export endpoint — the only place the bundle exists at all."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "pub struct ReleaseCommit {",
            "detail": "The release artifact's per-commit entry carries only chain_attestation (chain JSON: attestation_id, chain_depth, chain nodes) — it never includes the VerificationBundle (trust anchors, git diff, timestamp), so 'attached to releases alongside the AIBOM' has no implementation in the release path."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "commit_sha: String::new(),",
            "detail": "Derived attestations (like the approval root) carry an empty commit_sha and are never updated with the pushed commit, so the commit index the export depends on is never populated in production."
          }
        ]
      },
      {
        "id": "f3d63f8ba82d318a407da9e3",
        "decision": "gap",
        "requirement_quote": "- Attached to the attestation record",
        "detail": "Every verification does produce a VerificationResult tree (label/valid/message/children) — the push/merge enforcement paths and the GET .../verification endpoint build real trees, and constraint children name the failing constraint. But of the three storage requirements: 'Logged to the audit system' is only satisfied as tracing structured logs (category/event fields), not as AuditEvent records in the audit store (no attestation.* AuditEventType variants exist); 'Attached to the attestation record' is not implemented — verification results are never persisted to the attestation or any store, only returned in the HTTP response; 'Queryable via GET .../verification' is implemented (the endpoint recomputes the tree on demand rather than returning a stored one).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/provenance.rs",
            "quote": "no attestation found for commit {commit_sha}",
            "detail": "The verification endpoint recomputes the result tree on each request; no code path stores a VerificationResult against the attestation record."
          },
          {
            "file": "crates/gyre-domain/src/audit.rs",
            "quote": "Custom(String),",
            "detail": "The AuditEventType enum ends at supply-chain/custom — there are no attestation.created/verified/constraint_failed/chain_invalid or key_binding.* variants, so provenance verification results cannot be recorded in the audit event store."
          },
          {
            "file": "crates/gyre-common/src/attestation.rs",
            "quote": "pub struct VerificationResult {",
            "detail": "The recursive result-tree type exists and is produced by every verification path; the gap is purely its persistence/audit recording."
          }
        ]
      },
      {
        "id": "234004cc5b4e4a3d78c95ccc",
        "decision": "gap",
        "requirement_quote": "**Key exchange:** The approval endpoint requires the client to have a valid `KeyBinding` (§2.3).",
        "detail": "The amendment's shape is implemented: POST /api/v1/specs/:path/approve accepts output_constraints and scope, verifies the user's Ed25519 signature over the InputContent against the active KeyBinding's public key (rejecting invalid signatures), and stores the SignedInput as the chain root. When output_constraints/scope are omitted, the SignedInput is created with only strategy-implied constraints, as specced. But the KeyBinding requirement is not enforced: when the approver has no active KeyBinding, or has one but omits user_content_signature, the approval still succeeds and no SignedInput is created (debug-log skip). The spec says the endpoint requires a valid KeyBinding and the client must establish one first; the implementation lets unconstrained, unsigned approvals through, which is exactly the 'authorization created with no cryptographic root' case the chain-mandatory policy (§7.2) then fails to catch.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "no active KeyBinding for approver — skipping SignedInput creation (Phase 1)",
            "detail": "The approval succeeds without a KeyBinding — the SignedInput is silently skipped rather than the request being rejected, so the 'requires the client to have a valid KeyBinding' clause is not enforced."
          },
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "let signed_input = gyre_common::SignedInput {",
            "detail": "When a binding and user signature are present, a genuine user-signed SignedInput is constructed and stored — the amendment's positive path is real."
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
        "id": "d181208ee20b618ef25389b3",
        "decision": "gap",
        "requirement_quote": "The stack attestation is no longer a separate check — it is one constraint among many, evaluated by the same verification algorithm.",
        "detail": "Partially implemented. The attestation-level half is unified: derive_strategy_constraints emits 'agent.attestation_level >= N' from the repo's real stack policy (KV repo_stack_policies), evaluated by the common CEL pipeline at push and merge. The AIBOM half is done: per-commit chain_attestation replaces the flat stack_attestation field. But the stack-hash half remains a separate check: the CEL constraint set never includes a stack-hash equality from gyre-stack.lock (no 'agent.stack_hash ==' constraint is derived anywhere); stack attestation is still enforced by the separate enforce_stack_policy path with its own Block/Warn modes, min-attestation-level rejection and lockfile-drift detection outside the constraint evaluation algorithm. The two verification paths are therefore not unified into one algorithm as the amendment requires.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "async fn enforce_stack_policy(",
            "detail": "Stack attestation enforcement remains a separate push-time check (min level with Block/Warn modes, gyre-stack.lock drift) outside the CEL constraint pipeline — the unification the amendment mandates is not carried out for the stack-hash constraint."
          },
          {
            "file": "crates/gyre-domain/src/constraint_evaluator.rs",
            "quote": "strategy: attestation level must be >= {level}",
            "detail": "The attestation-level constraint from repo stack policy IS unified into the CEL constraint set (the partially-implemented half)."
          },
          {
            "file": "crates/gyre-server/src/api/aibom.rs",
            "quote": "pub chain_attestation: Option<serde_json::Value>,",
            "detail": "AIBOM carries the full attestation chain per commit, replacing the flat stack_attestation field as specced."
          }
        ]
      },
      {
        "id": "e5da488abf98ee7cd64ea91f",
        "decision": "gap",
        "requirement_quote": "4. The `DerivedInput` and `KeyBinding` are injected into the agent's environment (new).",
        "detail": "Steps 1–3 of the amendment are implemented: the OIDC JWT is minted as before; spawn generates a real per-agent Ed25519 keypair and stores a workload KeyBinding; and create_derived_input_for_agent builds a spawner-signed DerivedInput and persists a chain node (though in the production flow it never fires because find_by_task cannot see the approval-time root — see §2.1/§4.2). Steps 4–5 are not implemented: the DerivedInput and KeyBinding are stored in server-side KV namespaces (agent_derived_inputs, agent_key_bindings, agent_signing_keys) and are never injected into the agent's environment (no env var or protocol delivery of the binding/derived input to the container), and no production path has the agent sign its output attestation at push time — every production construction site of AttestationOutput sets agent_signature: None, so the child's key is generated and stored but never used to sign work output.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "kv_set(\"agent_derived_inputs\", agent_id, di_json)",
            "detail": "The DerivedInput is stored in a server-side KV namespace, not injected into the agent's environment — no env var/protocol delivery of the derived input or key binding to the spawned container exists."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "commit_sha: String::new(),",
            "detail": "The derived attestation (and every other production attestation output) is created with agent_signature absent/None — grep shows no production site sets agent_signature, so step 5 (agent signs its output at push time) has no implementation."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "let derived_input = gyre_common::DerivedInput {",
            "detail": "Steps 2–3 are real: a workload key is generated and a spawner-signed DerivedInput is constructed and persisted when a chain is visible."
          }
        ]
      },
      {
        "id": "59873e07f74dcd542130bf41",
        "decision": "gap",
        "requirement_quote": "- The approver can add explicit constraints via a CEL expression editor with autocomplete for available context fields.",
        "detail": "The spec-approval UI gains constraint editing via web/src/lib/ConstraintEditor.svelte, wired into the approval flow (DetailPanel 'Approve with Constraints'): strategy-implied constraints are fetched read-only from the server (GET /api/v1/constraints/strategy), explicit constraints are entered as name+CEL-expression rows, scope constraints are entered as allowed/forbidden glob lists, and a Dry Run button calls POST /api/v1/constraints/dry-run which compiles/validates the expressions and evaluates them against the current repo state, reporting per-constraint pass/fail. The Explorer side is implemented by ProvenanceChain.svelte (used in DetailPanel), rendering the commit's attestation chain as a directed graph from GET /api/v1/repos/:id/attestations/:commit_sha/chain with per-node signer identity, constraint count, chain depth, verification status and failed-constraint highlighting. However two features the amendment explicitly names are missing: the CEL editor is a plain text input with an example placeholder — no autocomplete for available context fields — and scope constraints are comma-separated glob text inputs rather than a file-tree picker. These are specced features of the approval UI named in the amendment, not presentation-level details; per the goal's rule 4 they must be implemented or the spec amended. Previously classified satisfied by calling the deviations presentation-level; per review that classification is retracted and the section is a gap.",
        "evidence": [
          {
            "file": "web/src/lib/ConstraintEditor.svelte",
            "quote": "placeholder='CEL expression, e.g. output.changed_files.all(f, f.startsWith(\"src/\"))'",
            "detail": "The CEL expression editor is a plain text input with an example placeholder — there is no autocomplete for available context fields as §7.6 requires."
          },
          {
            "file": "web/src/lib/ConstraintEditor.svelte",
            "quote": "Restrict which files the agent may modify (comma-separated glob patterns).",
            "detail": "Scope constraints are entered as comma-separated glob text inputs (allowed/forbidden), not the file-tree picker with glob patterns the amendment specifies."
          },
          {
            "file": "web/src/lib/ConstraintEditor.svelte",
            "quote": "async function dryRun() {",
            "detail": "The implemented half: the dry-run button evaluates the assembled constraint set (explicit + scope) against the repo state via the server endpoint and surfaces per-constraint issues."
          },
          {
            "file": "web/src/components/ProvenanceChain.svelte",
            "quote": "/api/v1/repos/${repoId}/attestations/${commitSha}/chain",
            "detail": "Provenance chain visualization fetches the directed-graph chain data for the commit and renders nodes with signer identity, constraint count and verification status."
          }
        ]
      },
      {
        "id": "c8e81723fe471db62eb85bbc",
        "decision": "gap",
        "requirement_quote": "All events include the full `VerificationResult` tree for forensic analysis.",
        "detail": "All seven event names are emitted at the right places, but as structured tracing logs only: attestation.created (approval, spawn derivation, gate attestation), attestation.verified / attestation.constraint_failed (constraint result logging), attestation.chain_invalid (chain and context-binding failures), key_binding.created / key_binding.revoked (key-binding endpoints) — each with category ('Provenance'/'Identity') and event fields. None of them are recorded through the audit system the amendment references: the AuditEvent/AuditEventType machinery (state.audit.record, SIEM forwarding, /api/v1/audit endpoints) has no attestation.* or key_binding.* variants, so these events never reach the audit store or SIEM stream. And no event includes the full VerificationResult tree — the log lines carry scalar fields (constraint counts, messages), and the spec's requirement that every event carry the tree for forensics is unmet.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/constraint_check.rs",
            "quote": "event = \"attestation.verified\",",
            "detail": "The attestation.verified event is emitted as a tracing log with scalar fields only — no VerificationResult tree is attached to the event."
          },
          {
            "file": "crates/gyre-domain/src/audit.rs",
            "quote": "Custom(String),",
            "detail": "The audit event type enum has no attestation.* or key_binding.* variants, so provenance/identity events cannot be recorded into the audit store that observability.md defines (total auditing, SIEM forwarding) — they exist only in tracing output."
          }
        ]
      },
      {
        "id": "67eacb60171e6cedff82c482",
        "decision": "gap",
        "requirement_quote": "- Produce `SignedInput` on every spec approval.",
        "detail": "Four of the five Phase 1 bullets are done: the KeyBinding exchange is implemented (POST /api/v1/auth/key-binding with proof-of-possession, countersignature, storage, TTL cap); attestation chains are stored alongside the existing AttestationBundle (chain store + dual-write at merge); verification runs in audit-only mode (verify_attestation_audit_only, evaluate_push_constraints) that logs and never rejects; and the push flow is otherwise unchanged. The remaining bullet is not met as stated: SignedInput is not produced on every spec approval — production is conditional on the approver having an active KeyBinding AND supplying user_content_signature; otherwise the approval succeeds with no SignedInput (silent skip, see §7.1). Since Phase 1 is explicitly non-enforcing the machinery matches the phase's spirit, but 'every spec approval' is a literal requirement the code does not satisfy.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/specs.rs",
            "quote": "no active KeyBinding for approver — skipping SignedInput creation (Phase 1)",
            "detail": "Approvals without an active KeyBinding (or without user_content_signature) produce no SignedInput — the 'every spec approval' bullet is conditional in production."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "pub(crate) fn verify_attestation_audit_only(",
            "detail": "Audit-only verification is genuinely non-enforcing (logs VerificationResult trees; rejection lives in the separate Phase 3 path), matching the phase's non-enforcing bullets."
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
      },
      {
        "id": "1f7628ebe440a75c8f32588b",
        "decision": "gap",
        "requirement_quote": "- Gate agents produce `GateConstraint` records.",
        "detail": "Mixed: chain verification at push and merge time exists (verify_chain + constraint evaluation in both enforce paths); the AIBOM is extended with full chain attestations per commit; and orchestrator DerivedInput production code exists — but it never fires in the production flow (find_by_task cannot see the unlinked root, §2.1/§4.2). 'Gate agents produce GateConstraint records' is not implemented at all: the only production GateAttestation constructor sets constraint: None and no review/validation agent path creates a GateConstraint (all other constructions are tests). 'Legacy AttestationBundle deprecated' is not done either — the legacy bundle is still generated at every merge and written as the git-note fallback whenever no chain is found (which is always, in production).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/gate_executor.rs",
            "quote": "constraint: None, // Gate constraints are attached by review/validation agents",
            "detail": "No production gate emits a GateConstraint — the phase's explicit bullet is unimplemented (only the consumption side exists)."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "// During the dual-write period, we write EITHER the chain attestation",
            "detail": "The legacy AttestationBundle is still produced at merge and used as the git-note fallback — not deprecated, though this is consistent with the chain never being findable by commit in production."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "no attestation chain for task — skipping DerivedInput creation",
            "detail": "Orchestrator → agent DerivedInput production never fires in the production flow because the parent chain is unreachable by task lookup."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/api/trust_anchors.rs",
        "sha256": "2c66866c4d6f6adb9b38836da2eddb57f9d06aab75a1c0e5eb55397e8b41e518"
      },
      {
        "file": "crates/gyre-domain/src/constraint_evaluator.rs",
        "sha256": "8afbe1cf985b526bdc19cd04cfb8d5495443623bbbbada4477ade29d431f6ac2"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/trust_anchor.rs",
        "sha256": "741c5c4c5f707e05ad26a99ee9b37f3c780ab74de4bdf006a2eefb73f1cf4bc2"
      },
      {
        "file": "crates/gyre-server/src/api/specs.rs",
        "sha256": "701749a457338c0bff05f4c0da748f94759958d1140c9a68edfcf26aa5e0ab41"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "4dc88c1516f2135573d94cf255c246ef1b73ce7b3849683669a6543b21823e60"
      },
      {
        "file": "crates/gyre-server/src/constraint_check.rs",
        "sha256": "e77213a288710523fe9e26a1b0e1e4b1f48854a2bc51c0ac887bfb46de1de3a8"
      },
      {
        "file": "crates/gyre-common/src/attestation.rs",
        "sha256": "259fc089d08f476538b6865e84c21f810779fa717a8ca51c6a4d624696aef930"
      },
      {
        "file": "crates/gyre-server/src/api/key_binding.rs",
        "sha256": "45279359d4659b79d7f2b5c3dfb0dada0442ba70be5abab9c158e1622aa85997"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "fd5036bd1272469fc55a2b304ac10a7db63c95a6d5f546b37a3c2c6c86307149"
      },
      {
        "file": "crates/gyre-server/src/gate_executor.rs",
        "sha256": "c4b7e05809c8be5cd04d754fbcd53619bb8832ac29c355222a5c031972ac1ef5"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "5a5676ba50a366b0ed75b1297e3eb95f1490d18d5983758b869da2201e53e34d"
      },
      {
        "file": "crates/gyre-common/src/key_binding.rs",
        "sha256": "fa5d58b009f6bff20a6da651c85277521eee0eff0909ae0dd993fedeb21123ef"
      },
      {
        "file": "crates/gyre-server/src/api/provenance.rs",
        "sha256": "6861ae70cdcefd73365bd8d75462331d0925336bbb847d77c9b11d1d53aa60d4"
      },
      {
        "file": "crates/gyre-domain/src/attestation.rs",
        "sha256": "a8954208e773442481f6fd3d774571ac77a9e5316b77e0bb34ea38d1072b5e23"
      },
      {
        "file": "crates/gyre-server/src/attestation.rs",
        "sha256": "122270f68398bf06a3c328047e20138cc7a3bdfc449aa7b7559140548ebdb2fa"
      },
      {
        "file": "crates/gyre-ports/src/chain_attestation.rs",
        "sha256": "b2c8482b3f527ebd5732a961f62070901cbe6464cdc816a41c843c976201fd51"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/chain_attestation.rs",
        "sha256": "f3ccd16bc8d8b824d073220aa97520cbe5bc652a314292eea6ec33baca9a2831"
      },
      {
        "file": "crates/gyre-adapters/src/postgres/authorization_provenance.rs",
        "sha256": "aa50befb44aca787a9623b43df5a614372f7d16b5a4c3f034a028a491062bd27"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "46d398754fa7d7921c396160262899d430e24de71bdd53c32e0e7f89c0329afb"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "012f5b3fc89e4f82182168f7c32bb966544d4cfd90201147deb0b6a7971d6d40"
      },
      {
        "file": "crates/gyre-server/src/api/aibom.rs",
        "sha256": "cac01a8dd660c75389f91f5de01f6dce9c4c0029bd27b3c5ae24560e6e889bb8"
      },
      {
        "file": "crates/gyre-domain/src/audit.rs",
        "sha256": "78b4aa3679c84bbadfe186506b04f34c234e3278c57caf84cb64e4b7dfe6aae6"
      },
      {
        "file": "crates/gyre-server/src/abac_middleware.rs",
        "sha256": "45b235e9ae706261d72d29e161333ca9d9b3c66d7a015e6c3624ca60f8cb9d0b"
      },
      {
        "file": "crates/gyre-common/src/message.rs",
        "sha256": "dea344a7ab04b3f2bd754e1335f9143184c5a02508f0763ebd7638f17d735558"
      },
      {
        "file": "web/src/lib/ConstraintEditor.svelte",
        "sha256": "37dbfd7d84c19cdf5db2f0666706eb53db6e1ad3ab82b4641611f6d56a14a391"
      },
      {
        "file": "web/src/components/ProvenanceChain.svelte",
        "sha256": "eb193b67d222657e46cb6498bd020ff5845d64f4216e524d87cfb724f81c4615"
      }
    ]
  },
  "plan": {
    "audit": "288c22f8d08aad1b3d60067aa84453b1c4b815792ba3f8837367a84f2544d869",
    "tasks": [
      {
        "key": "approval-signed-input-root",
        "title": "Enforced KeyBinding and SignedInput at spec approval, spec-update derivation, and constraint-editing UI completion",
        "covers": [
          "234004cc5b4e4a3d78c95ccc",
          "67eacb60171e6cedff82c482",
          "5bf9cf12933a9a01581e530f",
          "dcf557ae5dbc627aab292307",
          "59873e07f74dcd542130bf41"
        ],
        "required_behavior": "Server, approval enforcement (§7.1 / §8 Phase 1): POST /api/v1/specs/:path/approve enforces the key-exchange requirement — the approver must hold a valid, unexpired, unrevoked active KeyBinding and supply user_content_signature; otherwise the request is rejected (4xx) with an error naming POST /api/v1/auth/key-binding, and no SpecApprovalEvent or SignedInput is recorded. The silent skip branches are removed (specs.rs:897 'KeyBinding exists but no user_content_signature provided … skipping SignedInput creation (Phase 1)' and the no-active-KeyBinding skip at specs.rs:904). Every successful approval produces a user-signed SignedInput stored as the chain root (Ed25519 signature over the InputContent hash verified against the binding's public key, as today). KeyBinding document signature (§2.3, review correction): the CLIENT constructs the full binding document — public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at — and signs it with the ephemeral private key; POST /api/v1/auth/key-binding verifies the user's signature over the canonical binding document (not merely proof-of-possession over the raw public key bytes, key_binding.rs:94), validates the claimed user_identity/issuer/trust_anchor_id against the authenticated session and the tenant's trust anchor registry, and countersigns strictly as a registration/timestamp witness — identity attribution of a binding rests on the user's signature, so a compromised server cannot rewrite a stored binding's identity fields and re-countersign, and offline verifiers have user-signed identity claims to check. Server-side assignment of identity fields without a user signature is removed. Client (§7.1): the production approval client (web spec-approval flow, DetailPanel approveCurrentSpec/approveWithConstraints, web/src/lib/DetailPanel.svelte:1038/1056) establishes a KeyBinding first — generate an ephemeral Ed25519 keypair client-side (WebCrypto), construct and sign the binding document, register it, then sign the InputContent hash (registry SHA from GET /specs/:path) and submit user_content_signature — so the enforced requirement does not break the human approval loop. Spec-update derivation (§4.2 trigger 2): when a spec is modified and re-approved and a prior SignedInput exists for the same spec path in the same workspace/repo scope, the new authorization is derived from the prior — the server loads the prior root and the new authorization's output_constraints are the server-computed union of the prior's constraints and the newly submitted ones (additive only, §4.3; forbidden_paths likewise union), with the parent linkage recorded (parent_ref to the prior root attestation's stored id), so a re-approval can never silently drop a previously signed constraint; a client cannot narrow the union. Approval UI (§7.6, review correction): the CEL expression editor in web/src/lib/ConstraintEditor.svelte gains autocomplete for the available context fields (the §3.3 context: input.*, output.*, agent.*, target.*, action — offered as completions, inserting valid references), and scope constraints are entered via a file-tree picker over the repo that emits glob patterns (replacing the plain comma-separated glob text inputs, ConstraintEditor.svelte:167-181); the read-only strategy-implied display and the dry-run button are retained. COORDINATION: the active spec-registry plan's approval-ledger-unification task (planner spec-c08e139362b99830187a32b4) also modifies the approve_spec handler's storage writes (specs.rs:652/:687) — different requirements, same function; sequence or merge deliberately, and do not regress either contract.",
        "acceptance": [
          "Approving with no active KeyBinding (or an expired/revoked one), or with a binding but no user_content_signature, is rejected with an error naming POST /api/v1/auth/key-binding; no approval event or SignedInput is persisted.",
          "Every successful approval persists a SignedInput chain root whose user signature verifies; no conditional skip path remains in the approval handler.",
          "KeyBinding registration verifies the user's signature over the full binding document: a document whose user_signature does not verify, or whose user_identity/issuer/trust_anchor_id disagree with the authenticated session or the tenant's trust anchor registry, is rejected; altering any stored identity field invalidates the user's document signature (identity attribution no longer rests on the platform countersign alone).",
          "The web approval flow works end-to-end against the enforced endpoint: it establishes a binding on first use, signs each approval, and an invalid signature is rejected.",
          "Re-approving a modified spec whose prior approval carried constraints yields a new authorization whose evaluated constraint set contains every prior constraint plus the new ones; the chain records the parent link to the prior root; a re-approval request that omits a prior constraint still carries it (server-side union).",
          "The CEL editor offers context-field autocomplete (selecting a field inserts a valid reference into the expression), and the scope constraint picker selects files/directories from the repo tree and emits glob patterns; the dry-run still evaluates the assembled set."
        ],
        "negative_probe": "At the base: (a) POST /specs/:path/approve with no active KeyBinding returns success and records the approval with no SignedInput (specs.rs:904 logs 'no active KeyBinding for approver — skipping SignedInput creation (Phase 1)') — the probe approves without a binding and must now fail with the key-binding error; (b) the key-binding exchange verifies user_signature only over the raw public key bytes (key_binding.rs:94 'The user signs the public key bytes as a proof-of-possession') while user_identity/issuer/trust_anchor_id are server-assigned and covered only by the platform countersign — probe: rewrite a stored binding's user_identity and re-countersign with the platform key; at the base the forgery is undetectable by any verifier (no user-signed identity claims exist), after the task the user's document signature no longer matches and verification fails; (c) spec-update probe: approve spec v1 with explicit constraint output.changed_files.all(f, f.startsWith(\"src/payments/\")), modify the spec, re-approve without restating it — at the base the prior constraint is absent from the new authorization (the §9 'spec revision silently drops security constraint' attack), after the task it is present; (d) the ConstraintEditor is a plain text input with a placeholder and comma-separated glob inputs (ConstraintEditor.svelte:154, 167-181) — after the task, autocomplete and the file-tree picker exist and are exercised in the approval flow.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "chain-task-commit-linkage",
        "title": "SignedInput roots travel with tasks; attestations bind real commits; merge node with mapped fields; release bundle; PostgreSQL adapters",
        "covers": [
          "0a001bfe1c6ccfa6bd7e4cb3",
          "c49cf3ba98e72736cfcb8bee",
          "1a1c2218815dfe88c38a9789",
          "bb664ae221456d242e8ae921",
          "acda07a63c3a25e9c32461e5"
        ],
        "required_behavior": "Task linkage (§2.1 'travels with every task derived from that spec'): chain lookups keyed by task (spawn's create_derived_input_for_agent, spawn.rs:4292/4307; push/merge enforcement in constraint_check.rs, find_by_task at :51/:288/:594/:844; gate_executor) resolve the approval-time SignedInput root from the task's spec_path — the same resolution §2.4 already uses (newest active approval for the path in the task's workspace/repo scope) — so the chain (root + derived nodes) is locatable for every task whose spec has an approved SignedInput. A root shared by multiple tasks must remain resolvable for each (no single-slot task_id overwrite that orphans sibling tasks; implement via spec-path resolution at lookup or a per-task link, without breaking the §5.4 port's method contract). Observable effects: spawn no longer takes the 'no attestation chain for task — skipping DerivedInput creation' branch for approved specs (a spawner-signed DerivedInput node is created); push/merge enforcement and gate_executor locate the chain. Commit binding (§5.3 locations 2–3, §6.3): when a push is accepted, the pushing agent's chain leaf is updated with the real pushed commit SHA (AttestationOutput.commit_sha; today approval stores commit_sha \"\" at specs.rs and spawn stores commit_sha: String::new() at spawn.rs) and re-persisted (upsert), so find_by_commit matches real commits. The already-implemented downstream machinery must become reachable for real commits: the chain git note is written to refs/notes/attestations on the commit, the AIBOM's per-commit chain_attestation is populated, and GET /api/v1/repos/:id/attestations/:commit_sha/bundle returns the VerificationBundle (chain root-to-leaf, the tenant's trust anchors, the actual git diff, assembly timestamp) instead of 404 'no attestation found for commit'. Release attachment (§6.3, review correction): the VerificationBundle is attached to releases alongside the AIBOM — the release artifact path (GET /repos/:id/aibom?format=release and the release-prepare flow) carries or references the per-commit VerificationBundle, coordinating with the staged supply-chain plan's release-artifacts-aibom-sbom task (planner spec-bdd98e1a5c6d22f3f997de92) and the ci-docs-release plan's release-flow task so the bundle rides the same release-artifact mechanism rather than duplicating release machinery. §5.2 subsumption completion (review correction): AttestationOutput gains the mapping table's extended fields conversation_sha and completion_summary (today the struct has only content_hash/commit_sha/agent_signature/gate_results, gyre-common/src/attestation.rs:181-192, and the values exist only on the legacy MergeAttestation, merge_processor.rs:2830-2834); the merge attestation becomes the final attestation in a chain — merge appends a merge-time attestation node to the chain (linked to the chain leaf, recording the merge commit SHA as AttestationOutput.commit_sha per the merge_commit_sha mapping row, carrying conversation_sha/completion_summary populated from the same sources the legacy bundle uses, and the gate results), so both mapping-table rows that name 'AttestationOutput extended field (unchanged)' are realized on the chain and a chained merge loses no data relative to the legacy bundle. PostgreSQL adapters (§5.4): the PgStorage implementations of ChainAttestationRepository, KeyBindingRepository and TrustAnchorRepository in crates/gyre-adapters/src/postgres/authorization_provenance.rs are real — every method currently anyhow::bail!s (16 stubs); replace them with real SQL against PostgreSQL via the existing store! wiring (PgStorage is selected whenever GYRE_DATABASE_URL is postgres://…, a documented production backend), tenant-scoped like the SQLite adapter, with the same port contract semantics (content-addressed upsert, parent-ref chain walk with loop guard, task/commit/repo indexes, TTL/revocation semantics for key bindings, CRUD for trust anchors); schema comes from the shared embedded migrations that already run on both backends (any new migration uses the next unused 6-digit sequence number and portable SQL per check-migration-sql-portability); on a PostgreSQL deployment the provenance stores must behave identically to SQLite, because later enforcement (boundary-enforcement) must not silently assume store availability it does not have on PG.",
        "acceptance": [
          "For a task whose spec was approved with a SignedInput, spawning an agent creates a spawner-signed DerivedInput chain node (the skip branch does not fire), and the chain (root → derived) is locatable from the task for push/merge enforcement and gate attestation attachment.",
          "Two tasks derived from the same approved spec both resolve the shared root (no orphaning).",
          "After the agent pushes, the chain leaf carries the pushed commit SHA: GET .../attestations/:commit_sha/bundle returns 200 with the full VerificationBundle, the chain note exists on the commit under refs/notes/attestations, and the AIBOM entry for the commit includes the chain.",
          "After merge, a merge-time attestation node is appended to the chain recording the merge commit SHA (merge_commit_sha mapping row) and carrying conversation_sha and completion_summary (both §5.2 mapping rows realized on the chain); the merge-time ABAC gate and note attachment locate the chain by commit.",
          "A produced release artifact carries or references the VerificationBundle for the release's commits (chain + trust anchors + git diff + timestamp) alongside the AIBOM.",
          "On a PostgreSQL deployment (GYRE_DATABASE_URL=postgres://…): chain attestation save/find_by_id/load_chain/find_by_task/find_by_commit/find_by_repo, key-binding persistence, and trust-anchor CRUD all succeed with tenant isolation — no method bails; the approval→spawn→push→merge flow that works on SQLite works on PG."
        ],
        "negative_probe": "At the base: approve a spec with a binding and signature, create a task from it, spawn an agent — spawn logs 'no attestation chain for task — skipping DerivedInput creation' (spawn.rs:4307) and no derived node exists (find_by_task returns nothing because the root is stored with task_id \"\", specs.rs); after the agent pushes, GET .../attestations/<pushed-sha>/bundle returns 404 'no attestation found for commit' (approval and spawn both store commit_sha \"\" and nothing ever updates it), git notes show no chain note for the commit, and push enforcement logs 'no attestation chain found for task (Phase 3 enforcement, allowing)' (constraint_check.rs:299). Merge-node probe: after a merge, the chain contains no merge-time node (merge_processor attaches the pre-existing chain as a git note only, and the legacy AttestationBundle remains the merge-time record), and AttestationOutput has no conversation_sha/completion_summary fields (attestation.rs:181-192) — a consumer reading the chain cannot obtain values the legacy bundle carries. Release probe: the format=release AIBOM artifact contains per-commit chain_attestation but no VerificationBundle, and no release-preparation output references one. PostgreSQL probe: start the server with a postgres:// GYRE_DATABASE_URL and perform any provenance store operation — at the base every one fails with 'ChainAttestationRepository not implemented for PgStorage' (authorization_provenance.rs is a self-described stub, 16 bail! calls); after the task the operations succeed and a PG-backed approval→push→verification flow returns real chain data. Every one of these must flip after the task.",
        "dependencies": [
          "approval-signed-input-root"
        ],
        "existing_task": null
      },
      {
        "key": "chain-verification-trust",
        "title": "Complete chain verification: immutable trust-anchor policy, anchor constraints, binding verification, preconditions, content-addressed node ids, workload claims, configurable depth limits",
        "covers": [
          "80fa5d551228a894b5a85132",
          "74593d79542d482cc5567984",
          "7910f63c3be5b0641f51c47b",
          "827a7cc2ad6eaf0f8e262900",
          "7298cfa0831c074e5e21aa9e"
        ],
        "required_behavior": "§1.1: trust-anchor management (POST/PUT/DELETE /api/v1/tenants/:id/trust-anchors) is guarded by an immutable Deny ABAC policy (builtin, same pattern as builtin:require-human-spec-approval, not overridable by an Allow policy) that denies non-human subjects — an agent principal holding the Admin role can no longer add, remove, or rotate anchors (reads keep the Admin role gate; today protection is handler RBAC only, trust_anchors.rs require_admin). Anchor-level constraints (TrustAnchor.constraints, §1.1/§3.2) are loaded into the evaluated constraint set at verification time for the anchors that authenticated the chain's key bindings — additive with explicit, strategy-implied, and gate constraints (today they are stored and returned by the CRUD API but never evaluated). §4.4 step 2: chain verification resolves each node's key_binding.trust_anchor_id against the tenant-scoped trust anchor registry (a workspace inherits its tenant's anchors) and fails closed on unknown/removed anchors; the platform countersignature on bindings is verified as the registration witness, and — per the §2.3 review correction delivered by approval-signed-input-root — the user's document signature over the binding document is verified so the binding's identity claims are user-attested, not platform-attested; binding expiry is checked (already present). §4.4 steps 4–6: preconditions are evaluated against the parent's verified state (CEL, evaluation errors reject — fail closed; today spawn creates them empty and no verifier executes them); the update expression is evaluated to derive content and the derived content hash must match the claimed content; parent linkage is verified by content hash — parent_ref must equal the parent attestation's stored id, spawn-created parent_ref values must be the parent's stored id (fixing the SHA-of-JSON vs stored-id mismatch; today verify_chain checks only 'has_parent_ref && has_parent_at_lower_depth', git_http.rs), and load_chain must traverse production-created chains root→leaf with no gaps. §5.1 id field (review correction): spawn-created derived attestation node ids are content-addressed — SHA-256 over the node's canonical content, matching the root's SHA-256-of-SignedInput construction — replacing the random uuid::Uuid::new_v4() id (spawn.rs:4470), so identical derivations yield identical ids and any verifier (including offline §6.3 verification) can recompute and validate an id. §4.5: workload key bindings carry the JWT claims (task_id, spawned_by, stack_hash, persona) populated at spawn (today the KeyBinding type has none of these fields, gyre-common/src/key_binding.rs); verification checks them against the attestation metadata/agent context; the synthesized empty-signature fallback bindings are replaced with real signed bindings so the binding attests the workload identity, not just key possession; spawned_by links the orchestrator back to the human who approved the chain root. §4.6: the chain depth limit is workspace-configurable (a strategy-implied chain.depth <= N derived from workspace configuration; Supervised workspaces default to shallow limits such as 2, Autonomous may allow deeper) with the hard safety-net cap of 10 retained; the verification endpoint no longer hardcodes max_depth = 10 (provenance.rs:182 'Will be configurable per workspace in future').",
        "acceptance": [
          "An agent principal with the Admin role cannot create/update/delete trust anchors (403 from the immutable Deny policy; an Allow policy does not override); a human admin still can.",
          "A chain node whose key binding references an unknown or removed trust anchor fails verification with a named reason; a binding with an invalid countersignature fails; a binding whose user document signature does not verify over its identity fields fails (identity is user-attested).",
          "A DerivedInput whose precondition evaluates false (or errors) against the parent state rejects; one whose update expression yields content whose hash differs from the claimed content rejects.",
          "load_chain walks a production-created chain (spawn-derived nodes) root→leaf: each parent_ref equals the parent's stored attestation id.",
          "A spawn-created derived node's id is the SHA-256 of its canonical content: re-hashing the node's content yields the stored id, and two identical derivations produce the same id (recomputable by an offline verifier, unlike uuid::Uuid::new_v4()).",
          "Derived-node key bindings carry task_id/spawned_by/stack_hash/persona, and a mismatch with the acting agent or attestation metadata rejects at verification.",
          "A workspace-configured max chain depth N rejects chains deeper than N; the hard cap 10 still applies; the verification endpoint uses the configured limit.",
          "TrustAnchor.constraints appear in the evaluated constraint set (a violating anchor constraint is reported and, on the enforcing path, rejects)."
        ],
        "negative_probe": "At the base: (a) an agent principal granted UserRole::Admin can PUT /api/v1/tenants/:id/trust-anchors/:aid successfully (protection is handler RBAC only — trust_anchors.rs require_admin — the specced immutable Deny policy does not exist); (b) verify a chain whose leaf key binding has trust_anchor_id 'nonexistent-anchor' — verification accepts it (no anchor resolution exists in any verification call path); (c) craft a DerivedInput with precondition 'false' — verification ignores preconditions entirely (spawn creates them empty and no verifier executes them; git_http.rs checks only 'has_parent_ref && has_parent_at_lower_depth'); (d) load_chain on a spawn-created leaf returns only the leaf (parent_ref is SHA-256 of the parent attestation JSON, never equal to the parent's stored id); (e) a spawn-created node's id is a random UUID (spawn.rs:4470) — re-derive the node's content and hash it: the result does not match the stored id and two identical derivations get different ids, so no offline verifier can validate ids; (f) provenance.rs:182 hardcodes 'let max_depth = 10u32; // Will be configurable per workspace in future'; (g) no verification path checks a user document signature over binding identity fields (the only user signature ever checked is the SignedInput content signature). Each probe must fail after the task (403, named rejection, named rejection, full root→leaf traversal, recomputable content-addressed id, configured N enforced, named binding-signature rejection).",
        "dependencies": [
          "chain-task-commit-linkage"
        ],
        "existing_task": null
      },
      {
        "key": "gate-constraint-emission",
        "title": "Gate agents produce signed GateConstraint records; legacy AttestationBundle deprecated only for fully-mapped chained merges",
        "covers": [
          "3a1de6328a8b4ff8639ece89",
          "1f7628ebe440a75c8f32588b"
        ],
        "required_behavior": "§3.2 Source 3 (emission half): review/validation gate agents attach signed GateConstraint records — a gate whose structured verdict output carries restrictions for subsequent work (e.g. a review gate scoping remaining changes to specific paths) constructs GateAttestation.constraint = Some(GateConstraint { gate_id, gate_name, constraint, signed_by }) signed over the shared signable_bytes with the gate signer key, instead of the unconditional constraint: None (gate_executor.rs:2028). The emitter must be a real production gate path (the gate executor's review/validation gates), driven by the gate's actual verdict output — the platform never fabricates constraints the gate did not produce; constraints are additive only (a gate can tighten, never loosen — the existing accumulate path already enforces concatenation). §8 Phase 4 remaining bullets, with reachability delivered by the dependency: orchestrator→agent DerivedInput production fires in the production flow (chain-task-commit-linkage), full chain verification at push/merge (chain-verification-trust and boundary-enforcement), AIBOM chain attestations per commit (already implemented, reachable via the dependency). Legacy deprecation (review correction — conditional, not unconditional): when a chain attestation is available for the merged work AND the chain's merge-time node carries the §5.2 mapping-table fields (conversation_sha/completion_summary, delivered by chain-task-commit-linkage), the merge processor no longer generates/stores the legacy AttestationBundle (the chain note is the attestation of record and no mapped data is lost); the legacy bundle/type/port remain as the fallback for chain-less legacy work during migration, per §5.2's backward-compatibility retention — do not deprecate the legacy bundle while the chain lacks the mapped fields.",
        "acceptance": [
          "A review/validation gate that produces a restriction in its verdict yields a persisted GateAttestation with constraint: Some(GateConstraint) whose signature verifies over signable_bytes; a production (non-test) GateConstraint constructor exists.",
          "The gate constraint enters the evaluated set at merge: a merge whose final diff violates a gate-attached constraint is rejected naming that constraint; gates cannot loosen the set (no replace/subtract path).",
          "A merge with a chain available produces no new legacy AttestationBundle record or bundle note (chain note only) while the chain's merge node carries conversation_sha/completion_summary — a chained merge loses no data the legacy bundle carried; chain-less merges still write the legacy fallback."
        ],
        "negative_probe": "At the base: run any production review gate and read its GateAttestation — constraint is None (gate_executor.rs:2028 'constraint: None, // Gate constraints are attached by review/validation agents') and the only non-test constructions of GateConstraint are in test modules, so no gate can ever tighten the constraint set; then merge a diff that violates what the review gate required — the merge succeeds because no gate constraint is evaluated. After the task the gate's constraint is present and signature-verified, and the violating merge is rejected naming it; a chained merge whose chain node carries the mapped §5.2 fields leaves no legacy bundle record (at the base one is always generated), while the mapped data (conversation_sha/completion_summary) remains readable from the chain — at the base it exists only on the legacy bundle.",
        "dependencies": [
          "chain-task-commit-linkage"
        ],
        "existing_task": null
      },
      {
        "key": "boundary-enforcement",
        "title": "Mandatory provenance at both delivery routes and merge: enforced builtin policy, fail-closed error paths, workload-identity match, merge-time signature checks, ABAC actions",
        "covers": [
          "290653a126dab6fc9fd71f53",
          "43f8e2cad645c41a4bcdc0a2",
          "4952c3bd11782374a70c9107",
          "29f20a71bb7c6f10151e3b87"
        ],
        "required_behavior": "§7.2/§8 Phase 3: builtin:require-signed-authorization (immutable Deny, priority 998) is enforced, not just seeded — the push-time attestation-scoped ABAC evaluation becomes enforcing (a Deny decision rejects the push with refs undone, replacing the audit-only 'the gate itself remains audit-only (Phase 1)' behavior in git_http.rs) and is evaluated for every push and merge action by non-system subjects, including chain-less work (the attestation-chain-present unless-attribute no longer short-circuits evaluation; the merge-time gate no longer skips evaluation when no chain is found — 'Deny unless a valid attestation chain exists' applies to chain-less merges; today merge proceeds when '!chain_found && denial.is_none()'). System subjects are exempt per the policy conditions. If literal enforcement on human pushes conflicts with another canonical spec (spec-lifecycle.md's non-blocking human push contract, which the active agent-workflow plan's pr-protection task also preserves), resolve the conflict through the spec lifecycle (propose an amendment) — never by silently weakening the policy or leaving it audit-only; the enforcement ships per the canonical §7.2 text unless and until an amendment is approved. Fail-closed error paths: on the enforcing push and merge paths, chain-store query errors and ABAC-evaluation-unavailability errors REJECT with a named reason (refs undone / merge failed) instead of allowing — the 'Query failures are non-blocking' branch in enforce_push_constraints (constraint_check.rs:304) and the 'push attestation ABAC evaluation unavailable (audit-only, push proceeds)' path (git_http.rs) are removed on enforcing paths; a store outage can no longer silently disable mandatory verification (consistent with §3.4's fail-closed philosophy). jj delivery route: the same pre-accept enforcement — chain verification, constraint evaluation, and attestation ABAC — runs at the server-side jj export delivery route (create_mr_for_completion after jj_bookmark_set + jj_git_export in spawn.rs, which today moves the MR source-branch ref with no receive-pack and therefore no verification), coordinating with the staged source-control plan's jj-export-gate-audit-parity task (planner spec-36fad8fcafe7b73a2b6f2d0e, plan 775ba5f9c68bb733dd2a785caaa58464bb763a7ae1490f85cad7de2a805d5a5e) which refactors gate evaluation and push-audit emission into shared production call points used by both routes — this task's attestation/constraint enforcement rides the same shared call point so both delivery routes evaluate the same enforcement, without duplicating logic that can drift; on rejection the source-branch ref is restored to its pre-export position, no MR is created from the violating commits, and the agent's completion fails naming the reason (source-control.md requires push-time validation regardless of whether the client used git or jj). §6.1 fourth bullet: push and merge verification checks that the acting agent's workload identity matches the key binding in the attestation chain — the chain leaf's DerivedInput key binding (agent identity, task_id, spawned_by) must correspond to the acting agent; mismatch rejects with a named reason (today no push/merge path compares the acting agent to the chain's key binding). §6.2 Phase 5 at boundaries: output signature verification (agent_signature over content_hash, every gate signature over signable_bytes) runs in the merge enforcement path, not only in the GET verification endpoint; invalid signatures fail the merge (today a merge whose chain carries an invalid gate signature succeeds). §6.2/§3.3 action context: the GET /api/v1/repos/:id/attestations/:commit_sha/verification endpoint derives the action (push/merge) from the verification context instead of hardcoding Action::Push (provenance.rs:274), so action-sensitive constraints evaluate correctly for merge commits. §7.2 actions/routes: the attestation resource's actions are realized in the ABAC engine — /api/v1/attestations/verify maps to the verify action (not 'read', abac_middleware.rs:423-427), GET .../attestations/:commit_sha/bundle maps to export, .../verification to verify, .../chain to read; the three repos/:id/attestations/:commit_sha/* routes are removed from scripts/abac-route-registry-exemptions.txt (lines 31-33; shrink only — the file is frozen against growth) and registered in the RouteResourceMapping resolver, with per-handler tenant/workspace authorization where middleware scoping does not apply; the revoke action is registered in the attestation resource's action vocabulary for policy authoring (no revocation endpoint is specced in this document — creating one would be a speculative feature and is out of scope). The already-populated subject attributes (chain_depth, root_signer, constraint_count) feed the enforced evaluation and its decision audit trail.",
        "acceptance": [
          "A push by a non-system subject with no valid attestation chain is rejected (403, refs undone) with the policy named; a chain-less merge is denied; system-subject actions are exempt.",
          "The policy is immutable: an Allow policy cannot override the Deny (same immutability class as builtin:require-human-spec-approval).",
          "Fail-closed: a simulated chain-store error (or ABAC evaluation unavailability) on the enforcing push/merge path results in REJECTION with a named reason, not allowance.",
          "A jj-workspace agent delivery with no valid chain is rejected at delivery with the source-branch ref restored to its pre-export position and no MR created — not first at merge.",
          "A push/merge by an agent whose identity does not match the chain leaf's key binding (different agent or task) is rejected naming the workload-identity mismatch.",
          "A merge fails when an agent_signature or gate signature on the chain is invalid; valid signatures pass.",
          "The verification endpoint evaluates action-sensitive constraints with the correct action for merge commits (no hardcoded Push).",
          "The attestation routes are policy-evaluated (resolver entries with the attestation resource and verify/export actions; the exemption file no longer lists them — its entry count shrinks); a Deny attestation policy blocks an out-of-scope caller on these routes.",
          "End-to-end positive path: a task with a valid chain, matching workload identity, and satisfied constraints pushes and merges successfully — legitimate attested work is not falsely rejected."
        ],
        "negative_probe": "At the base: (a) an agent pushes with no chain and is accepted — constraint_check.rs:299 logs 'no attestation chain found for task (Phase 3 enforcement, allowing)' and the push-time attestation ABAC gate is audit-only by its own comment; (b) a chain-less merge proceeds because merge_processor skips evaluation when '!chain_found && denial.is_none()'; (c) simulating a chain-store error (find_by_task returns Err) allows the push ('Query failures are non-blocking', constraint_check.rs:304) and ABAC evaluation unavailability proceeds ('audit-only, push proceeds'); (d) a jj-workspace agent's delivery via create_mr_for_completion runs jj_git_export and creates an MR with no pre-accept verification at all; (e) a merge whose chain carries an invalid gate signature succeeds (Phase 5 output-signature verification only runs in the GET endpoint); (f) the three attestation routes sit in scripts/abac-route-registry-exemptions.txt (lines 31-33) so no middleware policy evaluation runs and the handlers perform no per-handler tenant/workspace authorization; (g) /api/v1/attestations/verify resolves to action 'read' (abac_middleware.rs:423-427); (h) the verification endpoint builds every CEL context with Action::Push (provenance.rs:274), so a constraint like 'action == \"merge\" ? … : …' misevaluates for merge commits. Workload-identity probe: push agent A's commits while the chain leaf's key binding names agent B (or another task) — at the base the push is accepted (no identity comparison exists in any push/merge path). Each must flip: chain-less non-system push/merge rejected with the policy named, store errors reject, jj delivery rejected with the ref restored and no MR, invalid gate signature fails the merge, routes resolver-registered and policy-evaluated, verify action mapped, correct action per verification context, identity mismatch rejected.",
        "dependencies": [
          "chain-task-commit-linkage",
          "chain-verification-trust",
          "agent-output-signing"
        ],
        "existing_task": null
      },
      {
        "key": "verification-result-persistence",
        "title": "VerificationResult trees persisted on attestations and provenance/identity events recorded in the audit system",
        "covers": [
          "f3d63f8ba82d318a407da9e3",
          "c8e81723fe471db62eb85bbc"
        ],
        "required_behavior": "§6.4: every verification (push enforcement, merge enforcement, and the GET .../verification endpoint) produces a VerificationResult tree that is attached to the attestation record and stored for audit — persisted through the existing ChainAttestationRepository (attached to the attestation record and re-saved, so it is durable across restarts on every backend including PostgreSQL once chain-task-commit-linkage lands the PG adapter; no bare in-memory AppState map, per the in-memory-state-stores invariant), queryable via GET /api/v1/repos/:id/attestations/:commit_sha/verification (the endpoint returns the stored tree; recomputation may remain only as a fallback when none is stored — today the endpoint recomputes on each request and no code path stores a result tree against an attestation record). §7.7: the seven event kinds — attestation.created, attestation.verified, attestation.constraint_failed, attestation.chain_invalid (Provenance) and key_binding.created, key_binding.expired, key_binding.revoked (Identity) — are recorded through the audit system (AuditEvent records with new AuditEventType variants/categories, state.audit persistence, SIEM forwarding, /api/v1/audit queryability) in addition to the existing tracing logs (today they exist only as structured tracing logs with scalar fields; the AuditEventType enum in gyre-domain/src/audit.rs ends at supply-chain/Custom with no provenance or identity variants); every event that arises from a verification includes the full serialized VerificationResult tree in the audit record for forensic analysis, not just scalar fields. Existing tracing logs are retained (no regression).",
        "acceptance": [
          "After a push/merge verification, the attestation record carries the VerificationResult tree (with children naming failing constraints); GET .../verification returns the stored tree.",
          "The seven event kinds are queryable via /api/v1/audit with the correct categories (Provenance/Identity); events arising from verifications embed the full VerificationResult tree.",
          "Verification results survive a server restart (durable storage); key-binding lifecycle events (create/expire/revoke) are recorded as Identity audit events."
        ],
        "negative_probe": "At the base: after any verification no VerificationResult is persisted against the attestation (the endpoint recomputes the tree on each request; no code path stores a result tree against an attestation record), and /api/v1/audit returns no attestation.* or key_binding.* events (the AuditEventType enum in gyre-domain/src/audit.rs ends at supply-chain/Custom — no provenance or identity variants exist), while attestation.verified exists only as a tracing log with scalar fields and no tree (constraint_check.rs:1569). Probe: run a verification, then query the audit store and the attestation record — both come back empty at the base; after the task the stored tree is returned by the endpoint and the audit query finds the events with embedded trees.",
        "dependencies": [
          "chain-task-commit-linkage"
        ],
        "existing_task": null
      },
      {
        "key": "agent-output-signing",
        "title": "Agent-side provenance: DerivedInput and KeyBinding injected into the agent environment; the agent signs its output at push time",
        "covers": [
          "e5da488abf98ee7cd64ea91f"
        ],
        "required_behavior": "§7.4 steps 4–5: spawn injects the workload KeyBinding and DerivedInput into the agent's environment as machine-readable env vars alongside the existing GYRE_* variables (e.g. GYRE_KEY_BINDING / GYRE_DERIVED_INPUT, with the private key delivered the same way GYRE_SVID_KEY / GYRE_AUTH_TOKEN are), replacing server-KV-only storage as the delivery mechanism (today spawn stores them only in server-side KV namespaces — kv_set(\"agent_derived_inputs\", agent_id, …) — with no env/protocol delivery to the container); the binding expires with the agent's JWT; a re-spawned agent (Ralph loop iteration) gets a new KeyBinding and a new DerivedInput derived from the same parent, preserving the constraint chain (no constraints dropped). The agent signs its output at push time: the agent runner (docker/gyre-agent/agent-runner.mjs, using the established pre-push http.extraHeader mechanism that already injects X-Gyre-Conversation-Turn) signs the pushed content with the injected key — the signature covers the content hash the server computes for the push (its commit SHA(s)) — and attaches it via a push header (e.g. X-Gyre-Output-Attestation-Signature carrying agent id, content hash, signature). The forge's receive path verifies the signature against the chain leaf's key binding public key, records agent_signature on the leaf (with the commit SHA per chain-task-commit-linkage; today every production construction site of AttestationOutput sets agent_signature: None), and rejects pushes whose signature does not match the binding's key or the pushed content (fail closed). Raw git pushes without the runner keep today's agent_signature-absent behavior (§5.1 'if capable'); the spawned-agent runner path signs by default.",
        "acceptance": [
          "A spawned agent's container environment includes the derived input and key binding (plus private key) as env vars; the values match the stored chain node and binding.",
          "The runner's push carries the output signature; the server verifies it against the leaf's key binding and persists agent_signature on the chain leaf for the pushed commit; the GET verification endpoint's Phase-5 agent_signature check passes for that commit.",
          "A push with a forged/mismatched signature header (wrong key or mismatched content hash) is rejected (403, refs undone).",
          "A Ralph-loop re-spawn produces a new KeyBinding and a new DerivedInput from the same parent — the chain grows and no constraints are dropped."
        ],
        "negative_probe": "At the base: a spawned agent's environment contains no key-binding or derived-input variables (spawn stores them only in server-side KV namespaces — kv_set(\"agent_derived_inputs\", …) — and no env/protocol delivery to the container exists), and every production AttestationOutput sets agent_signature: None (no production construction site sets it), so the agent key is generated and stored but never used to sign work output and the verification endpoint's Phase-5 agent-signature check has nothing to verify. Probe: inspect the spawned container's env (no GYRE_KEY_BINDING/GYRE_DERIVED_INPUT) and read the leaf after a push (agent_signature null); submit a push whose signature header was produced with a different key — at the base it is ignored/accepted; after the task the env vars are present, the leaf carries a verified agent_signature, and the forged-signature push is rejected.",
        "dependencies": [
          "chain-task-commit-linkage",
          "chain-verification-trust"
        ],
        "existing_task": null
      },
      {
        "key": "stack-hash-constraint-unification",
        "title": "Stack-hash attestation unified into the CEL constraint set as one constraint among many",
        "covers": [
          "d181208ee20b618ef25389b3"
        ],
        "required_behavior": "§7.3: the stack-hash equality check becomes a strategy-implied CEL constraint (agent.stack_hash == \"<expected fingerprint>\") derived from the repo's gyre-stack.lock in the common derive_strategy_constraints path (gyre-domain/src/constraint_evaluator.rs), evaluated by the single verification algorithm at push and merge alongside the explicit, strategy-implied, and gate constraints — the stack attestation is one constraint among many, and the two push-time verification paths (stack attestation + constraint verification) are unified into a single algorithm (today the equality check lives only inside enforce_stack_policy's separate path in git_http.rs, outside the CEL pipeline, while the attestation-level half is already unified). Because the constraint is derived in the common strategy-implied path, every route and boundary that evaluates the constraint set evaluates it — receive-pack and merge today, and the jj export delivery route once boundary-enforcement / the staged source-control plan's jj-export-gate-audit-parity establishes the shared enforcement call point there (no route-specific stack check). Always-on semantics are preserved: the repo-level stack constraint is derived and evaluated on every push to a policy-bearing repo, with or without an attestation chain (a chain-less push's constraint set is the repo-level set rather than empty), so unification never weakens stack enforcement relative to today's separate check. The separate stack-hash-equality branch in enforce_stack_policy is retired in favor of the unified constraint, while supply-chain.md's own observable policy semantics are preserved: Block mode = the constraint is enforcing (violation rejects with refs undone, naming the constraint); Warn mode = the violation emits the ConstraintViolation event and stack-drift audit record without rejecting; min-attestation-level rejection and the lockfile-drift surfaces (GET /repos/:id/stack-drift) keep working. The CEL context's agent.stack_hash continues to come from real attestation state (verified attestations per the supply-chain plan's cli-push-stack-attestation signing mechanism, or platform-observed state) — never an unverified self-report. Coordinate with the staged supply-chain plan (planner spec-bdd98e1a5c6d22f3f997de92, plan b2576f0bb5cd13421ed650b13f3912dad0c8bb4f739145bc108b9e2fc2a0a571, tasks cli-push-stack-attestation and forge-verification-override): land the unification as the evaluation-path change those requirements compose with (always-on evaluation, attributed override grants), not as a removal of the policy semantics they specify.",
        "acceptance": [
          "For a repo whose gyre-stack.lock pins a fingerprint, the constraint set assembled at push/merge includes the stack-hash equality constraint; a push by an agent whose attested stack_hash differs is rejected in Block mode naming the strategy stack-hash constraint, and in Warn mode emits ConstraintViolation plus the stack-drift audit record without rejecting — the same observable outcomes as today, via the single CEL pipeline.",
          "A repo without a lockfile pin derives no stack-hash constraint (no behavior change for unpinned repos).",
          "The separate stack-hash equality branch no longer exists outside the constraint pipeline (single verification algorithm); min-attestation-level rejection, drift detection, and the stack-drift audit trail still function."
        ],
        "negative_probe": "At the base: derive_strategy_constraints emits no stack-hash constraint (no 'agent.stack_hash ==' constraint is derived anywhere; the equality check lives only inside enforce_stack_policy's separate path at git_http.rs, outside the CEL pipeline). Probe: collect the evaluated constraint set for a pushing agent whose attested stack drifts from the lockfile — the set contains no stack constraint at the base; after the task the constraint is present and a Block-mode drift push is rejected naming it (with agent.stack_hash taken from real attestation state, so a falsely self-reported hash still fails).",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "approval-signed-input-root": "task-351",
    "chain-task-commit-linkage": "task-352",
    "chain-verification-trust": "task-353",
    "gate-constraint-emission": "task-354",
    "boundary-enforcement": "task-355",
    "verification-result-persistence": "task-356",
    "agent-output-signing": "task-357",
    "stack-hash-constraint-unification": "task-358",
    "_publication": "task-359",
    "_base": "f3ca7db3bf7ee4b35cab550ad18011c13d8680c4"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
