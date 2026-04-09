# TASK-018 Review — R1

**Reviewer:** Verifier  
**Verdict:** `needs-revision` (2 findings)

---

## Findings

- [-] [process-revision-complete] **F1: Wrong git notes ref namespace — task-vs-spec transcription error**

  The spec §5.3 (line 519 of `authorization-provenance.md`) states:

  > **Git notes** (`refs/notes/attestations`) — the chain attestation is attached to the relevant commit, **same as the existing merge attestation bundle**.

  "Same as the existing merge attestation bundle" means the same ref that the legacy `AttestationBundle` uses — `refs/notes/attestations` (confirmed by `attestation.rs:8`). The task plan invented a separate ref `refs/notes/chain-attestations` to "avoid collisions," and the code follows the task's incorrect value:

  ```rust
  // attestation.rs:48
  pub const CHAIN_ATTESTATION_NOTES_REF: &str = "refs/notes/chain-attestations";
  ```

  The spec is the source of truth. The chain attestation must use `refs/notes/attestations`. Git notes supports only one note per object per ref, so the dual-write model requires the chain attestation to *replace* the legacy note on the same ref (or the note content must be a combined format). The task's sub-namespace approach contradicts the spec's explicit `refs/notes/attestations` designation and "same as" language.

  **Affected code:**
  - `crates/gyre-server/src/attestation.rs:48` — constant definition
  - All callers that reference `CHAIN_ATTESTATION_NOTES_REF`
  - Tests that verify the ref path

- [-] [process-revision-complete] **F2: Single attestation stored instead of full chain**

  The task plan specifies:

  > `fn attach_chain_attestation_note(repo_path, commit_sha, attestation_chain_json)` that writes **the full attestation chain** as a git note

  > `fn read_chain_attestation_note(repo_path, commit_sha) -> Option<Vec<Attestation>>`

  The acceptance criteria state:

  > Reading a chain attestation from git notes returns **the full chain**

  The implementation writes a single `&Attestation` and reads `Option<Attestation>`:

  ```rust
  // attestation.rs:58-62
  pub async fn attach_chain_attestation_note(
      repo_path: &str,
      commit_sha: &str,
      attestation: &Attestation,  // single, not Vec
  )

  // attestation.rs:121-123
  pub async fn read_chain_attestation_note(
      repo_path: &str,
      commit_sha: &str,
  ) -> Option<Attestation>  // single, not Vec
  ```

  The purpose of git notes storage (§5.3) is clone-time portability — attestation travels with the repo. A single leaf node cannot be verified offline because the chain cannot be reconstructed without the database (parent_ref links to attestation IDs in the database store). The note must contain the full chain (`Vec<Attestation>`) so that an offline verifier can walk and verify the complete authorization chain from the note alone.

  The callers in `merge_processor.rs` and `gate_executor.rs` also pass a single attestation rather than loading the full chain via `load_chain` before writing.

  **Affected code:**
  - `crates/gyre-server/src/attestation.rs:58-62` — write function signature
  - `crates/gyre-server/src/attestation.rs:121-123` — read function signature
  - `crates/gyre-server/src/gate_executor.rs:307` — passes single leaf
  - `crates/gyre-server/src/merge_processor.rs:564-586` — passes single attestation
  - All tests that round-trip a single attestation
