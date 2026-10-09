---
title: "Platform Model Secrets Domain Types + Port"
spec_ref: "platform-model.md §7 Secrets Delivery"
depends_on: []
progress: needs-revision
coverage_sections:
  - "platform-model.md §7 Secrets Delivery"
  - "platform-model.md §7 Principle"
  - "platform-model.md §7 Architecture"
  - "platform-model.md §7 Secret Scoping"
  - "platform-model.md §7 Secret Types"
  - "platform-model.md §7 Storage Backend"
commits: ["86c11e375ba89eed09e22b5c20aa18943e792388", "c0f6f042df285973fcf1cb8e7e60ddff75ba2fde", "bb331e32e73b28eed81ad05793b5baa282795bf9", "5206b8750328f39c1c76294044f25dbe989a8178", "d18ace22aa87aaec1818eadad6532259fe8f9b55", "9072ac62ed0e94612fefd1d908cc034fd47548d5", "3953d31ee5ebdf23b980165530e2612fcc1a11b3", "70e9cc03f58af6a7e6f453cf14eded17b7f3f338", "d60e8d0efe9f8be5c312f946804942503c83105b", "a3fde958cd56d6f760d5e59b84927601ba708f17", "a38170c9c866a05033b23238398660b35e333eee", "01493c8864dda8a8c82b46d0aff141df0b017aad", "3a5be015d2fa96ef14a55109098fe869cf56ae13"]
review: specs/reviews/task-097.md
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

- [x] Secret, SecretScope, SecretType domain types defined
- [x] SecretRepository port trait with all methods
- [x] SQLite adapter with AES-256-GCM encryption at rest
- [x] Database migration for secrets table
- [x] resolve_for_agent collects secrets from all scopes
- [x] Agent spawn uses resolved secrets instead of hardcoded GYRE_CRED_*
- [x] Secret values never logged or serialized to JSON
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §7 "Secrets Delivery" for the full spec. The current credential injection is in `gyre-server/src/api/spawn.rs` around lines 603-637 (GYRE_CRED_* prefix). Follow the hexagonal pattern: types in gyre-common, port in gyre-ports, adapter in gyre-adapters. Use `ring` for encryption (already a dependency for Ed25519 in key_binding.rs). The migration numbering is currently at 000038 — check the latest migration number before creating yours.
