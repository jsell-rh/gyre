---
title: "Authorization Provenance — Storage Adapters and Migration"
spec_ref: "authorization-provenance.md §5.3–5.4"
depends_on: 
  - task-003
progress: complete
review: specs/reviews/task-004.md
coverage_sections: []
commits: 
  - 085e5584
---

## Spec Excerpt

Attestations are stored in the database, indexed by content hash, task_id, repo_id, workspace_id. Storage supports chain traversal via parent_ref. Key bindings and trust anchors also require persistence.

## Implementation Plan

1. Create Diesel migration for:
   - `trust_anchors` table (id, tenant_id, issuer, jwks_uri, anchor_type, constraints_json, created_at)
   - `key_bindings` table (id, user_id, tenant_id, public_key, issuer, trust_anchor_id, issued_at, expires_at, platform_countersign, revoked_at)
   - `attestations` table (id TEXT PK [content hash], input_type TEXT, input_json TEXT, output_json TEXT, metadata_json TEXT, parent_ref TEXT, chain_depth INTEGER, workspace_id TEXT, repo_id TEXT, task_id TEXT, agent_id TEXT, created_at INTEGER, tenant_id TEXT)
   - Indexes: attestation by task_id, by commit_sha, by repo_id+created_at, by parent_ref

2. Implement SQLite adapter for `ChainAttestationRepository` (implement `load_chain` as recursive parent_ref walk)
3. Implement SQLite adapter for `TrustAnchorRepository`
4. Implement SQLite adapter for `KeyBindingRepository`
5. Add integration tests

## Acceptance Criteria

- [ ] Migration creates all required tables with proper indexes
- [ ] SQLite `ChainAttestationRepository` adapter passes tests
- [ ] SQLite `TrustAnchorRepository` adapter passes tests
- [ ] SQLite `KeyBindingRepository` adapter passes tests
- [ ] `load_chain` correctly walks parent_ref to reconstruct full chain
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Check current migration number: `ls crates/gyre-adapters/migrations/ | tail -3`
3. Read existing adapter patterns in `crates/gyre-adapters/src/sqlite/`
4. Follow the Diesel model pattern used by other adapters
5. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `085e5584` feat(provenance): add storage adapters and migration for authorization provenance (TASK-004)
