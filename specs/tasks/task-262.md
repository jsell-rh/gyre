---
title: "SPIFFE federation: trust-bundle exchange between Gyre instances"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / spiffe-federation / b60edce5bc49"
depends_on: [task-261]
progress: complete
commits: ["14773a01f43c4f161630c021523a53a3051251f2"]
---

## Required behavior

Close the one unimplemented bullet of the Federation section: 'SPIFFE federation handles the workload attestation layer independently.' The existing OIDC-issuer federation (GYRE_TRUSTED_ISSUERS, validate_federated_jwt with remote JWKS discovery, SSRF same-origin guard, 5-min cache) stays as-is. Add SPIFFE trust-bundle exchange: a Gyre instance B can register/fetch instance A's SPIFFE trust bundle (per the SPIFFE federation spec — bundle endpoints keyed by trust domain, refresh on interval), and B's workload-identity verification accepts SVIDs from A's trust domain using A's exchanged bundle while still rejecting SVIDs from trust domains whose bundles were never exchanged. Workload-attestation federation is independent of the OIDC layer: a federated agent's Layer-2 JWT is validated via OIDC discovery, and its Layer-1 SVID is validated via the SPIFFE bundle — neither substitutes for the other, and no static credentials (shared keys/secrets) are exchanged between instances, only issuer URLs and public trust bundles.

## Acceptance Criteria

- [ ] Instance B configured with A as a federation peer obtains A's SPIFFE trust bundle (fetch or registration) and verifies an A-trust-domain SVID presented by a federated agent, extracting the same SPIFFE ID A's verifiers would.
- [ ] An SVID from a trust domain whose bundle was never exchanged with B is rejected by B's workload-identity verification, even when the caller's OIDC JWT from that instance's issuer is otherwise trusted.
- [ ] Bundle refresh/rotation works: after A rotates its bundle, B re-fetches within the configured interval and verification follows the refreshed bundle (old-bundle-only verification of a rotated SVID fails closed rather than silently succeeding).
- [ ] No static credential material is exchanged: federation configuration consists of issuer/trust-domain endpoints, with all key material fetched via the public bundle/JWKS discovery endpoints.

## Verification intent

