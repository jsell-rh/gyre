# Review — task-264 (Sigstore layer-3 completion: every commit signed, keyless by default, no silent fallback, real Rekor for attestation bundles)

Spec: `specs/system/identity-security.md` §Layer 3; gap `68b06ebc09a7c8d49aa516d0`.
Base `05024b3e` → candidate `acdee889` (implementation `cd0ff0ea`, review repair `fd91e5ba`, base merge `43c0b505`, verification repair `7e96fca0`, process commits `6f3dd98d`/`8b3d4d78`/`98d0e362`/`60044b51`/`7c3b90eb`/`adffbd52`/`2a7d23d0`/`40bf87ba`/`1a9ebfb6`/`acdee889`).
Verdict: **approved** — no blocking findings.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server --tests` (build wrapper) → **exit 0** — the candidate type-checks including every touched test module (9m06s cold).
- `cargo test -p gyre-adapters rekor` → **3/3 passed**: `submit_round_trips_hashedrekord_and_surfaces_outage` (real TCP listener speaking the Rekor entries contract: hashedrekord body carries the bundle digest + bundle signature + forge public key; 500 → explicit Err naming the MR, never Ok(None)), `unconfigured_client_fails_explicitly`, `unsigned_bundle_is_refused`.
- `cargo fmt --check -p gyre-server -p gyre-adapters -p gyre-ports` → exit 0.
- 14 mechanical invariant scripts (arch, unbounded-external-http, byte-slice-truncation, migration-versions, mem-port-contracts, inert-enforcement, scope-literal-defaults, fabricated-scope-defaults, lossy-secret-conversion, relative-path-defaults, fail-open-ref-resolution, forwarded-header-trust, in-memory-state-stores, forged-scope-fields) → all exit 0.
- `grep -rn NoopRekorClient crates/` → zero production references (docs/spec history text only); `grep bearer_token.clone().unwrap_or_default` → zero matches (empty-token presentation to Fulcio fully removed).
- Full diff inspection of all 26 changed files.

Environment restrictions (not code defects): the build wrapper rejects gyre-server test compilation in this sandbox, and the `jj` binary is not installed. The verifier must execute the recorded server suites (below).

## Acceptance criteria — verified against source

1. **Every commit surface signs.** All four surfaces route through the shared `commit_signatures::sign_commit` and persist via `CommitSignatureRepository`: jj new (`api/jj.rs` — the port now returns `JjNewChange {change_id, commit_id}` so the signable commit id is available; response carries `commit_id`; regression test drives the real router through the signature + verification endpoints), jj squash (rewritten onto the shared entry point), git HTTP push (`git_http.rs git_receive_pack` — synchronous signing of every landed commit before the response; on failure refs are undone, a `git_push_rejected` audit event names the `commit-signing` gate + active mode, and the push is rejected 403/502; landed-commit enumeration failure rejects the push fail-closed; already-signed SHAs reached by branch-creation walks keep their original attribution records), MR merge (`merge_processor.rs sign_merge_commit` wired into BOTH the single-MR path — failure triggers the post-merge recovery protocol — and the atomic-group path — failure rolls the group back). The verification endpoint (`GET .../signature/verification`) validates through `verify_state_commit_signature`, which recomputes the canonical payload from the record's own fields for both local (real ring Ed25519) and fulcio (ECDSA + cert chain + Rekor content-match) modes.
2. **Keyless by default.** `SigningMode::parse` maps unset/empty → `Fulcio`; `local`/`none` are explicit opt-ins; an unrecognized value is a hard startup error (panics in `from_env`) instead of the old silent `_ => Local` downgrade. Production `build_state` uses `SigningConfig::from_env()`.
3. **No silent degradation.** On Fulcio failure `sign_commit` returns `SigningStackFailed { mode: "fulcio" }` → 403 (`NoOidcIdentity`, gated by `jwt_is_presentable` BEFORE any HTTP call — empty/dev-token bearers are never presented) or 502 naming the signing mode; push surface rejects with refs undone + audit; merge surface reverts/escalates. The ONLY fallback is operator-configured `GYRE_SIGNING_FULCIO_FALLBACK=local`, which warns and stores an honest `sigstore_mode=local` record. Kill-tests exist for each surface (`jj_squash_fulcio_unreachable_fails_loudly_no_local_fallback` asserts 502 + no local-key record; `push_rejected_loudly_when_fulcio_unreachable` asserts 502 + refs restored + no record + audit; `merge_commit_signing_fails_loudly_when_fulcio_unreachable`).
4. **Attribution cryptographically bound.** `signed_commit_payload` (canonical serde JSON `{commit_sha, task_id, spawned_by}`) is the signed material for local Ed25519, keyless ECDSA, and the Rekor hashedrekord digest (`commit_digest`). Verification recomputes it from the record's own fields, so tampering `task_id`/`spawned_by` breaks both the signature check and the Rekor entry content-match. Tamper tests exist for both modes (`local_verify_fails_on_tampered_task_attribution`, `sigstore::tests::verification_fails_on_tampered_task_attribution` — the task's verification-intent probe verbatim).
5. **Real Rekor for attestation bundles.** `NoopRekorClient` is deleted; `HttpRekorClient` (latency-bounded reqwest, 10 s) is wired in `build_state` (configured only via `GYRE_REKOR_URL`) and submits each signed bundle as a hashedrekord; `store_merge_attestation_bundle` persists the entry UUID (KV `rekor_attestation_entries`, keyed by MR id) and returns an explicit `Err` on unconfigured/outage/missing-UUID — surfaced as a warn by both merge success paths while the landed merge is not rolled back. Adapter wire tests pass locally (see probes).

## Verification repair (`7e96fca0`) — checked against the durable finding

The prior verifier run (`cargo test --all` at `60044b51`) failed exactly 3 of 2484 tests, all in suites the build-budget rule defers from local execution. The repair commit addresses each root cause correctly: the rekor-storage fixture now builds a real git repo with a real commit object (the old fake SHA `rekorsha` failed `git notes add` before Rekor was reached), and the rogue-leaf attack now signs the canonical attribution-bound payload (`record_signed_payload`) so the record is internally consistent and only the certificate-leaf binding (phase b) catches it — preserving the test's isolation. `unconfigured_rekor_fails_explicitly` now asserts `contains("not configured")` specifically instead of the accidental `contains("rekor")` match.

## Tests the independent verifier must execute

- `cargo test -p gyre-server --lib commit_signatures sigstore merge_attestation::rekor_storage_tests merge_processor::merge_signing_tests` (includes the three repaired tests and the tamper/fail-loud kill-tests)
- `cargo test -p gyre-server --lib api::jj::tests git_http::tests` (jj-new signing, push signing, fail-loud, attribution-preservation, enumeration fail-closed)
- `cargo test -p gyre-server --test git_integration --test e2e_ralph_loop --test graph_integration --test spec_enforcement` (these harnesses opt into `GYRE_SIGNING_MODE=local` — regression-sensitive)
- `cargo test -p gyre-adapters rekor jj_ops` (jj binary required for jj_ops; rekor already run here: 3/3 pass)
- `cargo test --all` as the final gate

## Non-blocking observations (recorded, no action required)

- jj new cannot un-create the change when signing fails; the operation returns an explicit error (403/502) and no signature record is persisted — the fail-loud contract is met, the unsigned change simply remains in the working copy.
- In fulcio mode a branch-creation push signs every reachable unsigned commit serially (each bounded by the 10 s signing HTTP timeout) — a latency characteristic of "every commit signed", not a defect.
- Merge-attestation bundles are submitted to Rekor only when `GYRE_REKOR_URL` is explicitly set, while commit-signature recording defaults to the public Rekor — an intentional, documented asymmetry (server-config.md) avoiding publishing MR metadata to a public log by default.
- Working-tree note: `specs/tasks/task-264.md` is dirty in this checkout (frontmatter reset to the assignment state by stage bookkeeping); the assigned candidate commit `acdee889` contains the full task record with `## Shipped`. Not a candidate defect.
- Generated `web/dist/` was not rebuilt by the agent — owned by the deterministic verifier per the standing rule; no authored frontend changes are in this diff.
