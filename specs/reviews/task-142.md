# Review — task-142 (Implement merge attestation bundle schema and multi-party signing)

Spec: `specs/system/agent-gates.md` §Part 3 "Merge Attestation Bundle" + "What Gets Signed and By Whom".
Candidate under review: `afe2497b` (base `2824d9b1`; product commit `331a6eaf`, plus process/doc commits `eb227476`/`2ece2a2b`/`dfc54ad4`/`afe2497b`).
Verdict: **needs-revision** — two findings (F1, F2), both attestation-content fidelity defects with mechanical repairs. Core deliverable is otherwise real and verified as far as this sandbox's build budget allows.

## Round 1 (2026-11-14)

Probes (exit-coded evidence in `/tmp/stage/review-evidence/`):

- `/tmp/stage/bin/cargo test -p gyre-domain merge_attestation` → **4 passed** (spec-schema field names + round-trip; canonical unsigned JSON excludes signature deterministically; gate unsigned JSON; sha256 fixed vector).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → **clean** (type-checks the lib and all new server tests, including `merge_processor_generates_multi_party_attestation_bundle`).
- `scripts/check-arch.sh` → passed (domain types in `gyre-domain`, signer port in `gyre-ports`, no forbidden deps/I/O).
- `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-inert-enforcement`, `check-lossy-secret-conversion`, `check-fabricated-scope-defaults` → all OK.
- `check-task-commit-attribution.sh` timed out (needs full history); by inspection the only product-surface commit in range (`331a6eaf`) is recorded in the candidate frontmatter.
- gyre-server test *execution* deferred to the independent verifier per the 4 GiB budget protocol. **Verifier must run:** `cargo test -p gyre-server --lib merge_attestation` and `cargo test -p gyre-server --lib merge_processor_generates_multi_party_attestation_bundle`.

Verified working (no findings):

- **Domain type + JSON schema.** `MergeAttestationBundle`/`SpecRefAttestation`/`AuthorAttestation`/`GateAttestation` serialize under the spec's exact key names (`merge_attestation_version` "1.0", `mr_id`, `merge_commit_sha`, `merged_at`, `spec_ref` string, `spec_approval` object, `author`, `gates[]` with `name`/`type`/`status`/`output_hash`/timing/`signed_by`/`signature` and the AgentReview extras, `bundle_signature`). `unsigned_canonical_json()`/`unsigned_gate_json()` define deterministic signature payloads (signature field excluded from its own payload).
- **Port.** `AttestationSigner` (`gyre-ports/src/attestation_signer.rs`) matches the task plan's trait exactly; hexagonal boundary clean.
- **Real signing.** `ForgeSigner` signs with the server's real Ed25519 key (`AgentSigningKey::sign_bytes`); `verify_merge_attestation_bundle` performs real `ring` Ed25519 verification against the named party's key. The tamper tests mutate real signed content (gate status, `merged_at`, cross-party signature swap) and assert verification failure — not structural checks.
- **Generation after every merge.** Both merge success paths (`process_next` single-MR and each `merge_atomic_group` member) call `generate_merge_attestation_bundle` post-merge; no existing merge logic was modified (diff is purely additive after the merge lands); failure logs and never rolls back the merge. No other code path performs merges (verified by grep for merge-execution call sites).
- **Multi-party attribution.** Forge signs TestCommand/LintCommand (and unknown/fallback gates); the gate agent that submitted the review signs AgentReview/AgentValidation results, with reviewer identity/decision/body-hash resolved from the bound `Review` (`Review.gate_id`). Spec approval is carried from the approval ledger's newest active entry for the exact (path, sha) — never fabricated; MRs without an author agent omit the author block.
- **Storage.** Audit event (`merge_attestation_bundle` custom type; verified the query filter matches `Custom` string round-trip) + git note under `refs/notes/merge-attestations` (separate ref, does not race the legacy dual-write).
- **Tests are real guards.** The integration test drives `process_next` over a seeded MR with one TestCommand and one AgentReview gate, asserts `MrStatus::Merged`, parses the recorded audit event back into a bundle, and verifies every signature; removing the generation call fails it. No self-confirming/mirrored-logic tests found.

Findings:

- [F1] **(major) Author `attestation_level` is derived by a third, divergent rule that contradicts both canonical derivations.** `crates/gyre-server/src/merge_attestation.rs` `build_author_attestation` maps container→3 / non-"local" compute_target→2 / else→1. The platform's spawn JWT (`api/spawn.rs` ~852) keys on resolved target *type* (Kubernetes → **1**), and `constraint_check::derive_attestation_level` (used at spec-approval validation, commented "Never invented") keys on stack/container evidence (local + attested stack → **2**). Concrete divergences: a Kubernetes-target agent gets JWT level 1 but bundle level 2; a local agent with an attested stack gets bundle level 1 ("raw subprocess") where the platform's canonical derivation says 2. The code comment claiming parity with the spawn path is false for both cases, and the new unit test asserts the divergent value (`Some(1)`). In a provenance document, the bundle can attest a level contradicting the JWT minted for the same agent. **Repair:** derive from the agent's stored token claims (`agent_tokens` KV) or call `constraint_check::derive_attestation_level`, update the comment, and fix the test's expected value.
- [F2] **(moderate) `GateAttestation.reviewer_stack_attestation` is declared but never populated.** The spec's bundle schema shows it on AgentReview gate entries and identity-security.md §Layer 2 requires merge attestation to "record which stack produced each gate result"; grep shows the field is only ever assigned `None`. The reviewer's stack fingerprint is available from the same `workload_attestations` KV namespace the author lookup already reads (keyed by `review.reviewer_agent_id`). **Repair:** populate it in `build_gate_attestation`/`generate_merge_attestation_bundle` exactly like the author's, and extend a test to cover it.

Minor (non-blocking, recorded for completeness):

- `GateAgentSigner` keys are derived from the forge key, so the forge can produce gate-agent signatures; the spec's "no single entity can forge the complete bundle" property is not cryptographically real yet. This is the placeholder the task explicitly permits and the Shipped notes disclose it, but the coverage-matrix row 13 phrase "so the forge alone cannot forge a complete bundle" overstates the property — reword when task-143/144 land real OIDC identities.
- `merged_at` is u64 epoch seconds while the spec's JSON example shows ISO-8601; the task plan mandates u64, so this follows the assigned contract (flag for the spec owner, not the implementer).
- The working tree carries an uncommitted revert of `specs/tasks/task-142.md` to `not-started` (stage setup, present before this review began, untouched); the candidate commit itself carries `progress: ready-for-review` + Shipped notes.

Test-inflation check: none found — all new tests assert against real signatures, real store round-trips, or real merge execution.

Scope check: diff is limited to the four new/edited source files plus task/coverage docs; no production code outside the added bundle path was touched, and the merge processor's existing logic is unmodified.
