---
title: "Sigstore layer-3 completion: every commit signed, keyless by default, no silent fallback, real Rekor for attestation bundles"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / sigstore-layer3-completion / b60edce5bc49"
depends_on: [task-107]
progress: complete
commits: ["7e96fca019100d9bcc03aad6dff32640c3d18601", "fd91e5ba64f5b4a2ed4463afe9d9f1117df4c659", "cd0ff0eacdddd3e6d27acf5a86cb8427946a0595", "6f3dd98ddc96ba451c8773cede85fff2e8eda19b"]
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

Task-264 closes the five Layer-3 shortfalls on top of the task-107 stack. Implementation: `6f3dd98d` (pipeline checkpoint), `cd0ff0ea` (implementation + finalizer), review repair `fd91e5ba` (round `31d4fb6e7aa04318939755a5d28aa1bf`: branch pushes no longer overwrite existing attribution records; landed-commit enumeration fails closed), verification repair `7e96fca0` (deferred-suite fixture fixes). This round is merge repair `c8245f79a52e4d138cbf74f1d81da3ee`: the published PR conflicted with current main; merge commit `863ebbe6` resolves the single conflicted path `specs/coverage/system/identity-security.md` as a union — the task-264 layer-3 audit line is kept and extended to record that row 7 (Federation) takes main's task-262 implemented state, rows 1/2/5 keep the task-264/task-261 reconciled state, coverage recomputed to 10/10 (2 n/a, 0 not-started, 0 task-assigned, 3 verified, 7 implemented). All other paths auto-merged: `lib.rs` keeps both the task-264 `HttpRekorClient` wiring and task-262's `load_spiffe_service`/`FederationBundles` (AppState carries both), docs keep the task-264 signing/Rekor rows alongside the `GYRE_SPIFFE_FEDERATION_*` rows. No implementation behavior changed in the reconciliation.

### Behavior

1. **Every commit surface signs.** A single shared entry point, `commit_signatures::sign_commit`, is called by all four commit-producing surfaces, each persisting a `CommitSignature`: `jj new` (the `JjOpsPort::jj_new` port returns `JjNewChange {change_id, commit_id}` so the signable, content-addressed commit id is available), `jj squash`, git HTTP push (`git_http.rs` `git_receive_pack` — signs every landed commit *before* responding; on signing failure it undoes the ref updates, emits a `git_push_rejected` audit event naming the `commit-signing` gate and the active mode, and rejects the push), and MR merge-commit creation (`merge_processor.rs` `sign_merge_commit`, wired into both the single-MR path — signing failure triggers the post-merge recovery protocol: queue pause, revert, escalation — and the atomic-group path, which rolls the group back). Pushes over already-signed history skip SHAs with a persisted record (original attribution preserved; enumeration errors reject the push).
2. **Keyless by default.** `SigningMode::parse` maps unset/empty → `Fulcio`; `local` and `none` are explicit operator opt-ins only. An unrecognized `GYRE_SIGNING_MODE` value is a hard startup error instead of a silent `_ => Local` downgrade.
3. **No silent degradation.** On Fulcio failure the operation fails loudly with an error naming the active mode (403 for callers with no presentable OIDC JWT — `sigstore::jwt_is_presentable` gates *before* any HTTP call, so an empty/dev-token bearer is never presented to Fulcio; 502/push-rejection/merge-revert for stack failures). The only fallback is the operator-configured `GYRE_SIGNING_FULCIO_FALLBACK=local`, which warns and stores an honest `sigstore_mode=local` record.
4. **Attribution is cryptographically bound.** The local Ed25519 signature, the keyless ECDSA signature, and the Rekor hashedrekord digest all cover the canonical payload `{commit_sha, task_id, spawned_by}` (`signed_commit_payload`/`commit_digest`/`record_signed_payload`). Verification (local and fulcio paths, including the Rekor entry content-match) recomputes the payload from the record's own fields, so a tampered `task_id` or `spawned_by` fails verification.
5. **Real Rekor for attestation bundles.** `NoopRekorClient` is deleted; production wiring (`build_state`) and the test fixture wire `HttpRekorClient` (gyre-adapters/src/rekor.rs), which submits each signed bundle as a hashedrekord to `GYRE_REKOR_URL` (latency-bounded, 10 s timeout) and persists the returned entry UUID keyed by MR id (KV namespace `rekor_attestation_entries`). An unconfigured log or a Rekor outage is an explicit `Err` surfaced as a warn/audit state by `store_merge_attestation_bundle` — never a silent `Ok(None)` — while the already-landed merge is not rolled back.

Integration harnesses that push/merge with non-JWT dev tokens (git_integration, e2e_ralph_loop, graph_integration, spec_enforcement) explicitly set `GYRE_SIGNING_MODE=local` — the documented operator opt-in — mirroring `mem::test_state`, so Fulcio-by-default does not break unrelated push/merge coverage. Docs: server-config.md (`GYRE_SIGNING_MODE`, `GYRE_SIGNING_FULCIO_FALLBACK`, `GYRE_REKOR_URL`) and api-reference.md.

### Test evidence

- **Ran here (build wrapper /tmp/stage/bin/cargo, post-merge at `863ebbe6`):** `cargo check -p gyre-server --tests` — clean, type-checks the merge resolution across both the task-264 signing surfaces and task-262's federation code. `cargo test -p gyre-spiffe` 36/36 green (includes the task-262 federation suite merged in this round). `cargo test -p gyre-adapters rekor` 3/3 green. `cargo fmt --check -p gyre-server -p gyre-spiffe -p gyre-adapters` clean. `check-arch` green. Evidence saved under /tmp/stage/review-evidence.
- **The build wrapper rejects gyre-server test compilation in this assignment (4 GiB model budget), so the verifier must execute:** `cargo test -p gyre-server --lib -- merge_attestation::rekor_storage_tests sigstore::tests::verification_fails_on_rogue_leaf_paired_with_genuine_chain`, plus the task-264 suites: `commit_signatures::tests`, `api::jj::tests`, `git_http::tests::{push_signs_every_landed_commit, push_rejected_loudly_when_fulcio_unreachable, branch_push_over_signed_history_preserves_original_attribution, landed_commit_enumeration_failure_is_an_error}`, `merge_processor::merge_signing_tests`, and the regression-sensitive existing binaries `cargo test -p gyre-server --test git_integration --test e2e_ralph_loop --test graph_integration --test spec_enforcement` (these opt into local signing), plus `cargo test -p gyre-adapters rekor`.
- **Environment restriction:** the `jj` binary is not installed in this sandbox, so jj-dependent suites must run in the verifier environment. No listener was needed (in-process axum `oneshot` requests only); no transport restriction was encountered.
