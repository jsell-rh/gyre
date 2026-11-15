# Review — task-143 (Implement attestation bundle verification protocol)

Spec: `specs/system/agent-gates.md` §Part 3 Verification.
Commits under review: `c880471a` (implementation), `c2df4d71` (review repair — reject forge-signed agent gates in the consistency check); process commits `b1c8ed5f`/`a9061837` are frontmatter-only.
Candidate: `a9061837a307ebcac0946e995847b40a4f9a9ddd`, base `b8d63b0d115c1b7623480f69010f43d718e336bc`.
Verdict: **approved** — every acceptance criterion holds with independent probe evidence; the one prior-round defect (forge-signed agent gate passing) is repaired and pinned by a mutation-verified test.

Evidence: `/tmp/stage/review-evidence/task-143/probes.md`.

## Probes run

- `cargo test -p gyre-domain` — 558 passed, 0 failed (11 `attestation_verification` tests, 4 `merge_attestation` tests).
- `cargo test -p gyre-cli` — 222 passed, 0 failed (command parsing; mock-server capture of the real `POST /api/v1/attestations/verify` request + per-component response parsing; human-readable rendering; real-git note reading with explicit error on a missing note).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — 0 errors (7m24s, warm shared target). Server test *runs* deferred to the verifier per the 4 GiB budget directive; required commands recorded in the evidence file.
- **Mutation probe on the c2df4d71 repair**: in a throwaway worktree at the candidate, the consistency check's forge arm (reject agent gate types signed by "forge") was replaced with a no-op → `agent_gate_signed_by_forge_fails_consistency` **FAILED**. The test is a real guard, not self-confirming. Worktree removed; checkout clean.
- Invariant gates on the candidate: `check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`, `check-relative-path-defaults.sh`, `check-task-commit-attribution.sh`, `check-inert-enforcement.sh` — all exit 0.

## Verified against the acceptance criteria

- **`AttestationVerifier` port trait defined** (`gyre-ports/src/attestation_verifier.rs`): `verify_bundle`/`verify_gate_signature`/`verify_spec_approval`, matching the task's trait sketch. Wired into `AppState.attestation_verifier` (production `build_state`, `mem.rs` test state, and the middleware test clone — all three construction sites).
- **`VerificationResult` captures per-component status** (`gyre-domain/src/attestation_verification.rs`): exactly the planned shape (`valid`, `bundle_signature_valid`, `gate_results[]`, `spec_approval_valid: Option<bool>`, `consistency_check`, `errors[]`) plus `GateVerificationResult`.
- **Structural consistency checks (pure, I/O-free)**: `check_bundle_consistency` enforces non-empty `mr_id`/`merge_commit_sha`, well-formed `path@sha` `spec_ref`, `spec_approval` only with a `spec_ref`, per-gate signature presence, the signing-party rule in **both directions** (command gate signed by an agent rejected; AgentReview/AgentValidation gate signed by the forge rejected — the multi-party property of §What Gets Signed and By Whom), reviewer `signed_by`/`reviewer_agent_id`/`reviewer_oidc_sub` binding, and author `oidc_sub`/`agent_id` binding. Note: the bundle format (task-142) carries a single `mr_id`/`spec_ref` at bundle level rather than per-gate copies, so "same MR across gates" is enforced as the invariants that make that single binding sound — a reasonable, documented adaptation (the plan's "mismatched MR IDs across gates" unit test is structurally impossible in this format; empty-`mr_id` is tested instead).
- **Cryptographic verification for bundle, gates, and spec approval**: `ForgeAttestationVerifier` uses real ring Ed25519 (`verify_ed25519` over the canonical unsigned JSON) — the bundle signature against the forge server key, per-gate signatures against the party `signed_by` names (forge key or per-agent re-derived key), spec-approval signatures against the approver's key when present. Tampering tests exercise real crypto (mutated status fails exactly that gate; mutated `merged_at` fails the bundle signature; a cross-party swapped signature fails under the named key). Spec approval is an honest tri-state: absent → `None` + explicit finding (never silently passed); present agent-approver signatures round-trip `Some(true)`/`Some(false)`; non-agent approvers are reported not platform-verifiable (task-142's reviewed deferral pending Sigstore/OIDC).
- **`POST /api/v1/attestations/verify`**: registered in `api/mod.rs`, ABAC resolver entry `attestation`/`read` (not the frozen exemption file — registry gate passes), documented in `docs/api-reference.md`. Failed verification is 200 with `valid: false` + findings; non-bundle body → 422. Handler tests drive the real router via `oneshot`.
- **CLI `gyre verify-attestation <commit-sha>`**: reads the bundle from the merge commit's git note via real `git notes show` (missing note → explicit error, not a silent pass), POSTs to the endpoint, prints the per-component result, exits 1 on failure. Documented in `docs/cli.md` including the notes-ref fetch hint.
- **Tests pass** — see probes; server-side suites recorded for the verifier (`--lib merge_attestation` incl. `forge_signed_agent_review_gate_fails_verification`, `--lib api::attestations`, merge-processor e2e).
- **Spec step 4 (Rekor)** correctly stays with task-144's storage scope: the bundle format carries no author commit SHAs to check, and the task plan (items 1–6, acceptance criteria) does not include it.

