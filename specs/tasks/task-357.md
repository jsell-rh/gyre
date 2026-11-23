---
title: "Agent-side provenance: DerivedInput and KeyBinding injected into the agent environment; the agent signs its output at push time"
spec_ref: "Spec gap spec-8493af123949c4d249a7ab8c / agent-output-signing / 1d3b1257654b"
depends_on: [task-352, task-353]
progress: not-started
commits: []
---

## Required behavior

§7.4 steps 4–5: spawn injects the workload KeyBinding and DerivedInput into the agent's environment as machine-readable env vars alongside the existing GYRE_* variables (e.g. GYRE_KEY_BINDING / GYRE_DERIVED_INPUT, with the private key delivered the same way GYRE_SVID_KEY / GYRE_AUTH_TOKEN are), replacing server-KV-only storage as the delivery mechanism (today spawn stores them only in server-side KV namespaces — kv_set("agent_derived_inputs", agent_id, …) — with no env/protocol delivery to the container); the binding expires with the agent's JWT; a re-spawned agent (Ralph loop iteration) gets a new KeyBinding and a new DerivedInput derived from the same parent, preserving the constraint chain (no constraints dropped). The agent signs its output at push time: the agent runner (docker/gyre-agent/agent-runner.mjs, using the established pre-push http.extraHeader mechanism that already injects X-Gyre-Conversation-Turn) signs the pushed content with the injected key — the signature covers the content hash the server computes for the push (its commit SHA(s)) — and attaches it via a push header (e.g. X-Gyre-Output-Attestation-Signature carrying agent id, content hash, signature). The forge's receive path verifies the signature against the chain leaf's key binding public key, records agent_signature on the leaf (with the commit SHA per chain-task-commit-linkage; today every production construction site of AttestationOutput sets agent_signature: None), and rejects pushes whose signature does not match the binding's key or the pushed content (fail closed). Raw git pushes without the runner keep today's agent_signature-absent behavior (§5.1 'if capable'); the spawned-agent runner path signs by default.

## Acceptance Criteria

- [ ] A spawned agent's container environment includes the derived input and key binding (plus private key) as env vars; the values match the stored chain node and binding.
- [ ] The runner's push carries the output signature; the server verifies it against the leaf's key binding and persists agent_signature on the chain leaf for the pushed commit; the GET verification endpoint's Phase-5 agent_signature check passes for that commit.
- [ ] A push with a forged/mismatched signature header (wrong key or mismatched content hash) is rejected (403, refs undone).
- [ ] A Ralph-loop re-spawn produces a new KeyBinding and a new DerivedInput from the same parent — the chain grows and no constraints are dropped.

## Verification intent

At the base: a spawned agent's environment contains no key-binding or derived-input variables (spawn stores them only in server-side KV namespaces — kv_set("agent_derived_inputs", …) — and no env/protocol delivery to the container exists), and every production AttestationOutput sets agent_signature: None (no production construction site sets it), so the agent key is generated and stored but never used to sign work output and the verification endpoint's Phase-5 agent-signature check has nothing to verify. Probe: inspect the spawned container's env (no GYRE_KEY_BINDING/GYRE_DERIVED_INPUT) and read the leaf after a push (agent_signature null); submit a push whose signature header was produced with a different key — at the base it is ignored/accepted; after the task the env vars are present, the leaf carries a verified agent_signature, and the forged-signature push is rejected.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/authorization-provenance.md`. Source: `973068a86ab3154b6cba448fda9369125326cab7`; plan: `fdf43e24079d62bc303fa18c819671b940fa5414c868ffdc489bfd20a3d957c0`.

[
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
  }
]
