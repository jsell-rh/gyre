---
title: "Integrate Sigstore/Fulcio for keyless commit signing"
spec_ref: "identity-security.md §Layer 3: Sigstore/Fulcio"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-107.md
coverage_sections:
  - "identity-security.md §Layer 3: Sigstore/Fulcio"
commits: ["cd84c820d32b7670febdd659ed051daf7b5719a0", "ec3e247ed31b5f1b18297cf2e2594c4dcbb862d7", "d7d341ca19c2de6ebd48516bf712e27a51061697"]
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
rounds (F1-F19).

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

### This round (F19 repair + base merge, head `d7d341ca`)

- **F19** (blocking, trust-bundle wire format): upstream Fulcio serves
  the protojson of `message TrustBundle { repeated CertificateChain
  chains = 1; }` — `{"chains": [{"certificates": ["<PEM>", ...]}]}`,
  each chain an OBJECT with a `certificates` array (confirmed against
  fulcio.proto, grpc_server.go `GetTrustBundle`, and sigstore-python's
  REST client). The prior `chains: Vec<Vec<String>>` decode failed on
  every real instance, so `verify_signature_fulcio` returned early with
  `fulcio trust bundle response invalid` for EVERY fulcio-mode record —
  the verification endpoint and provenance Phase 6 reported every
  legitimate commit as FAILED. Fixed with `TrustBundleChainWire`, an
  untagged enum that accepts the upstream object shape and tolerates the
  legacy bare-array shape, normalized in `fetch_trust_bundle`; a chain
  that is neither shape is rejected (the decode genuinely gates).
- **Mock/probe shape correction** (the F1/F7 self-confirming-mock class):
  `MockSigningStack::trust_bundle_json`, the hand-built F11 multi-chain
  test bundle, and the independent upstream-conformance probe
  (`UpstreamFulcio::trust_bundle_json`) now serve the upstream object
  shape; the probe's shape guard asserts the object form itself so it
  cannot silently drift back to bare arrays.
- **New test** `verification_accepts_upstream_trust_bundle_object_shape`:
  upstream object shape verifies end-to-end (`valid=true`), the legacy
  bare-array shape stays tolerated, and a string chain (neither shape)
  is rejected by `fetch_trust_bundle`.
- **Base merge**: merged the assigned base `2115ae23` (task-064 view-query
  grammar + verifier-script-overlay main) into the task branch; the only
  conflicts were regenerated `web/dist` build artifacts, resolved to the
  base's bundles to match its `web/src` (the task branch made no frontend
  source changes).
- F12-F17 from prior rounds (key persistence, `GYRE_AGENT_SIGNING_KEY`
  docs, rogue-leaf/chain binding, fmt/clippy gate repairs, warn-continue
  exemption line) remain in place and re-verified this round.

### Verification evidence (head `d7d341ca`, under /tmp/stage/review-evidence/)

Focused suites: sigstore 17/17 (including the new F19 shape test),
`task107_upstream_conformance` 3/3 — `production_verifies_against_
upstream_pem_bundle` now drives production `verify_signature_fulcio`
against a bundle encoded exactly as upstream serves it and passes —
commit_signatures 6/6, api::provenance 5/5,
`task107_signing_key_env` 1/1. Mechanical gates at the new head
against the base `2115ae23`: `check-rustfmt-diff.py` exit 0
(`f19-rustfmt-gate.txt`), `check-clippy-diff.py` exit 0 — 26 Rust files,
1127 existing warnings outside changes (`f19-clippy-gate.txt`) — and
`check-arch.sh`, `check-inert-enforcement.sh`,
`check-fabricated-scope-defaults.sh`, `check-warn-continue-creation.sh`,
`check-mem-port-contracts.sh`, `check-dead-message-kinds.sh`, and
`check-task-commit-attribution.sh` all exit 0
(`f19-gate-*.txt`). Live Fulcio interop remains blocked by sandbox
network policy (fulcio.sigstore.dev:443 connect refused,
`f19-live-verification.txt`); exact-head GitHub CI is mandatory after
review.
