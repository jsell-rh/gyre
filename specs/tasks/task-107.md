---
title: "Integrate Sigstore/Fulcio for keyless commit signing"
spec_ref: "identity-security.md §Layer 3: Sigstore/Fulcio"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-107.md
coverage_sections:
  - "identity-security.md §Layer 3: Sigstore/Fulcio"
commits: ["fbee68cb9af60852f671ba898e58ac2a78f9168f", "607cb30f60deb08f8fdd41e8b84220f4443c6d05", "d4a6cc929782a9289829afd43e09522f7a0b1287", "cd5de50983b605acd2c6ea90bed319fd5d5522e7"]
---

## Spec Excerpt

From `identity-security.md` §Layer 3:

> **Keyless commit signing** via Sigstore. An agent with an OIDC identity gets a short-lived signing certificate from Fulcio. Every commit is cryptographically signed — the signature proves which agent, on which task, spawned by which user, made the commit. No long-lived GPG keys to manage or rotate. Signatures recorded in **Rekor transparency log** (public or private instance) for non-repudiation.

Current state: `commit_signatures.rs` uses local Ed25519 signing. The mode can be set to "fulcio" but falls back to local with a warning log. No actual Fulcio certificate issuance, no Rekor transparency log integration.

## Implementation Plan

1. **Fulcio certificate issuance (`gyre-server/src/commit_signatures.rs`):**
   - Implement the Fulcio signing flow:
     a. Agent has an OIDC JWT (from Gyre's OIDC provider)
     b. Request short-lived signing certificate from Fulcio using the JWT
     c. Use the certificate to sign the git commit
     d. Certificate is ephemeral — no key management
   - Use the `sigstore` Rust crate if available, or implement HTTP calls to Fulcio API
   - Configure Fulcio URL via `GYRE_FULCIO_URL` env var (default: public Fulcio instance)

2. **Rekor transparency log:**
   - After signing, record the signature in Rekor for non-repudiation
   - Configure Rekor URL via `GYRE_REKOR_URL` env var
   - Support both public Rekor and private instances
   - Store the Rekor log entry ID alongside the commit signature for later verification

3. **Verification endpoint:**
   - Add verification logic that checks:
     a. The commit signature is valid
     b. The signing certificate was issued by the expected Fulcio instance
     c. The certificate's OIDC subject matches the expected agent identity
     d. A matching Rekor entry exists (non-repudiation)
   - Surface verification status in provenance chain API

4. **Configuration:**
   - `GYRE_SIGNING_MODE`: `local` (default, current behavior), `fulcio` (real Sigstore), `none` (skip signing)
   - `GYRE_FULCIO_URL`: Fulcio instance URL
   - `GYRE_REKOR_URL`: Rekor instance URL
   - Fall back gracefully to local signing if Fulcio is unreachable (with warning)

5. **Testing:**
   - Unit tests with mocked Fulcio/Rekor responses
   - Integration test with local Sigstore stack (if practical)
   - Test fallback behavior when Fulcio is unreachable

## Acceptance Criteria

- [x] Fulcio certificate issuance using agent OIDC JWT
- [x] Commits signed with Fulcio-issued certificate
- [x] Signatures recorded in Rekor transparency log
- [x] Verification endpoint checks certificate chain and Rekor entry
- [x] Configurable via GYRE_SIGNING_MODE, GYRE_FULCIO_URL, GYRE_REKOR_URL
- [x] Graceful fallback to local signing when Fulcio unreachable
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/identity-security.md` §Layer 3. The current signing implementation is in `gyre-server/src/commit_signatures.rs`. The OIDC provider is in `gyre-server/src/oidc.rs` — agents already have JWTs that can be presented to Fulcio. Check if the `sigstore` crate is available in the Rust ecosystem (crates.io). The provenance chain API may be in `gyre-server/src/api/` — grep for `provenance` or `attestation`. Git push processing is in `gyre-server/src/git_http.rs`.

## Shipped

Fulcio keyless commit signing with Rekor transparency-log recording,
implementing every plan item and every review finding from the prior
rounds (F1-F14).

### Behavior

- **Fulcio issuance** (`sigstore.rs::sign_commit_keyless`): ephemeral
  rcgen P-256 key pair per squash, proof-of-possession (base64 DER
  ECDSA over the JWT `sub`) verified by the receiving Fulcio, and
  `publicKey.content` sent as PEM text - the encoding upstream
  Fulcio's `challenges.ParsePublicKey` accepts. The caller's validated
  agent JWT (`AuthenticatedAgent.bearer_token`) is presented to
  Fulcio; the response's `signedCertificateEmbeddedSct`/`DetachedSct`
  chain is decoded (PEM text, raw DER, then base64(DER) - mutually
  unambiguous) and stored leaf-first.
- **Attribution** (F2): `SigningAttribution::from_auth` threads the
  caller's validated JWT claims (`task_id`, `spawned_by`, agent id)
  into the record; non-JWT callers get the resolved agent id with
  empty (not fabricated) task/user fields.
- **Verification** (`verify_signature_fulcio`, exposed at
  `GET /api/v1/repos/:id/commits/:sha/signature/verification` and as
  Phase 6 of the provenance chain verification - F6): four phases with
  trust anchors resolved ONLY from server `SigningConfig` (F3), chain
  validation against EVERY chain in the Fulcio trust bundle (F11) with
  `not_before`/`not_after` enforcement on every stored certificate
  (F5), SAN/CN subject match, and a Rekor check that fetches the entry
  and compares its hashedrekord body's digest, signature, and
  certificate against THIS record (F4 - content match, not existence).
- **Rekor recording**: hashedrekord entry (commit digest, base64 DER
  signature, base64 PEM leaf certificate) POSTed at sign time; the
  returned entry UUID is stored with the record.
- **Persistence** (F9): `CommitSignatureRepository` port with SQLite
  and PostgreSQL adapters (migration `2026-10-09-000056`), keyed by
  `(repo_id, commit_sha)`, wired through `store!` in `build_state`.
- **Latency bound** (F10): every outbound Fulcio/Rekor call is wrapped
  in `tokio::time::timeout(SIGNING_HTTP_TIMEOUT_SECS = 10)`.
- **ABAC** (F8): the verification route is registered in
  `ResourceResolver::routes` (`repo`/read).
- **Configuration**: `GYRE_SIGNING_MODE` (`local`/`fulcio`/`none`),
  `GYRE_FULCIO_URL`, `GYRE_REKOR_URL`, via `SigningConfig::from_env`.
- **Operator key provisioning** (F12 + this round): the forge Ed25519
  key is persisted in the durable KV store across restarts, and
  `GYRE_AGENT_SIGNING_KEY` (base64 PKCS#8 DER, **v1 or v2** -
  `docs/server-config.md` documents the row and a working generation
  command) explicitly provisions/rotates it. Invalid material is a
  hard startup error.
- **Tests** (F7): the mock stack encodes the upstream wire contract
  (rejects base64(DER) keys like real Fulcio, verifies PoP for real,
  serves a two-chain trust bundle, echoes stored Rekor entries) with
  failure knobs per phase; negative coverage for all four verification
  phases; handler-level fallback/attribution/none-mode tests through
  the real router; an independent upstream-conformance probe
  (`tests/task107_upstream_conformance.rs`) drives the production
  sign/verify paths against a Fulcio built from upstream proto
  documentation.

### This round (F13/F14 repair, head `cd5de509`)

- **F13**: the F12 auth.rs insertions had moved the exempted legacy
  `claims.tenant_id` fallback site past the frozen exemption line
  (auth.rs:786), so `scripts/check-fabricated-scope-defaults.sh`
  exited 1 on the candidate head and every Rust-touching commit failed
  pre-commit. The exemption line now tracks the current site
  (auth.rs:838); the entry count stays at the frozen baseline of 5.
- **F14**: `GYRE_AGENT_SIGNING_KEY` documented in
  `docs/server-config.md` next to `GYRE_SIGNING_MODE`. While writing
  the row, an empirical probe against the production loader showed the
  natural generation command (`openssl genpkey -algorithm ed25519
  -outform DER | base64 -w0`) produces PKCS#8 **v1**, which ring's
  strict `Ed25519KeyPair::from_pkcs8` rejects with
  `VersionNotSupported` - every common tool (openssl, Node, Python
  `cryptography`) emits v1. `AgentSigningKey::from_pkcs8` and
  `sign_bytes` now use ring's `from_pkcs8_maybe_unchecked`, the entry
  point ring's own docs direct v1 callers to: v2 parsing is
  byte-for-byte identical to the strict form (including the embedded
  public-key/seed consistency check), and for v1 the public key is
  derived from the operator's own private seed (jsonwebtoken's EdDSA
  signing path uses the same entry point). A permanent regression test
  with a fixed openssl-generated v1 key and precomputed expected kid
  was added to `tests/task107_signing_key_env.rs` (leg 0 of the
  provisioning/rotation test); it fails with
  `VersionNotSupported` against the previous loader.
- The pending merge of base `2f215b1e` (task-180) was resolved: both
  sides' new diesel tables kept, `web/dist` rebuilt from the merged
  `web/src` sources.

### Verification evidence (head `cd5de509`, under /tmp/stage/review-evidence/)

gyre-server lib suite 1273 passed / 0 failed
(`f13-gyre-server-lib-full.txt`), including the sigstore,
commit_signatures, api::jj, api::provenance, and auth suites;
`task107_signing_key_env` 1/1 (`f13-signing-key-env.txt`);
`task107_upstream_conformance` 3/3 (`f13-upstream-conformance.txt`);
adapters commit_signature 5/5
(`f13-adapters-commit-signature.txt`). All 19 mechanical gates pass
(`f13-mechanical-gates.txt`), including fabricated-scope-defaults
(exit 0, was exit 1 on the incoming candidate head) and
task-commit-attribution. Live Fulcio interop remains blocked by
sandbox network policy (fulcio.sigstore.dev:443 connect refused);
exact-head GitHub CI is mandatory after review.
