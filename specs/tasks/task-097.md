---
title: "Platform Model Secrets Domain Types + Port"
spec_ref: "platform-model.md §7 Secrets Delivery"
depends_on: []
progress: not-started
coverage_sections:
  - "platform-model.md §7 Secrets Delivery"
  - "platform-model.md §7 Principle"
  - "platform-model.md §7 Architecture"
  - "platform-model.md §7 Secret Scoping"
  - "platform-model.md §7 Secret Types"
  - "platform-model.md §7 Storage Backend"
commits: []
---

## Spec Excerpt

### Principle

Agents must never see their secrets in plaintext. Secrets are injected into the agent's environment by the platform, used opaquely, and revoked on teardown.

### Secret Scoping

```
Tenant secrets (shared across all workspaces)
  └── Workspace secrets (shared across repos in workspace)
        └── Repo secrets (specific to one repo)
              └── Task secrets (one-time, per-task)
```

### Secret Types

| Type | Example | Lifecycle |
|---|---|---|
| Static | `DATABASE_URL`, `SIEM_ENDPOINT` | Set by admin, persisted (encrypted at rest) |
| Ephemeral | Per-session DB credentials, short-lived API tokens | Generated at spawn, revoked at teardown |
| Rotated | OAuth tokens, Claude Max refresh tokens | Background job refreshes before expiry |
| Derived | Agent's own OIDC token, git credential | Generated from identity, scoped to session |

### Storage Backend

Default: secrets encrypted at rest with SOPS in database. Optional Vault integration.

## Implementation Plan

1. **Domain types (gyre-common):**
   ```rust
   pub enum SecretScope { Tenant, Workspace, Repo, Task }
   pub enum SecretType { Static, Ephemeral, Rotated, Derived }

   pub struct Secret {
       pub id: Id,
       pub name: String,
       pub scope: SecretScope,
       pub scope_id: String,        // tenant_id, workspace_id, repo_id, or task_id
       pub secret_type: SecretType,
       pub created_by: String,
       pub created_at: u64,
       pub expires_at: Option<u64>,
       pub last_rotated_at: Option<u64>,
       pub tenant_id: String,
   }
   ```
   Note: The `Secret` type never contains the plaintext value in-memory outside the adapter layer.

2. **Port trait (gyre-ports):**
   ```rust
   pub trait SecretRepository: Send + Sync {
       async fn create(&self, secret: &Secret, value: &[u8]) -> Result<()>;
       async fn get_value(&self, id: &Id, tenant_id: &str) -> Result<Option<Vec<u8>>>;
       async fn list_by_scope(&self, scope: SecretScope, scope_id: &str, tenant_id: &str) -> Result<Vec<Secret>>;
       async fn delete(&self, id: &Id, tenant_id: &str) -> Result<()>;
       async fn rotate(&self, id: &Id, new_value: &[u8], tenant_id: &str) -> Result<()>;
       async fn resolve_for_agent(&self, tenant_id: &str, workspace_id: &str, repo_id: &str, task_id: Option<&str>) -> Result<Vec<(String, Vec<u8>)>>;
   }
   ```

3. **Secret resolution for agent spawn:**
   - `resolve_for_agent` collects secrets from all applicable scopes (tenant → workspace → repo → task)
   - Secrets are injected as environment variables in the agent container
   - Replace the current `GYRE_CRED_*` hardcoded injection with dynamic secret resolution

4. **Encryption at rest:**
   - Use `ring` or `aes-gcm` for AES-256-GCM encryption of secret values in the database
   - Encryption key derived from `GYRE_SECRET_ENCRYPTION_KEY` env var (or auto-generated and stored)
   - The adapter encrypts on write, decrypts on read — domain layer never sees encrypted bytes

5. **Database migration:**
   - `secrets` table: id, name, scope, scope_id, secret_type, encrypted_value, nonce, created_by, created_at, expires_at, last_rotated_at, tenant_id
   - Indexes on (scope, scope_id, tenant_id) for efficient resolution

## Acceptance Criteria

- [ ] Secret, SecretScope, SecretType domain types defined
- [ ] SecretRepository port trait with all methods
- [ ] SQLite adapter with AES-256-GCM encryption at rest
- [ ] Database migration for secrets table
- [ ] resolve_for_agent collects secrets from all scopes
- [ ] Agent spawn uses resolved secrets instead of hardcoded GYRE_CRED_*
- [ ] Secret values never logged or serialized to JSON
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §7 "Secrets Delivery" for the full spec. The current credential injection is in `gyre-server/src/api/spawn.rs` around lines 603-637 (GYRE_CRED_* prefix). Follow the hexagonal pattern: types in gyre-common, port in gyre-ports, adapter in gyre-adapters. Use `ring` for encryption (already a dependency for Ed25519 in key_binding.rs). The migration numbering is currently at 000038 — check the latest migration number before creating yours.
