---
title: "Integrate Sigstore/Fulcio for keyless commit signing"
spec_ref: "identity-security.md §Layer 3: Sigstore/Fulcio"
depends_on: []
progress: needs-revision
review: specs/reviews/task-107.md
coverage_sections:
  - "identity-security.md §Layer 3: Sigstore/Fulcio"
commits: ["6b885f810304c33e21a7b6573b1c0cd71d097fdd", "c81906aa5c026d4d57f660484f71021704685d57", "4cab5001ebfe40bf2807fd8ff988ef7a2cc53e54", "60dff4ff9fb6a4cac105bbe891a46aec63cd1794", "ce98b0df3596d97db3ff5fceb572dccd31debad4", "188c996662af5a6d5c814305efafc67441e13ab4"]
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
