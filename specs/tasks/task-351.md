---
title: "Enforced KeyBinding and SignedInput at spec approval, spec-update derivation, and constraint-editing UI completion"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / approval-signed-input-root / 1d3b1257654b"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Server, approval enforcement (§7.1 / §8 Phase 1): POST /api/v1/specs/:path/approve enforces the key-exchange requirement — the approver must hold a valid, unexpired, unrevoked active KeyBinding and supply user_content_signature; otherwise the request is rejected (4xx) with an error naming POST /api/v1/auth/key-binding, and no SpecApprovalEvent or SignedInput is recorded. The silent skip branches are removed (specs.rs:897 'KeyBinding exists but no user_content_signature provided … skipping SignedInput creation (Phase 1)' and the no-active-KeyBinding skip at specs.rs:904). Every successful approval produces a user-signed SignedInput stored as the chain root (Ed25519 signature over the InputContent hash verified against the binding's public key, as today). KeyBinding document signature (§2.3, review correction): the CLIENT constructs the full binding document — public_key, user_identity, issuer, trust_anchor_id, issued_at, expires_at — and signs it with the ephemeral private key; POST /api/v1/auth/key-binding verifies the user's signature over the canonical binding document (not merely proof-of-possession over the raw public key bytes, key_binding.rs:94), validates the claimed user_identity/issuer/trust_anchor_id against the authenticated session and the tenant's trust anchor registry, and countersigns strictly as a registration/timestamp witness — identity attribution of a binding rests on the user's signature, so a compromised server cannot rewrite a stored binding's identity fields and re-countersign, and offline verifiers have user-signed identity claims to check. Server-side assignment of identity fields without a user signature is removed. Client (§7.1): the production approval client (web spec-approval flow, DetailPanel approveCurrentSpec/approveWithConstraints, web/src/lib/DetailPanel.svelte:1038/1056) establishes a KeyBinding first — generate an ephemeral Ed25519 keypair client-side (WebCrypto), construct and sign the binding document, register it, then sign the InputContent hash (registry SHA from GET /specs/:path) and submit user_content_signature — so the enforced requirement does not break the human approval loop. Spec-update derivation (§4.2 trigger 2): when a spec is modified and re-approved and a prior SignedInput exists for the same spec path in the same workspace/repo scope, the new authorization is derived from the prior — the server loads the prior root and the new authorization's output_constraints are the server-computed union of the prior's constraints and the newly submitted ones (additive only, §4.3; forbidden_paths likewise union), with the parent linkage recorded (parent_ref to the prior root attestation's stored id), so a re-approval can never silently drop a previously signed constraint; a client cannot narrow the union. Approval UI (§7.6, review correction): the CEL expression editor in web/src/lib/ConstraintEditor.svelte gains autocomplete for the available context fields (the §3.3 context: input.*, output.*, agent.*, target.*, action — offered as completions, inserting valid references), and scope constraints are entered via a file-tree picker over the repo that emits glob patterns (replacing the plain comma-separated glob text inputs, ConstraintEditor.svelte:167-181); the read-only strategy-implied display and the dry-run button are retained. COORDINATION: the active spec-registry plan's approval-ledger-unification task (planner spec-c08e139362b99830187a32b4) also modifies the approve_spec handler's storage writes (specs.rs:652/:687) — different requirements, same function; sequence or merge deliberately, and do not regress either contract.

## Acceptance Criteria

- [ ] Approving with no active KeyBinding (or an expired/revoked one), or with a binding but no user_content_signature, is rejected with an error naming POST /api/v1/auth/key-binding; no approval event or SignedInput is persisted.
- [ ] Every successful approval persists a SignedInput chain root whose user signature verifies; no conditional skip path remains in the approval handler.
- [ ] KeyBinding registration verifies the user's signature over the full binding document: a document whose user_signature does not verify, or whose user_identity/issuer/trust_anchor_id disagree with the authenticated session or the tenant's trust anchor registry, is rejected; altering any stored identity field invalidates the user's document signature (identity attribution no longer rests on the platform countersign alone).
- [ ] The web approval flow works end-to-end against the enforced endpoint: it establishes a binding on first use, signs each approval, and an invalid signature is rejected.
- [ ] Re-approving a modified spec whose prior approval carried constraints yields a new authorization whose evaluated constraint set contains every prior constraint plus the new ones; the chain records the parent link to the prior root; a re-approval request that omits a prior constraint still carries it (server-side union).
- [ ] The CEL editor offers context-field autocomplete (selecting a field inserts a valid reference into the expression), and the scope constraint picker selects files/directories from the repo tree and emits glob patterns; the dry-run still evaluates the assembled set.

## Verification intent

At the base: (a) POST /specs/:path/approve with no active KeyBinding returns success and records the approval with no SignedInput (specs.rs:904 logs 'no active KeyBinding for approver — skipping SignedInput creation (Phase 1)') — the probe approves without a binding and must now fail with the key-binding error; (b) the key-binding exchange verifies user_signature only over the raw public key bytes (key_binding.rs:94 'The user signs the public key bytes as a proof-of-possession') while user_identity/issuer/trust_anchor_id are server-assigned and covered only by the platform countersign — probe: rewrite a stored binding's user_identity and re-countersign with the platform key; at the base the forgery is undetectable by any verifier (no user-signed identity claims exist), after the task the user's document signature no longer matches and verification fails; (c) spec-update probe: approve spec v1 with explicit constraint output.changed_files.all(f, f.startsWith("src/payments/")), modify the spec, re-approve without restating it — at the base the prior constraint is absent from the new authorization (the §9 'spec revision silently drops security constraint' attack), after the task it is present; (d) the ConstraintEditor is a plain text input with a placeholder and comma-separated glob inputs (ConstraintEditor.svelte:154, 167-181) — after the task, autocomplete and the file-tree picker exist and are exercised in the approval flow.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
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
  }
]
