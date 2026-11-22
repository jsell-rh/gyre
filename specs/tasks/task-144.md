---
title: "Implement attestation bundle storage targets"
spec_ref: "agent-gates.md §Part 3 Storage"
depends_on:
  - task-142
progress: complete
coverage_sections:
  - "agent-gates.md §Storage"
commits: ["164c44be9b1b3cc779244d7cd9e02007d5d57bbd"]
---

## Spec Excerpt

From `agent-gates.md` §Part 3 Storage:

Merge attestation bundles are stored:
- As a **git note** on the merge commit (accessible via `git notes show`)
- In the **audit_events** table for queryability
- In **Rekor** transparency log for non-repudiation
- In the **AIBOM** for each release that includes the merge commit

## Implementation Plan

1. **Define `AttestationStorage` port trait** in `gyre-ports`:
   ```rust
   #[async_trait]
   pub trait AttestationStorage: Send + Sync {
       async fn store(&self, bundle: &MergeAttestationBundle) -> Result<()>;
       async fn retrieve_by_commit(&self, commit_sha: &str) -> Result<Option<MergeAttestationBundle>>;
       async fn retrieve_by_mr(&self, mr_id: &Id) -> Result<Option<MergeAttestationBundle>>;
   }
   ```

2. **Implement git note storage** in `gyre-adapters`:
   - After merge, write the serialized bundle JSON as a git note on the merge commit
   - Use `git notes add -f --ref=attestations <sha>` with the bundle JSON
   - Retrieval via `git notes show --ref=attestations <sha>`

3. **Implement audit_events storage** in `gyre-adapters`:
   - Record the attestation bundle as an audit event with `event_type: "merge_attestation"`
   - Store the full bundle JSON in the `details` field
   - Index by `mr_id` and `merge_commit_sha` for queryability

4. **Rekor integration (stub):**
   - Define a `RekorClient` port trait for transparency log submission
   - Implement a no-op adapter initially with a TODO for Rekor API integration
   - Log a warning when Rekor is not configured

5. **AIBOM integration point:**
   - Add a method to retrieve all attestation bundles for a set of commit SHAs (for release AIBOM generation)
   - This is a query method on `AttestationStorage`, not a separate storage target

6. **Wire into merge processor** — after bundle generation (task-142), call all storage backends.

7. **Tests:**
   - Integration test: bundle stored as git note, retrievable via `git notes show`
   - Integration test: bundle stored in audit_events, queryable by MR ID
   - Unit test: retrieval by commit SHA returns correct bundle
   - Unit test: retrieval by MR ID returns correct bundle

## Acceptance Criteria

- [ ] `AttestationStorage` port trait with store/retrieve operations
- [ ] Git note storage: bundle written to `refs/notes/attestations` on merge commit
- [ ] Audit events storage: bundle queryable by MR ID and commit SHA
- [ ] Rekor integration stubbed with no-op adapter
- [ ] AIBOM query method for bulk attestation retrieval by commit SHAs
- [ ] Merge processor stores bundle after generation
- [ ] Tests pass

## Agent Instructions

- Read task-142 output for the `MergeAttestationBundle` type
- Read `crates/gyre-server/src/merge_processor.rs` for the merge flow
- Read `crates/gyre-adapters/src/` for adapter implementation patterns
- Read `crates/gyre-ports/src/audit.rs` (or equivalent) for the audit event recording pattern
- For git note operations, use `std::process::Command` to run git commands (same pattern as existing git operations in the codebase)
- The hexagonal boundary applies: port traits in `gyre-ports`, implementations in `gyre-adapters`

## Shipped

### Behavior