Authenticate a federated agent from instance A at instance B in two configurations: (a) B trusts A's OIDC issuer but has NOT exchanged A's SPIFFE bundle — the agent authenticates (Layer 2) but its workload attestation must NOT verify or be reported as attested; (b) B exchanges the bundle — the same SVID verifies. A probe that passes in both configurations (or fails in both) proves the workload layer is not independently federated; today nothing even distinguishes the two cases, so the probe cannot pass pre-task.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/identity-security.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e`.

[
  {
    "id": "05e06b05e91abddea8707d41",
    "decision": "gap",
    "requirement_quote": "- SPIFFE federation handles the workload attestation layer independently.",
    "detail": "Three of four bullets are genuinely satisfied: two Gyre instances federate by trusting each other's OIDC issuers — GYRE_TRUSTED_ISSUERS config, validate_federated_jwt performs remote OIDC discovery (GET {issuer}/.well-known/openid-configuration -> jwks_uri -> JWK set) with an SSRF same-origin guard on the jwks_uri and a 5-minute JWKS cache, wired into both the auth extractor and the global middleware; an agent from instance A authenticates to B by presenting its JWT; and no static credentials are exchanged (only issuer URLs configured, keys fetched via discovery). The remaining bullet — 'SPIFFE federation handles the workload attestation layer independently' — has no implementation: there is no SPIFFE federation machinery anywhere (grep for SPIFFE trust bundles matches only the Sigstore Fulcio trust bundle), and federated agents are authenticated purely by JWT, with the wl_* claims in a federated token being whatever the remote instance minted and no cross-instance workload-attestation verification.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/auth.rs",
        "quote": "async fn validate_federated_jwt(token: &str, state: &Arc<AppState>) -> Option<AuthenticatedAgent> {",
        "detail": "Real cross-instance agent-JWT federation: validates EdDSA JWTs whose iss matches a configured trusted issuer, fetching the remote instance's JWKS via OIDC discovery (satisfying 'B verifies against A's OIDC discovery endpoint' and 'no static credentials exchanged')."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "// Check federation JWT from trusted remote Gyre instances (G11).",
        "detail": "Federation validation is wired into the production auth middleware path, confirming the OIDC-issuer federation bullets are live. In contrast, no analogous workload-attestation federation exists: nothing verifies a federated agent's workload attestation, and the only trust-bundle code in the repo is the Sigstore Fulcio bundle in sigstore.rs."
      },
      {
        "file": "crates/gyre-server/src/workload_attestation.rs",
        "quote": "//! G10: Local workload attestation — pragmatic SPIFFE alternative.",
        "detail": "The workload-attestation layer that this bullet says 'SPIFFE federation handles ... independently' is a local per-instance record with no federation capability of any kind — there is nothing to federate and no federation consumer."
      }
    ]
  }
]

## Shipped

SPIFFE trust-bundle exchange between Gyre instances: instance B, configured
with instance A as a federation peer, fetches A's SPIFFE trust bundle from
A's public bundle endpoint (keyed by trust domain, refreshed on interval)
and verifies federated agents' A-trust-domain SVIDs against it — while
rejecting SVIDs from trust domains whose bundles were never exchanged, even
when the caller's OIDC JWT from that instance's issuer is trusted. The
OIDC-issuer federation (GYRE_TRUSTED_ISSUERS, validate_federated_jwt) is
untouched; the two layers are independent by construction.

### Behavior

- `gyre-spiffe/src/federation.rs` (new): the federation protocol semantics.
  `ExchangedBundles` is the trust-bundle registry keyed by trust domain with
  a refresh interval; `anchors_for` fails closed on a never-exchanged domain
  and on a refresh that cannot be completed once the interval expired
  (verification follows the *refreshed* bundle, never a silently-stale one).
  `verify_federated_svid` extracts the SVID's claimed trust domain
  (`gyre_spiffe::extract_spiffe_id`, new peek helper), resolves that
  domain's exchanged bundle, and runs the full chain verification
  (`verify_svid`: signature against the exchanged anchors, validity
  windows, single URI SAN) binding the session to the caller's JWT `sub`.
  `BundleSource` is the host fetch transport (mocked in crate tests).
- `gyre-server/src/spiffe_federation.rs` (new): `GYRE_SPIFFE_FEDERATION_PEERS`
  (`trust_domain=endpoint_url`; a bare origin gets the standard
  `/.well-known/spiffe/keys.json` path appended; local-trust-domain and
  non-http(s) entries are rejected) and
  `GYRE_SPIFFE_FEDERATION_REFRESH_SECS` (default 300 s); `HttpBundleSource`
  fetches over a 10 s-bounded reqwest client and validates the document with
  `parse_trust_bundle` (garbage never becomes an anchor);
  `attest_federated_session` performs the verification and upserts the
  caller's workload-attestation record (a failed re-verification flips an
  existing record to `alive=false` — a previously attested federated agent
  is never left reported as attested).
- `api/federation.rs`: `POST /api/v1/federation/spiffe/attest` (federated
  callers only — local agents keep using the local
  `POST /api/v1/agents/{id}/attest` surface) and
  `GET /api/v1/federation/spiffe/workload` (the caller's own record, 404
  before a first verifying presentation). Both routes are ABAC-mapped
  (resource `federation`) — no exemptions.
- No static credential material is exchanged: federation configuration
  consists of issuer/trust-domain endpoints only; all key material is
  fetched via the public bundle/JWKS discovery endpoints.

### Test evidence

- Locally executed (this sandbox): `cargo test -p gyre-spiffe` — 36 passed,
  0 failed. The 8 new federation tests cover the two-configuration probe at
  the protocol level (same SVID: rejected "not federated" without the
  exchange, verified with it — same SPIFFE ID and serial A's own verifiers
  extract), never-exchanged third domain rejected alongside a federated
  peer, rogue signer, cross-session replay, no-SVID, bundle rotation
  (rotated SVID fails closed against the old bundle within the interval,
  verifies after the refresh, old-CA SVID then stops verifying), expired
  refresh failure fails closed, withdrawn bundle fails closed.
  `cargo clippy -p gyre-spiffe --all-targets`: 0 warnings. All mechanical
  gate scripts pass (arch, ABAC route registry, unbounded HTTP,
  in-memory stores, inert enforcement, etc.).
- Deferred to the independent verifier (the sandbox build wrapper blocks
  gyre-server code generation — infrastructure restriction recorded in
  /tmp/stage/review-evidence/README.md; `cargo check -p gyre-server --tests`
  is clean): `cargo test -p gyre-server --lib spiffe_federation::` and
  `cargo test -p gyre-server --lib api::federation::tests`. These exercise
  the server wiring over real HTTP from a local listener, including the
  verification-intent probe end to end: a federated agent from instance A
  authenticates at B via real OIDC discovery + JWKS in BOTH configurations
  (Layer 2 works), and its SVID verifies only when the bundle is exchanged
  (Layer 1 independent), plus HTTP-level rotation and record lifecycle.

Coverage row 7 (Federation) in specs/coverage/system/identity-security.md
updated to `implemented` (task-262) with source evidence; acceptance
criteria checkboxes are left untouched for the reviewer.
