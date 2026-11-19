---
title: "Sigstore layer-3 completion: every commit signed, keyless by default, no silent fallback, real Rekor for attestation bundles"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / sigstore-layer3-completion / b60edce5bc49"
depends_on: [task-107]
progress: not-started
commits: ["6f3dd98ddc96ba451c8773cede85fff2e8eda19b"]
---

## Required behavior

Narrowly scoped delta on the completed task-107 Sigstore stack (real Fulcio v2 client and Rekor hashedrekord submission/verification already exist in sigstore.rs) — do not replay task-107. Close the remaining Layer-3 shortfalls: (1) 'Every commit is cryptographically signed': extend signing beyond the single jj squash call site to all commit-producing surfaces — jj new, git HTTP push processing, and MR merge-commit creation — each producing a persisted CommitSignature record; GYRE_SIGNING_MODE=none remains only an explicit operator opt-in and must never be the default. (2) Keyless by default: the default SigningMode becomes Fulcio (spec: keyless signing, 'no long-lived GPG keys to manage or rotate'); local forge-key signing remains available only as explicit configuration. (3) No silent degradation: on Fulcio failure the commit path must fail loudly (explicit error or operator-configured behavior surfaced in the response/audit), never silently fall back to local forge-key signing while presenting the commit as signed; non-JWT callers must not present an empty bearer token to Fulcio (today the empty token guarantees failure and fallback) — either resolve a real identity for the caller or refuse keyless signing with an explicit error. (4) The signature proves which agent, on which task, spawned by which user: bind task_id and spawned_by into the signed/verified material — e.g. sign a canonical payload containing commit SHA + task_id + spawned_by and/or carry them in the Fulcio certificate request and Rekor entry — so that verification cryptographically checks them, rather than relying solely on detached plaintext DB columns. (5) Non-repudiation for all signed artifacts: replace the NoopRekorClient wired into merge-attestation bundle storage (rekor.rs, TODO task-144 step 4) with the real Rekor client — merge attestation bundles get real Rekor entries (entry UUID stored) when Rekor is configured, and a Rekor failure must not be reported as successful transparency-log recording.

## Acceptance Criteria

- [ ] With default configuration, commits created through every Gyre commit surface (jj new, jj squash, git HTTP push, MR merge) each have a persisted CommitSignature record, and the existing verification endpoint validates them.
- [ ] Default GYRE_SIGNING_MODE (unset) selects Fulcio keyless signing; local key signing requires explicit configuration.
- [ ] Fulcio unreachable (or returning an error) with keyless mode active: the commit operation fails with an explicit error — no record signed with the local forge key is produced for that operation, and the audit/response identifies the actual signing mode used.
- [ ] Verification of a signature whose task_id or spawned_by attribution does not match the expected values fails; the attribution is covered by the cryptographic signature/Rekor entry, not only by plaintext DB fields.
- [ ] Merge-attestation bundles are submitted to the configured Rekor instance with the stored entry UUID; NoopRekorClient is no longer wired into any production path, and a Rekor outage produces an explicit failure/warning state, not a silent Ok(None).

## Verification intent

Point GYRE_FULCIO_URL at an unreachable endpoint with default (keyless) configuration and drive a commit through any surface: the operation must fail loudly — today jj squash logs 'Fulcio keyless signing failed; falling back to local signing' and produces a local-key record, so a probe asserting 'no local-signed record and no success response' kills the silent-fallback bug. Second probe: take a stored CommitSignature, tamper the task_id attribution (or swap it with another task's), and run verification — it must fail, proving attribution is cryptographically bound rather than a plaintext column a mismatched test could mirror.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/identity-security.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e`.

[
  {
    "id": "68b06ebc09a7c8d49aa516d0",
    "decision": "gap",
    "requirement_quote": "- Every commit is cryptographically signed - the signature proves which agent, on which task, spawned by which user, made the commit.",
    "detail": "A genuine Sigstore stack exists: sigstore.rs implements a real Fulcio v2 REST client (ephemeral ECDSA P-256 keypair, proof-of-possession over the JWT sub claim, HTTP POST to /api/v2/signingCert) and real Rekor hashedrekord submission/verification (HTTP POST to /api/v1/log/entries, content-match verification of digest+signature+cert), with upstream wire-conformance tests. However the spec's requirements are not met in production: (1) 'Every commit is cryptographically signed' is false — the only production signing call site is the jj squash endpoint (POST /api/v1/repos/:id/jj/squash); jj new, git HTTP pushes, and merge commits produce no CommitSignature records, and GYRE_SIGNING_MODE=none skips signing entirely. (2) Keyless signing is opt-in only: the default SigningMode is Local (long-lived persistent forge Ed25519 key), not Fulcio. (3) On Fulcio failure the squash path silently falls back to local forge-key signing, and non-JWT callers present an empty bearer token so Fulcio always fails for them. (4) The signature does not prove task/spawned-by: the cryptographic signature covers only the commit SHA and the Fulcio cert binds only the OIDC sub; task_id and spawned_by are detached plaintext DB fields on the CommitSignature row. (5) Rekor non-repudiation is split: fulcio-mode commit signatures are really submitted to Rekor, but merge-attestation bundles — the other signed artifact — are wired to a NoopRekorClient that only warns and returns Ok(None), an explicit TODO stub.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/commit_signatures.rs",
        "quote": "/// Local Ed25519 signing with the forge key (default).",
        "detail": "SigningMode::from_env defaults to Local for any unrecognized GYRE_SIGNING_MODE value ('_ => Self::Local'). Default deployments therefore sign with the long-lived, persisted, operator-rotatable forge key rather than the specced keyless Fulcio path; keyless is opt-in ('fulcio' => Self::Fulcio)."
      },
      {
        "file": "crates/gyre-server/src/api/jj.rs",
        "quote": "jj squash: Fulcio keyless signing failed; falling back to local signing",
        "detail": "The single production signing surface (jj squash) silently degrades from keyless Fulcio signing to the local forge key on any Fulcio error, and presents auth.bearer_token.clone().unwrap_or_default() (an empty JWT for dev-token/API-key callers), so the 'agent with an OIDC identity gets a short-lived signing certificate from Fulcio' path fails closed to non-keyless signing in practice. grep confirms api/jj.rs is the only production caller of sign_commit_keyless/sign_commit_local."
      },
      {
        "file": "crates/gyre-adapters/src/rekor.rs",
        "quote": "TODO (task-144 plan step 4): real Rekor API integration",
        "detail": "The only RekorClient wired in production is NoopRekorClient ('let rekor: Arc<dyn RekorClient> = Arc::new(gyre_adapters::NoopRekorClient);' in lib.rs), whose submit warns 'Rekor transparency log not configured — merge attestation bundle not submitted for non-repudiation' and returns Ok(None). Merge attestation bundles therefore have no transparency-log non-repudiation at all; only opt-in fulcio-mode commit signatures reach a real Rekor."
      }
    ]
  }
]
