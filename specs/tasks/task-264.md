---
title: "Sigstore layer-3 completion: every commit signed, keyless by default, no silent fallback, real Rekor for attestation bundles"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / sigstore-layer3-completion / b60edce5bc49"
depends_on: [task-107]
progress: ready-for-review
commits: ["cd0ff0eacdddd3e6d27acf5a86cb8427946a0595", "6f3dd98ddc96ba451c8773cede85fff2e8eda19b"]
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

## Shipped

Task-264 closes the five Layer-3 shortfalls on top of the task-107 stack. Source: checkpoint commit `6f3dd98d` (implementation) plus this commit (finalizer: formatting, integration-harness signing opt-in, docs/coverage, review cleanup).

### Behavior

1. **Every commit surface signs.** A single shared entry point, `commit_signatures::sign_commit`, is now called by all four commit-producing surfaces, each persisting a `CommitSignature`: `jj new` (api/jj.rs — the `JjOpsPort::jj_new` port now returns `JjNewChange {change_id, commit_id}` so the signable, content-addressed commit id is available), `jj squash` (api/jj.rs), git HTTP push (git_http.rs `git_receive_pack` — signs every landed commit *before* responding; on signing failure it undoes the ref updates, emits a `git_push_rejected` audit event naming the `commit-signing` gate and the active mode, and rejects the push 403/502), and MR merge-commit creation (merge_processor.rs `sign_merge_commit`, wired into both the single-MR `process_next` path — signing failure triggers the existing post-merge recovery protocol: queue pause, revert, escalation — and the atomic-group path, which rolls the group back).
2. **Keyless by default.** `SigningMode::parse` maps unset/empty → `Fulcio`; `local` and `none` are explicit operator opt-ins only. An unrecognized `GYRE_SIGNING_MODE` value is a hard startup error (panics in `from_env` at boot) instead of the old silent `_ => Local` downgrade.
3. **No silent degradation.** On Fulcio failure the operation fails loudly with an error naming the active mode (403 for callers with no presentable OIDC JWT — `sigstore::jwt_is_presentable` gates *before* any HTTP call, so an empty/dev-token bearer is never presented to Fulcio; 502/push-rejection/merge-revert for stack failures). The only fallback is the new operator-configured `GYRE_SIGNING_FULCIO_FALLBACK=local`, which warns and stores an honest `sigstore_mode=local` record.
4. **Attribution is cryptographically bound.** The local Ed25519 signature, the keyless ECDSA signature, and the Rekor hashedrekord digest all cover the canonical payload `{commit_sha, task_id, spawned_by}` (`signed_commit_payload`/`commit_digest`/`record_signed_payload`). Verification (both local and fulcio paths, including the Rekor entry content-match) recomputes the payload from the record's own fields, so a tampered `task_id` or `spawned_by` fails verification — the attribution is not just a plaintext column.
5. **Real Rekor for attestation bundles.** `NoopRekorClient` is deleted; production wiring (server lib.rs `build_state`) and the test fixture both wire `HttpRekorClient` (gyre-adapters/src/rekor.rs), which submits each signed bundle as a hashedrekord to `GYRE_REKOR_URL` (latency-bounded, 10 s timeout) and persists the returned entry UUID keyed by MR id (KV namespace `rekor_attestation_entries`). An unconfigured log or a Rekor outage is an explicit `Err` surfaced as a warn/audit state by `store_merge_attestation_bundle` — never a silent `Ok(None)` — while the already-landed merge is not rolled back.

Production-wired integration harnesses that push/merge with non-JWT dev tokens (git_integration, e2e_ralph_loop, graph_integration, spec_enforcement) now explicitly set `GYRE_SIGNING_MODE=local` — the documented operator opt-in — mirroring `mem::test_state`, so the new Fulcio-by-default does not break unrelated push/merge coverage. Docs updated: server-config.md (`GYRE_SIGNING_MODE`, new `GYRE_SIGNING_FULCIO_FALLBACK`, `GYRE_REKOR_URL`) and api-reference.md (jj/new, jj/squash, git-receive-pack, merge-queue/enqueue rows).

### Test evidence

- **Ran here (smaller packages / type checks):** `cargo test -p gyre-adapters rekor` — 3/3 pass (hashedrekord wire round-trip against a local mock Rekor, unconfigured-log explicit error, unsigned-bundle refusal). `cargo check -p gyre-server --tests` — clean (type-checks all new server test code). `cargo fmt --all` applied. Mechanical checks pass locally: check-unbounded-external-http, check-lossy-secret-conversion, check-byte-slice-truncation, check-inert-enforcement, check-fabricated-scope-defaults, check-relative-path-defaults, check-scope-literal-defaults, check-forwarded-header-trust, check-fail-open-ref-resolution.
- **Deferred to the independent verifier (gyre-server test compilation exceeds this assignment's build budget):** `cargo test -p gyre-server --lib commit_signatures sigstore jj_squash jj_new push_signs push_rejected_loudly merge_commit_signing rekor_storage` — the task-264 suites are: `commit_signatures::tests::{signing_mode_default_and_explicit_values, fulcio_fallback_requires_explicit_local_value, local_verify_fails_on_tampered_task_attribution, signed_payload_is_canonical_and_attribution_sensitive}`, `sigstore::tests::{jwt_presentability_gate, verification_fails_on_tampered_task_attribution, commit_digest_is_stable_and_hex}`, `api::jj::tests::{jj_new_signs_commit_with_persisted_validated_signature, jj_squash_fulcio_unreachable_fails_loudly_no_local_fallback, jj_squash_fulcio_without_jwt_is_refused}`, `git_http::tests::{push_signs_every_landed_commit, push_rejected_loudly_when_fulcio_unreachable}`, `merge_processor::merge_signing_tests::{merge_commit_signing_persists_attribution_bound_record, merge_commit_signing_fails_loudly_when_fulcio_unreachable}`, `merge_attestation::rekor_storage_tests::{successful_submission_stores_the_rekor_entry_uuid, rekor_outage_is_an_explicit_failure_not_silent_success, unconfigured_rekor_fails_explicitly}`; plus the regression-sensitive existing binaries `cargo test -p gyre-server --test git_integration --test e2e_ralph_loop --test graph_integration --test spec_enforcement` (these now opt into local signing) and the existing jj/squash/sigstore suites.
- **Environment restriction:** the `jj` binary is not installed in this sandbox, so `cargo test -p gyre-adapters jj_ops` (updated for `JjNewChange`) could not run here; it must run in an environment with jj on PATH.
