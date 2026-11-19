# Review — task-264 (Sigstore layer-3 completion: every commit signed, keyless by default, no silent fallback, real Rekor for attestation bundles)

Spec: `specs/system/identity-security.md` §Layer 3 (gap `68b06ebc09a7c8d49aa516d0`).
Candidate under review: `b2f05414c100c73941658440afd81dae1937074e` against base
`4e88f183` (task-261 SPIFFE head; the rebase reconciliation `a9081053` merged main's
SPIFFE work as a union — AppState carries both the task-264 `HttpRekorClient` wiring
and task-261's `spiffe` service; verified by `cargo check -p gyre-server --tests`,
exit 0).
Verdict: **approved** — all five required behaviors are implemented in production
code with killing tests; no fake completion, no test inflation, no unsafe scope
handling found.

## Probes run (evidence: `/tmp/stage/review-evidence/`)

- `cargo check -p gyre-server --tests` (forced recheck) → exit 0, `Checking gyre-server
  v0.1.0` → `Finished dev profile in 2m 26s`, zero errors (type-checks the SPIFFE
  rebase union and every touched test module).
- `cargo test -p gyre-adapters rekor` → exit 0, **3/3 passed**, including the real
  wire round-trip against a local TCP listener speaking the Rekor
  `/api/v1/log/entries` contract (hashedrekord body assertions: bundle digest +
  bundle signature + forge public key; HTTP 500 outage → explicit Err naming the MR).
- `cargo test -p gyre-adapters jj_new` → exit 0 (the jj-wire assertions early-return
  via `jj_fixture_guard()` — **`jj` is not installed in this sandbox**; deferred to
  the verifier, see below).
- `cargo fmt --check -p gyre-server -p gyre-adapters -p gyre-ports` → exit 0.
- Mechanical checks all green: check-arch, check-unbounded-external-http,
  check-mem-port-contracts, check-byte-slice-truncation,
  check-in-memory-state-stores, check-dead-message-kinds,
  check-abac-exempt-handlers, check-task-commit-attribution (the last fails only
  against the stage harness's working-tree reset of task-264.md; passes at the
  committed candidate, verified via a stash/restore round-trip that left the tree
  unchanged).
- Full diff inspection of all 26 touched files (source, tests, docs, coverage).

## Verified against the acceptance criteria

1. **Every commit surface signs.** A single shared entry point
   (`commit_signatures::sign_commit`) is called by all four surfaces: `jj new`
   (returns `JjNewChange{change_id, commit_id}` from the port so the signable
   content-addressed commit id is available; record persisted and validated through
   the verification endpoint in
   `jj_new_signs_commit_with_persisted_validated_signature`), `jj squash`, git HTTP
   push (`git_receive_pack` signs every landed commit *before* responding; on signing
   or enumeration failure it undoes the refs, emits an audited `git_push_rejected`
   naming the `commit-signing` gate and active mode, and rejects with 403/502), and
   MR merge-commit creation (`sign_merge_commit`, wired into both the single-MR path
   — failure triggers the post-merge recovery protocol — and the atomic-group path,
   which rolls the group back). Review repairs hold: pushes over signed history skip
   SHAs with a persisted record (original attribution preserved,
   `branch_push_over_signed_history_preserves_original_attribution` asserts
   signer/task/spawner/signature/signed_at all survive), and landed-commit
   enumeration fails closed (`landed_commit_enumeration_failure_is_an_error`).
   `GYRE_SIGNING_MODE=none` skips signing only as an explicit opt-in.
2. **Keyless by default.** `SigningMode::parse` maps unset/empty → Fulcio;
   `local`/`none` are explicit; an unrecognized value is a hard startup error (the
   old silent `_ => Local` downgrade is gone). Production `build_state` uses
   `SigningConfig::from_env()`. Mem fixtures and the four non-JWT integration
   harnesses explicitly opt into `local` (documented operator opt-in).
