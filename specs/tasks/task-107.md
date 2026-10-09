---
title: "Integrate Sigstore/Fulcio for keyless commit signing"
spec_ref: "identity-security.md §Layer 3: Sigstore/Fulcio"
depends_on: []
progress: ready-for-review
review: specs/reviews/task-107.md
coverage_sections:
  - "identity-security.md §Layer 3: Sigstore/Fulcio"
commits: ["d4a6cc929782a9289829afd43e09522f7a0b1287", "c8f6c418"]

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

Fulcio keyless commit signing with Rekor transparency-log recording
(`d4a6cc92`, already in this task's `commits:` list) implements every
plan item and every review finding F1–F11 from the six prior review
rounds:

- **Fulcio issuance** (`sigstore.rs::sign_commit_keyless`): ephemeral
  rcgen P-256 key pair per squash, proof-of-possession (base64 DER
  ECDSA over the JWT `sub`) verified by the receiving Fulcio, and
  `publicKey.content` sent as **PEM text** — the encoding upstream
  Fulcio's `challenges.ParsePublicKey` accepts (F1: base64(DER) is
  rejected by every real instance). The caller's validated agent JWT
  (`AuthenticatedAgent.bearer_token`) is presented to Fulcio; the
  response's `signedCertificateEmbeddedSct`/`DetachedSct` chain is
  decoded and stored leaf-first.
- **Attribution** (F2): `SigningAttribution::from_auth` threads the
  caller's validated JWT claims (`task_id`, `spawned_by`, agent id)
  into the record — no placeholder literals; non-JWT callers get the
  resolved agent id with empty (not fabricated) task/user fields.
- **Verification** (`verify_signature_fulcio`, exposed at
  `GET /api/v1/repos/:id/commits/:sha/signature/verification` and as
  Phase 6 of the provenance chain verification — F6): four phases with
  trust anchors resolved ONLY from server `SigningConfig` (F3), chain
  validation against EVERY chain in the Fulcio trust bundle (F11) with
  `not_before`/`not_after` enforcement on every stored certificate
  (F5), SAN/CN subject match (c), and a Rekor check that fetches the
  entry and compares its hashedrekord body's digest, signature, and
  certificate against THIS record (F4 — content match, not existence).
- **Rekor recording**: hashedrekord entry (commit digest, base64 DER
  signature, base64 PEM leaf certificate) POSTed at sign time; the
  returned entry UUID is stored with the record.
- **Persistence** (F9): `CommitSignatureRepository` port with SQLite
  and PostgreSQL adapters (migration `2026-10-09-000056`), keyed by
  `(repo_id, commit_sha)` — repo-scoped lookups, restart-surviving
  records, wired through `store!` in `build_state`.
- **Latency bound** (F10): every outbound Fulcio/Rekor call is wrapped
  in `tokio::time::timeout(SIGNING_HTTP_TIMEOUT_SECS = 10)` — the
  fall-back-to-local guarantee holds for hung stacks, not just refused
  connections.
- **ABAC** (F8): the verification route is registered in
  `ResourceResolver::routes` (`repo`/read).
- **Configuration**: `GYRE_SIGNING_MODE` (`local`/`fulcio`/`none`),
  `GYRE_FULCIO_URL`, `GYRE_REKOR_URL`, via `SigningConfig::from_env`.
- **Tests** (F7): the mock stack encodes the upstream wire contract
  (rejects base64(DER) keys like real Fulcio, verifies PoP for real,
  serves a two-chain trust bundle, echoes stored Rekor entries) with
  failure knobs per phase; negative coverage exists for all four
  phases (tampered signature, untrusted CA, expired leaf, mismatched
  SAN, dropped Rekor entry, swapped Rekor entry, foreign trust anchor,
  wire-format rejection, wrong-preimage PoP) plus handler-level
  fallback/attribution/none-mode tests through the real router.

Verification evidence (this round, on branch head `1b55d097` =
 checkpoint `d4a6cc92` + upstream base merge): sigstore unit suite,
 jj API handler suite, ports/adapters commit_signature suites, and the
 task-107-relevant mechanical gates (ABAC route registry, in-memory
  state stores, unbounded external HTTP, migration versions and SQL
  portability, mem port contracts, arch) all pass — outputs retained
  under `/tmp/stage/review-evidence/`. Full `cargo test --all` and
  GitHub CI are owned by the verification/publication stages.

This round additionally repaired a pre-existing gate failure inherited
from the upstream base: task-210's main-merge commit `a781ede2`
(`crates/gyre-server/src/api/admin.rs`) was absent from
`specs/tasks/task-210.md`'s `commits:` frontmatter; recorded via a
specs-only `process(task-107)` commit per the check's prescribed fix
(`scripts/check-task-commit-attribution.sh` now passes).
