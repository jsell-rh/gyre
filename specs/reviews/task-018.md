# TASK-018 Review

**Reviewer:** Verifier

---

## R1 — `needs-revision` (2 findings)

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

---

## R2 — `needs-revision` (1 finding)

F1 and F2 are fully addressed:
- `CHAIN_ATTESTATION_NOTES_REF` is now `"refs/notes/attestations"` (attestation.rs:50). No stale references to `chain-attestations` anywhere in the codebase.
- `attach_chain_attestation_note` takes `chain: &[Attestation]` (attestation.rs:65). `read_chain_attestation_note` returns `Option<Vec<Attestation>>` (attestation.rs:129). `write_chain_note_if_committed` loads the full chain via `load_chain` (attestation.rs:185). Callers in `merge_processor.rs` (lines 565, 587) load full chains. Tests use `sample_chain` with 2-node chains and verify round-trip equality.

- [-] [process-revision-complete] **F3: Race condition between legacy and chain note writes — spec-required chain attestation may be silently overwritten**

  In `merge_processor.rs`, the legacy `AttestationBundle` write (line 517) and the new chain attestation write (line 569, inside `attach_chain_attestation_note`) both target `refs/notes/attestations` with `-f` as fire-and-forget `tokio::spawn` tasks. There is no ordering guarantee between them.

  Timeline:
  1. Line 517: `tokio::spawn(...)` — legacy write fires (JoinHandle dropped)
  2. Lines 559–566: Two `await`ed database queries (`find_by_commit`, `load_chain`)
  3. Line 569: `attach_chain_attestation_note(...)` `.await` — serializes JSON, spawns another `tokio::spawn` for the chain write, returns

  Both spawned tasks execute `git notes add -f` on the same ref. Whichever git command completes last determines the note content. The database queries in step 2 make it *likely* the legacy write (step 1) finishes before the chain write (step 3) starts — but there is no guarantee. With a fast in-memory database and a slow-to-start git process, the legacy write could complete after the chain write.

  If the legacy write wins the race, the git note contains `AttestationBundle` JSON. `read_chain_attestation_note` attempts to parse it as `Vec<Attestation>`, fails, and returns `None` — silently losing the chain attestation from git notes.

  The acceptance criterion states "chain overwrites legacy on same ref per spec." The spec §5.3 requires the chain attestation at `refs/notes/attestations`. A non-deterministic race means this requirement is not reliably met.

  **Fix options** (either is sufficient):
  - Await the legacy write's JoinHandle before starting the chain write, ensuring sequential ordering.
  - Skip the legacy note write when a chain attestation exists for the same commit (the chain overwrites it anyway, and the legacy `AttestationBundle` is separately persisted in the attestation store at line 550–553).

  **Affected code:**
  - `crates/gyre-server/src/merge_processor.rs:517` — legacy write `tokio::spawn` (JoinHandle dropped)
  - `crates/gyre-server/src/merge_processor.rs:569` — chain write via `attach_chain_attestation_note`

---

## R3 — `complete` (0 findings)

F1, F2, and F3 are all fully addressed:

- **F1** (wrong ref namespace): `CHAIN_ATTESTATION_NOTES_REF = "refs/notes/attestations"` (attestation.rs:50). Zero references to `chain-attestations` in the codebase.
- **F2** (single attestation instead of full chain): `attach_chain_attestation_note` takes `&[Attestation]` (attestation.rs:65), `read_chain_attestation_note` returns `Option<Vec<Attestation>>` (attestation.rs:129), `write_chain_note_if_committed` loads full chain via `load_chain` (attestation.rs:185). Merge processor loads full chains in both resolution paths (find_by_commit at line 532, find_by_task fallback at line 557). Tests use `sample_chain` with 2-node chains and verify full round-trip equality.
- **F3** (race condition): Merge processor now writes EITHER the chain attestation OR the legacy `AttestationBundle` — never both. The `chain_note_written` boolean (merge_processor.rs:524) gates the legacy fallback (line 579). No concurrent writes to `refs/notes/attestations` are possible for the same commit. Gate executor path (`write_chain_note_if_committed` at gate_executor.rs:307) has no competing legacy write.

All acceptance criteria verified:
1. Chain attestation written as git note on relevant commit ✓
2. Git note stored under `refs/notes/attestations` ✓
3. Reading returns full chain `Vec<Attestation>` ✓
4. Dual-write: chain takes priority, legacy only when no chain available ✓
5. Tests verify round-trip, commit isolation, overwrite, dual-write, and full field preservation ✓
6. `cargo test --all` passes (35 attestation-related tests) ✓