3. **No silent degradation.** Fulcio failure → explicit error naming the active mode
   (403 for callers with no presentable OIDC JWT — `sigstore::jwt_is_presentable`
   gates *before* any HTTP call, so an empty/dev-token bearer is never presented to
   Fulcio; 502/push-rejection/merge-revert for stack failures). The only fallback is
   operator-configured `GYRE_SIGNING_FULCIO_FALLBACK=local`, which warns and stores
   an honest `sigstore_mode=local` record. The old
   "Fulcio keyless signing failed; falling back to local signing" path is deleted
   (grep-verified). Killing tests: `jj_squash_fulcio_unreachable_fails_loudly_no_local_fallback`
   (502 + error names 'fulcio' + NO local record at the squash SHA),
   `jj_squash_fulcio_without_jwt_is_refused` (403 + "no presentable OIDC identity" +
   no record), `push_rejected_loudly_when_fulcio_unreachable` (502 + refs undone +
   no record + audited), `merge_commit_signing_fails_loudly_when_fulcio_unreachable`.
4. **Attribution is cryptographically bound.** The local Ed25519 signature, the
   keyless ECDSA signature, and the Rekor hashedrekord digest all cover the canonical
   payload `{commit_sha, task_id, spawned_by}` (`signed_commit_payload`, fixed field
   order). Verification (local path, fulcio phases (a) and (d)) recomputes the payload
   from the record's OWN fields, so a tampered task_id/spawned_by fails signature and
   Rekor-entry checks. Killing tests:
   `local_verify_fails_on_tampered_task_attribution`,
   `sigstore::verification_fails_on_tampered_task_attribution` (tampered record fails
   signature_valid AND rekor_entry_exists while cert/subject stay valid),
   `signed_payload_is_canonical_and_attribution_sensitive`, attribution-sensitive
   `commit_digest`. The rogue-leaf test signs the same canonical payload so it still
   isolates leaf binding (repair 7e96fca0).
5. **Real Rekor for attestation bundles.** `NoopRekorClient` is deleted; production
   wiring and the mem fixture use `HttpRekorClient` (hashedrekord over the bundle's
   canonical JSON carrying the existing multi-party `bundle_signature` + forge public
   key; latency-bounded 10 s). The returned entry UUID is persisted (KV namespace
   `rekor_attestation_entries`, keyed by MR id). An unconfigured log, an outage, or a
   missing UUID is an explicit `Err` surfaced as a warn/audit state by
   `store_merge_attestation_bundle` — never a silent `Ok(None)` — while the landed
   merge is not rolled back. Tests: `rekor_storage_tests` (scripted client over the
   real storage path with a real git repo/commit), adapters wire tests.

Coverage rows updated accurately (identity-security row 5 → implemented with concrete
source evidence; row 1 aggregate updated for the reconciled three-layer stack;
agent-gates row 15 → implemented). No unrelated rows touched.

## Deferred to the independent verifier (sandbox restrictions, not code defects)

- `cargo test -p gyre-server --lib -- commit_signatures::tests api::jj::tests
  merge_processor::merge_signing_tests merge_attestation::rekor_storage_tests
  sigstore::tests` and the four git_http push-signing tests (the build wrapper
  rejects gyre-server test compilation in the model sandbox).
- `cargo test -p gyre-server --test git_integration --test e2e_ralph_loop --test
  graph_integration --test spec_enforcement` (opt into local signing).
- `cargo test -p gyre-adapters` full suite — the jj-wire `jj_new` commit-id template
  test requires the `jj` binary, absent here.
- Final `cargo test --all`.

## Non-blocking observations (recorded for completeness)

- `api/spawn.rs` best-effort workspace provisioning also calls `jj_ops.jj_new`
  (transient working-copy snapshot on the spawn branch; failure already degrades with
  a warn). It is not one of the task's four enumerated commit surfaces; those
  snapshots acquire signatures when landed through push/squash/merge.
- Fulcio-mode push signs each new commit serially (Fulcio + Rekor round trips, each
  bounded by a 10 s timeout); a large initial keyless push is latency-heavy.
  Correctness unaffected.
- In Fulcio mode the merge queue mints a short-lived forge-issued merge-actor JWT
  (Gyre is a real OIDC IdP per Layer 2, so a Fulcio trusting the forge accepts it);
  against an untrusting Fulcio the merge fails loudly into the revert/escalation
  recovery — the documented fail-loud contract.
- `GYRE_SIGNING_FULCIO_FALLBACK=local` applies to stack failures only, not to the
  no-OIDC-identity refusal; the refusal is an explicit 403 either way. Documented in
  `docs/server-config.md`.

Test-inflation check: none found — every new test drives the real handler/storage
path and fails if the specced behavior is disabled (silent fallback, unsigned push,
record replacement, silent Rekor Ok, tampered attribution).
