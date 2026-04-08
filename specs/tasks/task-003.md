# TASK-003: Authorization Provenance — Domain Types and Port Traits

**Spec reference:** `authorization-provenance.md` §1–§5  
**Progress:** `complete`  
**Review:** [`specs/reviews/task-003.md`](../reviews/task-003.md)

## Spec Excerpt

`authorization-provenance.md` introduces cryptographic work authorization. This task creates the foundational domain types and port traits needed by all subsequent authorization provenance tasks.

### Types to create (in `gyre-common`):

- `TrustAnchor` — registered identity issuers (§1.1)
- `KeyBinding` — ephemeral Ed25519 keypair bound to user identity (§2.3)
- `SignedInput` — authorization root created on spec approval (§2.1–2.2)
  - `InputContent` — spec_path, spec_sha, workspace_id, repo_id, persona_constraints, meta_spec_set_sha, scope
  - `ScopeConstraint` — allowed_paths (glob[]), forbidden_paths (glob[])
- `OutputConstraint` — named CEL predicate (§3.1)
- `DerivedInput` — delegation provenance from orchestrator (§4.1)
- `Attestation`, `AttestationOutput`, `AttestationMetadata` — complete attestation record (§5.1)
- `GateAttestation`, `GateConstraint` — per-gate signed results (§3.2, §5.1)
- `VerificationResult` — recursive verification tree (§6.4)
- `ConstraintViolation` — new MessageKind variant (§7.5)

### Port traits to create (in `gyre-ports`):

- `ChainAttestationRepository` — save, find_by_id, load_chain, find_by_task, find_by_commit, find_by_repo (§5.4)
- `TrustAnchorRepository` — CRUD for trust anchors (tenant-scoped)
- `KeyBindingRepository` — store/invalidate/find key bindings

## Implementation Plan

1. Create types in `gyre-common` (new files: `attestation.rs`, `trust_anchor.rs`, `key_binding.rs`, `output_constraint.rs`)
2. Add `ConstraintViolation` to `MessageKind` enum in `message.rs`
3. Create port traits in `gyre-ports` (new files: `chain_attestation.rs`, `trust_anchor.rs`, `key_binding.rs`)
4. Add unit tests for serialization/deserialization of all new types
5. Ensure hexagonal boundary: these types must NOT import gyre-adapters

## Acceptance Criteria

- [x] All types from §1–§5 exist in `gyre-common` with Serialize/Deserialize
- [x] `ConstraintViolation` added to `MessageKind` enum
- [x] `ChainAttestationRepository` port trait defined in `gyre-ports`
- [x] `TrustAnchorRepository` port trait defined
- [x] `KeyBindingRepository` port trait defined
- [x] Unit tests for serde round-trips on all types
- [x] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `authorization-provenance.md` §1–§5 for type definitions
3. Read `crates/gyre-common/src/message.rs` for MessageKind pattern
4. Read `crates/gyre-ports/src/` for port trait patterns
5. Follow existing patterns for ID types, serde annotations, etc.
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `288c90be` feat(provenance): add authorization provenance domain types and port traits (TASK-003)
