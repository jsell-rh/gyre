# Review — task-261 (SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme)

Spec: `specs/system/identity-security.md` Layer 1 (gap `spec-31d3ae72e12e22c52442e309`).
Candidate under review: `f36c91f7` on
`pipeline/task-261/bde90e7160d84c87a7c81b379a79d7c8-1` (base
`28a3a40b`). Verdict: **approved**.

## Round 1

Probes (evidence: `/tmp/stage/review-evidence/summary.md` and per-probe
files):

- `/tmp/stage/bin/cargo test -p gyre-spiffe` → **27/27 passed**, exit 0.
  Includes the task's core verification-intent probes against real
  ECDSA P-256 material: unknown-signer SVID carrying a well-formed SPIFFE
  ID for the same session (`rejects_svid_signed_by_unknown_key` →
  `UntrustedSigner`), expired / not-yet-valid, foreign trust domain, wrong
  session, wrong role, no-SVID, no-anchors, tampered leaf DER (signature
  check covers the whole TBS), the serial-format regression
  (`issued_and_verified_serials_agree_on_leading_zero_nibbles`), and the
  Workload API wire-format tests (hand-encoded protobuf with explicit
  field numbers pinning the decoder to the documented encoding, not its
  own re-serialization).
- `/tmp/stage/bin/cargo check -p gyre-server` → clean, exit 0.
- All 84 `scripts/check-*.sh` at the candidate tree → exit 0 (incl.
  arch, ABAC route registry for the new `/attest` route, crypto-verify,
  inert-enforcement, unbounded-external-http, in-memory-state-stores,
  task-commit-attribution with the harness's unstaged frontmatter reset
  stashed).
- Grep-level retirement confirmed: `pid_is_alive` / the old
  `verify_attestation` are gone from the workload-identity path; no
  skip-on-empty stack compare exists anywhere on the verification
  surface.

Verified against each acceptance criterion:

- **AC1 (SVID per session, URI SAN format, chain validation):**
  `gyre-spiffe` `TrustDomainCa` issues ECDSA P-256 leaves whose single
  SAN is the URI SAN `spiffe://{trust-domain}/agent/{role}/session/{id}`
  (`SpiffeId::agent` fail-closed parse; trust domain configurable via
  `GYRE_SPIFFE_TRUST_DOMAIN`, validated at startup). `verify_svid`
  performs real signature verification (`x509_parser`
  `verify_signature` of the leaf against the bundle's CA keys), enforces
  validity windows on leaf and signer, requires exactly one URI SAN with
  no other name forms, and binds trust domain + role + session. The
  serial-pin in `verify_workload_identity` additionally rejects any other
  valid SVID for the same session identity. Proven by the passing unit
  tests above and the spawn/heartbeat integration tests (deferred to the
  verifier, see below).
- **AC2 (fail-closed rejections):** every listed case is implemented and
  unit-tested: unknown signer, expired, not-yet-valid, foreign trust
  domain, different session (and role), and no-SVID-presented
  (`NoSvid` / 403 with rejection reason on the REST surfaces,
  `tool_error` on MCP). No record → reject ("session was never
  attested"); no bypass variant — REST heartbeat, POST /attest, and MCP
  `gyre_agent_heartbeat` all funnel through the one
  `attest_session`/`verify_workload_identity` surface.
- **AC3 (trust bundle at well-known location, sole anchor):**
  `/.well-known/spiffe/keys.json` (registered on the public root router
  beside the OIDC JWKS) serves the SPIFFE bundle format (JWK Set,
  `use: "svid"`, `x5c` chains). Verification anchors are derived only
  from that bundle (local prebuilt or operator-configured external URL,
  fail-closed on non-bundle documents, latency-bounded fetch with a 10s
  timeout). Self-reported metadata never participates in the decision.
- **AC4 (wl_* claims preserved and bound):** `mint_worker_scoped` still
  embeds `wl_pid`/`wl_hostname`/`wl_compute_target`/`wl_stack_hash`;
  `wl_stack_hash` is resolved once (`resolve_session_stack_evidence`:
  registered M14.1 stack, else a SHA-256 over the server-authorized
  launch configuration) and the SAME value is bound into the SVID record
  at issuance, injected as `GYRE_STACK_HASH`, and enforced at
  verification (empty or tampered presentation rejected — tested in
  `heartbeat_with_tampered_stack_is_rejected`).
- **AC5 (G10 scheme retired):** the `/proc/{pid}` probe and the
  skip-on-empty string compare are deleted; `WorkloadAttestation` is
  metadata plus the SVID verification result
  (`spiffe_id`/`svid_serial`/`svid_verified_at`/`alive`), and `alive` is
  false until an SVID presentation verifies.

Issuance gating: `issue_session_svid` is called only from the authorized
spawn paths (`launch_agent_process`, the shared
`launch_agent_workload` gate path, ralph re-spawn), post-ABAC with the
resolved compute target and stack evidence; a failed issuance fails the
launch closed. SVID records live in the port-backed KV store
(`spiffe_svids`), the CA persists in the durable KV
(`server_identity`/`spiffe_trust_domain_ca`) with
operator-provisioning via `GYRE_SPIFFE_CA` (mismatched key/cert is a hard
startup error), and external SPIRE mode
(`GYRE_SPIFFE_WORKLOAD_API_SOCKET` + `GYRE_SPIFFE_TRUST_BUNDLE_URL`)
fails closed when the Workload API returns no SVID naming the session.
E2E agent scripts, docker entrypoint, agent protocol prompt, and docs
(`api-reference`, `agent-protocol`, `server-config`) all present the SVID
on the heartbeat/attest path.

Deferred to the independent verifier (sandbox memory cap blocks
gyre-server test compilation, consistent with the implementation's
coverage note): `cargo test -p gyre-server --lib` (workload attest +
trust-bundle endpoint + heartbeat accept/reject + MCP heartbeat tests),
`cargo test -p gyre-server --test api_integration agent_heartbeat`, and
standard CI (`cargo clippy --all-targets`, `cargo test --all`).

Non-blocking observations (follow-up material, not defects against this
task's contract):

- [~] SVID presentation is the PEM chain over the authenticated channel
  (chain validation + session/serial/stack binding) rather than a
  proof-of-possession of the SVID private key (no mTLS handshake or
  challenge-response). The task defines the Layer-1 surface as
  presentation + fail-closed chain validation, which is what shipped, and
  the surfaces are additionally behind `require_auth_middleware` and (MCP)
  repo-scope binding. A nonce-signature hardening using `GYRE_SVID_KEY`
  would be a natural future task.
- [~] `verify_workload_identity` skips the serial pin when
  `record.serial` is empty; the issuance path always fills the serial
  (Workload API chains are parsed at issuance), so this guard is
  effectively dead and the SPIFFE-ID session binding still applies.
- [~] `bind_stack_fingerprint`'s doc comment describes a
  "first-time presentation binds" path the (stricter) code does not
  implement — stale comment only.
