---
title: "SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / spiffe-workload-attestation / b60edce5bc49"
depends_on: []
progress: not-started
commits: []
---

## Required behavior

Implement Layer 1 of the identity stack with the specced existing protocol instead of the bespoke G10 local scheme (workload_attestation.rs self-reports PID/hostname/stack fingerprint and 'verifies' via a /proc existence probe plus a string compare that skips when either side is empty — a custom auth scheme the spec's 'All use existing protocols - no custom auth schemes' constraint forbids). Required: (1) X.509-SVIDs per the SPIFFE X.509-SVID specification: each agent session receives an SVID whose leaf certificate carries its SPIFFE ID as a URI SAN in the exact spec format spiffe://{trust-domain}/agent/{role}/session/{id} (trust domain configurable, e.g. GYRE_SPIFFE_TRUST_DOMAIN). When an external SPIRE server / Workload API is configured, SVIDs are obtained from it; otherwise Gyre issues SPIFFE-conformant SVIDs from a trust-domain CA it operates. (2) Cryptographic verification: the workload-identity verification path validates a presented SVID's chain against the trust bundle (SPIFFE bundle format), enforces certificate validity periods, and parses/binds the SPIFFE ID to the agent session — replacing the /proc/{pid} probe and the empty-string-skipping stack compare as the 'workload is real' proof. SVID issuance is gated on the spawn-time authorization evidence (authorized compute target / stack fingerprint), so the runtime proves it runs on authorized infrastructure. (3) Secure-by-default: an agent that cannot present a valid SVID fails workload-identity verification — no bypass path for unattested agents on the verification surface. (4) The server publishes its trust bundle at the standard SPIFFE well-known location so any verifier can check SVIDs. (5) The existing wl_* JWT claim contract is preserved: claims continue to be embedded and now reflect the SVID-backed attestation (stack fingerprint bound to the attested workload, not a free-form self-report). The JWT remains the Layer-2 permission token; the SVID is the Layer-1 workload proof, presented and verified on the attestation path (spawn/attest/heartbeat/verify), not merely recorded as metadata.

## Acceptance Criteria

- [ ] A spawned agent session holds an X.509-SVID whose URI SAN is exactly spiffe://{trust-domain}/agent/{role}/session/{session-id} for that session, and the server's verification path validates the chain against the published trust bundle and extracts the SPIFFE ID.
- [ ] Workload-identity verification rejects, fail-closed: an SVID signed by a key not in the trust bundle, an expired/not-yet-valid SVID, an SVID whose SPIFFE ID names a different agent session or a foreign trust domain, and a request presenting no SVID.
- [ ] The trust bundle is served in SPIFFE bundle format at the well-known endpoint and is the sole verification anchor (verification never trusts self-reported metadata).
- [ ] The wl_* JWT claims remain embedded for spawned agents and the stack fingerprint they carry is bound to the SVID-backed attestation record.
- [ ] The G10 /proc-probe and skip-on-empty string comparison are no longer the workload-identity verification on this path (grep-level: no verification path succeeds on an unattested or tampered workload).

## Verification intent

Take a spawned agent's SVID and substitute one signed by an unknown key but carrying a well-formed SPIFFE ID for the same session (or tamper with the stack fingerprint between attestation and verification, or present an empty stack string): the workload-identity verification must reject. Against current code this probe passes verification today (verify_attestation skips when either stack string is empty and nothing is cryptographic), so the probe fails on the pre-task code and only passes once real chain validation exists — asserting a 'spiffe' string or struct existing is not sufficient.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/identity-security.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e`.

[
  {
    "id": "9d623241914634d94371c864",
    "decision": "gap",
    "requirement_quote": "Three layers, each proving something different. All use existing protocols - no custom auth schemes.",
    "detail": "The three-layer structure exists in production (agent JWTs via AgentSigningKey, Sigstore Fulcio/Rekor client, workload attestation records), and Layers 2-3 use existing protocols (JWT/OIDC, Sigstore). However the section's own constraint 'All use existing protocols - no custom auth schemes' is violated at Layer 1: production replaced the specced SPIFFE/SPIRE protocol with a bespoke 'G10 local workload attestation' scheme (PID/hostname/stack-fingerprint record plus a /proc existence probe), whose module header explicitly calls it a 'pragmatic SPIFFE alternative'. The protocol-specific divergence is fully accounted under the Layer 1 child section; this parent records the constraint violation itself. The per-layer requirements are not re-counted here.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/workload_attestation.rs",
        "quote": "//! G10: Local workload attestation — pragmatic SPIFFE alternative.",
        "detail": "The entire Layer-1 implementation is a bespoke local scheme, not an existing protocol. Its header continues: 'Provides lightweight workload identity for agent processes without requiring an external SPIFFE/SPIRE infrastructure.' The spec's 'no custom auth schemes' constraint is therefore violated in production by the layer that was supposed to use SPIFFE/SPIRE."
      }
    ]
  },
  {
    "id": "16fb4f5c865aa918b828b94e",
    "decision": "gap",
    "requirement_quote": "- **SPIFFE** for cryptographic workload attestation *(Loom's weaver uses SVID for mutual TLS auth - strong reference)*.",
    "detail": "Required: SPIFFE-based cryptographic workload attestation proving the runtime runs on authorized infrastructure, SPIFFE IDs of the form spiffe://gyre.example.com/agent/{role}/session/{id}, and federation via SPIFFE trust bundle exchange between SPIRE servers. Actual production code has none of this. The sole Layer-1 implementation is a local, self-reported metadata record: OS PID, hostname read from /proc/sys/kernel/hostname, a compute-target label, a self-reported stack fingerprint, and optional container id/image digest, recorded by the server at spawn. 'Liveness verification' is a /proc/{pid} existence probe; 'verify_attestation' string-compares a caller-supplied stack string and skips when either side is empty. Repo-wide grep for 'spiffe://' matches only the two spec files (specs/system/identity-security.md, specs/system/supply-chain.md); grep for 'svid'/'mtls' across crates/ returns zero matches; there is no SPIRE agent/socket, no x509 SVID, no mTLS transport (agents authenticate with JWT bearer over HTTP), and no SPIFFE trust-bundle exchange. Nothing cryptographic binds the workload to authorized infrastructure, so 'the runtime proves it's running on authorized infrastructure' is not performed. The 'agents operate in secure-by-default environments' bullet likewise has no enforcing code.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/workload_attestation.rs",
        "quote": "//! an external SPIFFE/SPIRE infrastructure.",
        "detail": "Full header: 'Provides lightweight workload identity for agent processes without requiring an external SPIFFE/SPIRE infrastructure.' This is an explicit admission that the specced SPIFFE/SPIRE attestation is not implemented; the replacement collects local, self-reported metadata instead of cryptographic proof."
      },
      {
        "file": "crates/gyre-server/src/workload_attestation.rs",
        "quote": "std::path::Path::new(&format!(\"/proc/{pid}\")).exists()",
        "detail": "The only 'workload is real' verification is a local filesystem existence probe of the process PID — not cryptographic attestation, and not proof of authorized infrastructure. Inspected call paths: WorkloadAttestation records are created in api/spawn.rs at spawn and only re-checked via this probe plus a string comparison of the stack fingerprint."
      }
    ]
  }
]
