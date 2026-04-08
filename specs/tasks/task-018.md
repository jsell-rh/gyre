# TASK-018: Authorization Provenance — Git Notes Attestation Storage

**Spec reference:** `authorization-provenance.md` §5.3  
**Depends on:** TASK-008 (Phase 3: attestation chains are produced)  
**Progress:** `not-started`

## Spec Excerpt

From `authorization-provenance.md` §5.3:

> Attestations are stored in three locations:
>
> 1. **Attestation store** (database) — indexed by `id` (content hash), `task_id`, `repo_id`, `workspace_id`. Supports chain traversal via `parent_ref`.
>
> 2. **Git notes** (`refs/notes/attestations`) — the chain attestation is attached to the relevant commit, same as the existing merge attestation bundle.
>
> 3. **AIBOM** — release-time aggregation includes the full chain for each commit, replacing the flat agent-attribution model.

Storage locations (1) and (3) are covered by TASK-004 and TASK-009 respectively. This task implements location (2): git notes.

## Current State

- **Database storage** (TASK-004): `chain_attestations` table with `commit_sha` column — queries by commit SHA work ✓
- **Git notes for merge attestation**: The existing `MergeAttestation` / `AttestationBundle` is attached to commits via git notes at `refs/notes/attestations` in `crates/gyre-server/src/attestation.rs`
- **Chain attestation in git notes**: NOT implemented — the new `Attestation` chain type is stored only in the database, not in git notes

## Implementation Plan

1. **Extend the attestation module** (`crates/gyre-server/src/attestation.rs`):
   - Add `fn attach_chain_attestation_note(repo_path, commit_sha, attestation_chain_json)` that writes the full attestation chain as a git note under `refs/notes/attestations`
   - Reuse the existing git notes infrastructure (the same function that writes `MergeAttestation` notes)
   - Serialize the chain as JSON (same format as the database `input_json`/`output_json`/`metadata_json`)

2. **Call the new function** at attestation creation points:
   - When a new `Attestation` is saved to the database with a non-null `commit_sha`, also write it as a git note
   - This happens in Phase 3 (TASK-008) push/merge handlers and Phase 4 (TASK-009) full chain production

3. **Add a read function** for git notes attestation:
   - `fn read_chain_attestation_note(repo_path, commit_sha) -> Option<Vec<Attestation>>`
   - Used by the offline verification bundle export (TASK-008, `GET /api/v1/repos/{id}/attestations/{commit_sha}/bundle`)
   - Also useful for clone-time portability — attestation travels with the repo

4. **Handle dual-write** (backward compatibility):
   - During the migration period, both the legacy `MergeAttestation` note and the new chain attestation note are written
   - Use a sub-namespace: `refs/notes/attestations` (legacy) and `refs/notes/chain-attestations` (new) to avoid collisions
   - The legacy note continues to be written until TASK-009 deprecates `AttestationBundle`

5. **Add tests:**
   - Write chain attestation note → read it back → matches original
   - Multiple attestations for different commits don't collide
   - Reading a note for a commit without attestation returns None
   - Dual-write produces both legacy and chain notes

## Acceptance Criteria

- [ ] Chain attestation is written as a git note on the relevant commit
- [ ] Git note is stored under `refs/notes/chain-attestations`
- [ ] Reading a chain attestation from git notes returns the full chain
- [ ] Dual-write produces both legacy merge attestation and chain attestation notes
- [ ] Tests verify note round-trip and isolation between commits
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/attestation.rs` for the existing git notes pattern
3. Read `crates/gyre-adapters/src/git2_ops.rs` for git2 note operations
4. Read `authorization-provenance.md` §5.3 for storage requirements
5. Add the git notes write/read functions following the existing pattern
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
