---
title: "SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / spiffe-workload-attestation / b60edce5bc49"
depends_on: []
progress: complete
commits: ["aa32b0fa192eb647fe5fd84c0880b9a7c55c6680", "d9d97d4ee3f6d023997c3df053f89c0073484f6b", "2934153b307ad6f2cca32db758e134a375b8de90", "0c37ba3c1972849406c4c233579c4c2cd9ef8230", "14c63ffbd16a48b386fde1c63b18ff1af1e1e464", "ea0f207339d7f231560338d0e64b62302231de8a", "354a5e7a653431c98784ef3cb8326697b7998909", "c9a4d0337ef3aac273be09561d7548f82b2b51b8"]
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

## Shipped

Layer 1 of the identity stack is implemented with the specced existing
protocol: X.509-SVIDs per the SPIFFE X.509-SVID specification, replacing
the G10 local scheme (the `/proc/{pid}` probe and skip-on-empty stack
compare are deleted from the verification path).

### Implementation

- `crates/gyre-spiffe` (c9a4d033): `SpiffeId` (fail-closed parse/build of
  `spiffe://{trust-domain}/agent/{role}/session/{id}`), `TrustDomainCa`
  (ECDSA P-256 via rcgen; leaf carries the SPIFFE ID as its single URI
  SAN; serializable for operator provisioning/backup), `verify_svid`
  (chain validation via x509-parser `verify_signature` against the trust
  bundle's anchors, validity windows on leaf and signer, exactly-one
  URI SAN extraction, session binding), trust-bundle encode/parse
  (SPIFFE bundle format: JWK Set, `use: "svid"`, `x5c` chains), and a
  latency-bounded SPIFFE Workload API gRPC-over-UDS client (external
  SPIRE mode).
- `crates/gyre-server/src/spiffe.rs` (354a5e7a): `SpiffeService` (local
  trust-domain CA mode persisted in the durable KV store, or external
  Workload API mode anchored by `GYRE_SPIFFE_TRUST_BUNDLE_URL`,
  fail-closed on non-bundle documents); `issue_session_svid` (called
  only from the authorized spawn paths, gated on spawn-time
  authorization evidence); `verify_workload_identity` /
  `attest_session` — the single fail-closed verification surface (SVID
  record must exist, SVID must be presented, chain + validity + session
  + serial checks against the published bundle, stack fingerprint bound
  to the SVID-backed record); trust bundle served at
  `/.well-known/spiffe/keys.json`. `wl_*` JWT claims preserved and bound
  to the SVID-backed record. Verification wiring: REST heartbeat +
  `POST /agents/{id}/attest` + MCP `gyre_agent_heartbeat` (one surface,
  no bypass variant); spawn paths inject `GYRE_SVID_PEM`/`GYRE_SVID_KEY`/
  `GYRE_SPIFFE_ID`/`GYRE_STACK_HASH` into the workload environment.

### Review repair round 1 (0c37ba3c, finding ca00525f)

- Serial-format mismatch (intermittent rejection of genuine SVIDs):
  issuance, verification, and Workload API leaf parse now all format
  serials through `gyre_spiffe::canonical_serial_hex`; deterministic
  worst-case regression test forces a top byte of `0x01`.
- `mcp_scope_worker_cannot_heartbeat_foreign_agent` updated to seed the
  worker's SVID-backed record and present the issued chain on the
  self-heartbeat leg (foreign-agent scope denial unchanged).
- `GYRE_SPIFFE_CA` doc now shows the actual `SerializedCa` JSON shape;
  duplicated clippy allow removed.

### Review repair round 2 (d9d97d4e, finding a619ffb4)

- `crates/gyre-server/tests/api_integration.rs` `agent_heartbeat` was
  broken deterministically by the now-mandatory SVID presentation (agent
  created via plain POST, no spawn, empty heartbeat body → 403 before
  liveness is recorded). The test now seeds the agent's SVID-backed
  record via `gyre_server::spiffe::issue_session_svid` and presents the
  issued chain PEM plus the bound stack fingerprint, exactly as a
  spawned workload does via `GYRE_SVID_PEM`/`GYRE_STACK_HASH`. Same
  repair shape as the earlier mcp.rs fix; assertion strength unchanged.
- `scripts/e2e-agent.sh` and `scripts/e2e-agent-claude.sh` heartbeat
  curls now present the spawn-injected `GYRE_SVID_PEM`/`GYRE_STACK_HASH`
  (JSON body built with jq/node for the multi-line PEM, same pattern as
  `docker/gyre-agent/entrypoint.sh`), so agents driven by these dev
  helpers no longer silently fail every heartbeat.

### Verification repair round 3 (aa32b0fa, finding 3ee8be56)

- Verification run (checks.sh, exit 1) failed on exactly one finding:
  `crates/gyre-server/src/api/workload.rs:367: clippy::len_zero` (length
  comparison to one) in `trust_bundle_served_at_well_known_location` —
  every other gate in that run passed (rustfmt, arch lints, ABAC,
  MCP write tools, migration checks, all script guards, web build).
- Fix: `.len() >= 1` replaced with `!is_empty()` — clippy's own
  suggested replacement; assertion strength and behavior identical.
  rustfmt clean on the file; no other changed-line len()-comparison
  exists on the branch (the two remaining repo matches pre-date the
  base commit and are outside the changed-lines filter).
- Sandbox note: gyre-server cold clippy belongs to the independent
  verifier (4 GiB budget); evidence logged at
  `/tmp/stage/review-evidence/task-261-clippy-len-zero-repair.log`.

### Rebase repair round (b8b023fb)

- Rebased the branch onto base 05024b3e (task-254 agent signing key
  rotation). Single conflict in
  `crates/gyre-server/src/middleware.rs`: the candidate branch added
  `spiffe: base.spiffe.clone()` to the test AppState literal, the base
  added `signing_key_history: Vec::new()` to the same literal; AppState
  carries both fields, so the resolution keeps both lines. No behavior
  change; all other files auto-merged.
- Post-rebase probes (sandbox limits: gyre-server test compilation
  belongs to the independent verifier): `cargo check -p gyre-spiffe -p
  gyre-server` passes; `cargo test -p gyre-spiffe` passes 27/27
  including the full fail-closed rejection matrix (unknown key,
  expired, not-yet-valid, foreign trust domain, wrong session, wrong
  role, tampered leaf, no SVID, empty bundle); `rustfmt --edition 2021
  --check` clean on every .rs file changed vs base. Logs:
  /tmp/stage/review-evidence/task-261-spiffe-tests.log,
  task-261-rebase-verify.log.

### Test evidence

- Prior rounds (reviewer-run evidence, durable findings ca00525f and
  a619ffb4): gyre-spiffe serial-flakiness measured 0 FAILED / 20 runs
  after 0c37ba3c; the mcp.rs heartbeat test regression was confirmed
  fixed by the a619ffb4 review (it re-flagged only the api_integration
  and e2e-script siblings).
- Verifier must run: `cargo test -p gyre-server --test api_integration`
  (in particular `agent_heartbeat`), `cargo test -p gyre-server --lib`
  (heartbeat/SVID tests, including
  `mcp_scope_worker_cannot_heartbeat_foreign_agent`), and
  `cargo test -p gyre-spiffe`.
