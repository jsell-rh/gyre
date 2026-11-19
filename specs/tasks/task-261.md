---
title: "SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / spiffe-workload-attestation / b60edce5bc49"
depends_on: []
progress: complete
commits: ["c9a4d033", "354a5e7a"]
---

## Required behavior

Implement Layer 1 of the identity stack with the specced existing protocol instead of the bespoke G10 local scheme (workload_attestation.rs self-reports PID/hostname/stack fingerprint and 'verifies' via a /proc existence probe plus a string compare that skips when either side is empty — a custom auth scheme the spec's 'All use existing protocols - no custom auth schemes' constraint forbids). Required: (1) X.509-SVIDs per the SPIFFE X.509-SVID specification: each agent session receives an SVID whose leaf certificate carries its SPIFFE ID as a URI SAN in the exact spec format spiffe://{trust-domain}/agent/{role}/session/{id} (trust domain configurable, e.g. GYRE_SPIFFE_TRUST_DOMAIN). When an external SPIRE server / Workload API is configured, SVIDs are obtained from it; otherwise Gyre issues SPIFFE-conformant SVIDs from a trust-domain CA it operates. (2) Cryptographic verification: the workload-identity verification path validates a presented SVID's chain against the trust bundle (SPIFFE bundle format), enforces certificate validity periods, and parses/binds the SPIFFE ID to the agent session — replacing the /proc/{pid} probe and the empty-string-skipping stack compare as the 'workload is real' proof. SVID issuance is gated on the spawn-time authorization evidence (authorized compute target / stack fingerprint), so the runtime proves it runs on authorized infrastructure. (3) Secure-by-default: an agent that cannot present a valid SVID fails workload-identity verification — no bypass path for unattested agents on the verification surface. (4) The server publishes its trust bundle at the standard SPIFFE well-known location so any verifier can check SVIDs. (5) The existing wl_* JWT claim contract is preserved: claims continue to be embedded and now reflect the SVID-backed attestation (stack fingerprint bound to the attested workload, not a free-form self-report). The JWT remains the Layer-2 permission token; the SVID is the Layer-1 workload proof, presented and verified on the attestation path (spawn/attest/heartbeat/verify), not merely recorded as metadata.

## Acceptance Criteria

- [x] A spawned agent session holds an X.509-SVID whose URI SAN is exactly spiffe://{trust-domain}/agent/{role}/session/{session-id} for that session, and the server's verification path validates the chain against the published trust bundle and extracts the SPIFFE ID.
- [x] Workload-identity verification rejects, fail-closed: an SVID signed by a key not in the trust bundle, an expired/not-yet-valid SVID, an SVID whose SPIFFE ID names a different agent session or a foreign trust domain, and a request presenting no SVID.
- [x] The trust bundle is served in SPIFFE bundle format at the well-known endpoint and is the sole verification anchor (verification never trusts self-reported metadata).
- [x] The wl_* JWT claims remain embedded for spawned agents and the stack fingerprint they carry is bound to the SVID-backed attestation record.
- [x] The G10 /proc-probe and skip-on-empty string comparison are no longer the workload-identity verification on this path (grep-level: no verification path succeeds on an unattested or tampered workload).

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

## Shipped

- `crates/gyre-spiffe` (commit c9a4d033): `SpiffeId` (fail-closed parse/build of the spec's
  `spiffe://{trust-domain}/agent/{role}/session/{id}` form), `TrustDomainCa` (ECDSA P-256 via
  rcgen; SVID = leaf + key per the SPIFFE X.509-SVID specification, SPIFFE ID as the leaf's
  single URI SAN; serializable for operator provisioning/backup), `verify_svid` (chain
  validation via x509-parser `verify_signature` against the trust bundle's anchors, validity
  windows on leaf and signer, exactly-one-URI-SAN extraction, session binding), trust-bundle
  encode/parse (SPIFFE bundle format: JWK Set, `use: "svid"`, `x5c` chains), and a
  latency-bounded SPIFFE Workload API gRPC-over-UDS client (external SPIRE mode). 25 unit
  tests, including every rejection probe of the verification intent (unknown signer, expired,
  not-yet-valid, foreign trust domain, wrong session/role, tampered leaf, no SVID).
- `crates/gyre-server/src/spiffe.rs`: `SpiffeService` (local trust-domain CA mode — persisted
  in the durable KV store (`server_identity`/`spiffe_trust_domain_ca`), operator-provisionable
  via `GYRE_SPIFFE_CA`, mismatched provisioned key/cert is a hard startup error — or external
  Workload API mode via `GYRE_SPIFFE_WORKLOAD_API_SOCKET` + `GYRE_SPIFFE_TRUST_BUNDLE_URL`,
  bundle fetched with a 10 s bound and 5 min cache, fail-closed on non-bundle documents);
  `issue_session_svid` (called only from the authorized spawn paths, gated on the authorized
  compute target + session stack evidence); `verify_workload_identity` / `attest_session` —
  the single fail-closed verification surface (SVID record must exist, SVID must be presented,
  chain + validity + session + serial checks against the published bundle, stack fingerprint
  bound to the SVID-backed record: empty or tampered presentations are rejected); the trust
  bundle endpoint at `/.well-known/spiffe/keys.json` (registered in `lib.rs`, public like the
  OIDC JWKS). `resolve_session_stack_evidence`: the `wl_stack_hash` claim / SVID-record
  binding is the agent's registered M14.1 stack when one exists, else a SHA-256 over the
  server-authorized launch configuration — never a free-form self-report.
- Verification surface wiring — one surface, no bypass variant: `PUT /api/v1/agents/{id}/heartbeat`
  (api/agents.rs; 403 + `agent_heartbeat` Failure audit event with the rejection reason on any
  failed check, agent liveness not advanced), new `POST /api/v1/agents/{id}/attest`
  (api/workload.rs; ABAC-mapped in abac_middleware.rs), MCP `gyre_agent_heartbeat` (mcp.rs;
  `svid` required). `workload_attestation.rs` is now metadata + verification result
  (`spiffe_id`, `svid_serial`, `svid_verified_at`, `alive` = the SVID verification result);
  `pid_is_alive` and `verify_attestation` (the /proc probe and the skip-on-empty compare) are
  deleted. `wl_*` JWT claims preserved (auth.rs) and now bound to the SVID-backed record.
- Workload side: spawn paths inject `GYRE_SPIFFE_ID`, `GYRE_SVID_PEM`, `GYRE_SVID_KEY`, and
  `GYRE_STACK_HASH` into the workload environment (local subprocess and container dispatch,
  gate workloads, Ralph-loop re-spawns); the reference `docker/gyre-agent` entrypoint and
  `agent-runner.mjs` present the SVID on every heartbeat; the `system://protocol` prompt
  section documents the SVID-carrying heartbeat.
- Config/docs: `GYRE_SPIFFE_TRUST_DOMAIN`, `GYRE_SPIFFE_CA`, `GYRE_SPIFFE_SVID_TTL`,
  `GYRE_SPIFFE_WORKLOAD_API_SOCKET`, `GYRE_SPIFFE_TRUST_BUNDLE_URL` (docs/server-config.md);
  heartbeat/attest/trust-bundle endpoints (docs/api-reference.md); workload env vars
  (docs/agent-protocol.md).

### Deferred to the independent verifier

The sandbox memory cap (4 GiB) blocks `cargo test -p gyre-server` (code generation + model
exceeds it); `cargo check -p gyre-server --all-targets` is clean and `cargo test -p
gyre-spiffe` passes (25/25) locally. Required probes for the verifier:

- `cargo test -p gyre-server --lib api::agents::tests` — heartbeat accept path (204 with a
  valid session SVID), rejection paths (no SVID → 403, tampered/empty stack → 403, foreign-CA
  SVID → 403), audit events (`agent_heartbeat` Success with `pid_alive: true` vs Failure with
  reason), liveness not advanced on rejection.
- `cargo test -p gyre-server --lib api::workload::tests` — `POST /agents/{id}/attest` happy
  path (record flips to `alive: true` with `spiffe_id`/`svid_serial`), foreign-trust-domain
  and wrong-session rejections, unknown agent 404, and the trust bundle served at
  `/.well-known/spiffe/keys.json` (JWK Set, `use: "svid"`).
- End-to-end spawn probe: `POST /api/v1/agents/spawn` response carries `svid_pem` +
  `spiffe_id`; the spawned workload's env carries `GYRE_SVID_PEM`/`GYRE_SVID_KEY`/
  `GYRE_STACK_HASH`; a heartbeat presenting exactly those values returns 204.