- **Port** (`gyre-ports/src/attestation_storage.rs`): `AttestationStorage` trait — `store(repo_path, scope, bundle)`, `retrieve_by_commit`, `retrieve_by_mr`, and `retrieve_by_commits` (the AIBOM bulk query: every bundle attached to a set of commit SHAs, for release AIBOM generation). `AttestationScope` carries the workspace/repo identity the audit record is filed under. Companion `RekorClient` port (`gyre-ports/src/rekor.rs`) for transparency-log submission.
- **Adapter** (`gyre-adapters/src/attestation_storage.rs`, `MergeAttestationStorage`): implements the port over the two merge-time storage targets. **Git note**: the serialized bundle is written to `refs/notes/merge-attestations` on the merge commit via the git binary (`git notes add -f`), readable back with `git notes show` — the exact mechanism the spec names. (This ref, not the task-sketch `refs/notes/attestations`, is deliberate: the chain-attestation dual-write in `crate::attestation` owns `refs/notes/attestations` — task-142 shipped the separate ref for that reason and this preserves it.) **Audit event**: a `merge_attestation_bundle` event carrying the full signed bundle JSON in `detail`, indexed by MR id (`resource_type` "merge_request", `resource_id` = MR id) and scoped to the merge's workspace/repo. The audit record is written first: a git-note failure surfaces as the returned `Err` while the bundle stays queryable. Retrieval by commit serves the git note, falling back to the audit record when no note exists; retrieval by MR id and the AIBOM bulk query scan the audit records completely — the query limit is derived from `stats_by_type` and grown when writes raced the count, so no stored bundle is unreachable behind an arbitrary cap.
- **Rekor** (`gyre-adapters/src/rekor.rs`): `NoopRekorClient` — per the task plan's staged stub, every submission warns that non-repudiation is pending (`Rekor transparency log not configured`) and returns no log entry; `is_configured() == false`. The real Rekor API integration (hashedrekord submission, `GYRE_REKOR_URL`) remains open behind the port — this is the one §Storage target not production-real yet.
- **Wiring**: `AppState` carries `attestation_storage` + `rekor` (production wiring in `lib.rs` wraps the same search-sync-decorated audit repository every other audit event flows through; test states mirror it). Both merge-processor success paths (single-MR `process_next` and each atomic-group member) now store the generated bundle through the port and submit it to Rekor; `store_merge_attestation_bundle` in `gyre-server/src/merge_attestation.rs` delegates to the ports and keeps the never-roll-back-the-landed-merge contract (failures logged, surfaced). Storage-target constants (`MERGE_ATTESTATION_NOTES_REF`, `MERGE_ATTESTATION_AUDIT_TYPE`) moved to `gyre-domain` beside the bundle type and are re-exported from the server module.

### Test evidence

Probes run in this sandbox (build wrapper permits check-based and adapter/domain-scale probes):

- `cargo test -p gyre-adapters --lib attestation_storage` — 7 passed: git-note storage retrievable via `git notes show` (real temp git repo + real SQLite audit store), audit-event storage queryable by MR id (raw audit record and port path), commit retrieval + audit fallback when the note is removed, AIBOM bulk query over three merges, note-failure surfaces `Err` while the audit record stays queryable.
- `cargo test -p gyre-adapters --lib rekor` — 1 passed (no-op reports unconfigured, returns no entry).
- `cargo test -p gyre-adapters --lib` — 545 passed, 0 failed (full adapter suite, no regressions).
- `cargo test -p gyre-domain` — 539 + 8 passed (constants addition).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` / `--tests` — 0 errors (test target included).
- `cargo clippy -p gyre-ports -p gyre-adapters` and `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --lib`: warning profile on the touched files identical before/after the change (verified via stash round-trip); `cargo fmt --all --check` clean; `scripts/check-arch.sh` and the invariant checks (mem-port-contracts, inert-enforcement, unbounded-external-http, relative-path-defaults, lossy-secret-conversion, scope-literal-defaults, fabricated-scope-defaults, in-memory-state-stores, forwarded-header-trust) all pass.

gyre-server test execution is blocked by the sandbox build wrapper (server test codegen exceeds the 4 GiB budget — same constraint task-142 recorded); the independent verifier must run:

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor_generates_multi_party_attestation_bundle` — extended this round to also retrieve the bundle by MR id through the `AttestationStorage` port, proving the port wiring end-to-end after a real merge-cycle.