## Prior-round defect (c2df4d71) — repaired and pinned

The consistency check's forge arm previously never checked the gate type, so a forge self-attested AgentReview passed consistency and verified against the forge key (`valid: true`). The repair errors on agent gate types in the forge arm; both a domain consistency test and a server-side full-protocol test (forge key genuinely re-signs the AgentReview gate and the bundle — still `valid: false` on the step-5 rule, with `signature_valid: true` proving the rejection is the signing-party rule, not a crypto artifact) pin it. The mutation probe above independently confirms the domain test fails when the check is disabled.

## Scope and process observations

- **`scripts/check-inert-enforcement.sh` change is a legitimate repair, not gate weakening.** The base script referenced `BOUND` in its hit line but never defined it — any statement-position *consumed* `verify_*` method call crashed the script with `NameError` (reproduced on the base script with a probe file; exit 1, no verdict). The new `self.verify_gate_signature(g).await.unwrap_or(false)` in `merge_attestation.rs` would have hit exactly this at CI. The candidate defines `BOUND` with a consumption-shape regex; probes confirm it (a) passes the consumed call, (b) still flags a genuinely inert `self.verify_gate_signature(g).await;`, and (c) passes repo-wide. No exemption entries added anywhere.
- Coverage row 14 marked implemented with accurate notes; row 13's supersession note is correct (the bool verifier became the per-component protocol). Attribution gate passes; both product commits are in the task's `commits:` frontmatter.
- No frontend changes; no generated-output observations.

## Minor (non-blocking, recorded for completeness)

- The CLI duplicates the notes-ref default (`"refs/notes/merge-attestations"`) as a string literal because the canonical `MERGE_ATTESTATION_NOTES_REF` constant lives in `gyre-server`, which `gyre-cli` cannot import; the values match today. Hoisting the constant to `gyre-domain` (where the bundle type lives) would remove the duplication.
- Test name `body_that_is_not_a_bundle_is_rejected_400` asserts `UNPROCESSABLE_ENTITY` (422) — axum's actual code for a `Json` rejection; the name is stale, the assertion is right.
- A gate with a missing signature is reported twice (once by the consistency check, once in `gate_results[].errors`) — redundant but harmless.
- A bundle with zero gates passes the consistency check vacuously; only the forge key holder can produce a validly signed such bundle, and the merge processor only emits bundles after gates pass, so this is not exploitable.

Test-inflation check: none found. The end-to-end and repair tests use real key material, real signatures over real canonical bytes, and the real router/git CLI; the mutation probe proves the key test kills the defect it pins.

Verdict: **approved**. `approved: true`, findings empty — see `/tmp/stage/verdict.json`.
