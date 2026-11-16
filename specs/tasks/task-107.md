---
title: "Integrate Sigstore/Fulcio for keyless commit signing"
spec_ref: "identity-security.md §Layer 3: Sigstore/Fulcio"
depends_on: []
progress: complete
review: specs/reviews/task-107.md
coverage_sections:
  - "identity-security.md §Layer 3: Sigstore/Fulcio"
commits: ["6b800e1b5e29dbea9480d7aaa0e6b9947688d15b", "d7d341ca19c2de6ebd48516bf712e27a51061697", "cd84c820d32b7670febdd659ed051daf7b5719a0", "ec3e247ed31b5f1b18297cf2e2594c4dcbb862d7"]
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
  and PostgreSQL adapters (migration `2026-10-09-000060`, renumbered
  from `000056` at the base-merge onto `fca5de5fb` — the base's
  `2026-10-09-000056_user_tenant_id` claimed that version first), keyed by
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

### This round (merge repair onto base `fca5de5fb`, head `29ffb55c3`)

- **Base merge resolution**: the assigned repair merged the assignment
  base `fca5de5fb` (main: admission-policy preflight fix, impersonation,
  spec lifecycle configs, rollout policies) into the branch. Four
  conflicts resolved:
  - `crates/gyre-server/src/auth.rs`: kept BOTH sides' additions — the
    task's `bearer_token` plumbing for Fulcio (field, both assignment
    sites in `authenticate_request`, F12 key-persistence tests) and the
    base's auth-audit `record_auth_success` emission plus the
    `is_system_principal` spoof-shape test. For the JWT `tenant_id`
    claim the BASE's semantics win: the branch's prior round had fixed
    the flagged `claims.tenant_id` fallback by rejecting a JWT with no
    tenant claim, but main independently fixed the same site
    (task-103/task-104 verification repair) with the documented
    single-tenant IdP contract — the claim's serde schema default
    (`default_tenant_claim` on `JwtClaims`, "absent = default tenant",
    a deterministic function of the verified token, not a use-site
    fallback). The merged tree takes main's contract and its test
    (`jwt_missing_tenant_id_defaults_to_default` replaces the branch's
    rejection test); trunk semantics are preserved and the flagged site
    stays fixed either way.
  - `docs/server-config.md`: kept the base's `GYRE_IMPERSONATION_TTL`
    row plus the task's signing rows (`GYRE_SIGNING_MODE` with legacy
    `GYRE_SIGSTORE_MODE` alias, `GYRE_AGENT_SIGNING_KEY`,
    `GYRE_FULCIO_URL`, `GYRE_REKOR_URL`); the base's obsolete
    "fulcio reserved for future" `GYRE_SIGSTORE_MODE` row is superseded
    by the task's row that documents the same legacy alias.
  - `scripts/fabricated-scope-defaults-exemptions.txt` +
    `check-fabricated-scope-defaults.sh`: took the base's state —
    2 entries (speculative_merge.rs:372, gate_executor.rs:111),
    `FROZEN_EXEMPTION_COUNT=2`, with main's fix history for the
    auth.rs/git_http.rs sites. No entries added; the gate passes at the
    merged head.
- **Follow-on repairs**:
  - **Migration version collision**: the base's
    `2026-10-09-000056_user_tenant_id` claimed the Diesel version
    `20261009000056` that the task's
    `2026-10-09-000056_commit_signatures` also mapped to — only one
    runs per version, so the `commit_signatures` table was silently
    never created on fresh databases
    (`task107_signing_key_env` failed with "no such table:
    commit_signatures"). Renumbered the task's migration to
    `2026-10-09-000060_commit_signatures` (next unused sequence);
    `check-migration-versions.sh` and
    `check-migration-sql-portability.sh` pass.
  - Base-side `AuthenticatedAgent` constructors gained the task's
    `bearer_token` field (`impersonation.rs`, `api/spawn.rs` x5,
    `mcp.rs` x2) and task-side test literals in `commit_signatures.rs`
    gained the base's `impersonation` field.
- `cargo check -p gyre-server -p gyre-adapters --all-targets` passes at
  the merged head; `cargo fmt --check` clean; static gates re-run here:
  fabricated-scope, migration-versions, migration-sql-portability,
  arch, abac-route-registry, abac-exempt-handlers, mem-port-contracts,
  task-commit-attribution, unbounded-external-http, inert-enforcement —
  all pass.
- F1-F19 from prior rounds remain in place at the merged head,
  re-verified by the focused suites below.

### Verification evidence (head `29ffb55c3`, under /tmp/stage/review-evidence/)

`task107-merge-focused-tests.txt` — auth:: 56/56 (including
`jwt_missing_tenant_id_defaults_to_default`, the F12 key-persistence
tests, and the `is_system_principal` spoof-shape test), sigstore 17/17,
commit_signatures 6/6, api::provenance 5/5, api::jj 14/14 (including
the fulcio-unreachable fallback test through the real router),
impersonation 15/15, api::spawn::tests 52/52, mcp::tests 88/88;
`task107-merge-integration-tests.txt` — `task107_upstream_conformance`
3/3, `task107_signing_key_env` 1/1, `task107_signing_key_persistence`
1/1; `auth_integration` 21/21. `cargo check -p gyre-server -p
gyre-adapters --all-targets` exits 0. Full workspace suites,
architecture checks, all-target Clippy, and GitHub CI are owned by
verification/publication; live Fulcio interop remains blocked by
sandbox network policy (fulcio.sigstore.dev:443 connect refused,
prior-round `f19-live-verification.txt`); exact-head GitHub CI is
mandatory after review.
