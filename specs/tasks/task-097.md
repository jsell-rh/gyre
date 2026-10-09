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
commits: ["c50733cd7e0346d09a2bab8d10589065ff8e1a1f", "a5bfe3c6ed030271ffe6337b4d8005b8cc9fc8c0", "838c3a87f8b47ccc5f1c79fabb3a5114b65ccf86", "be39d1530e054fad34c306110a78c01af435af17", "65385c33d97de49dc354ef64ce46e7cd61f10a11", "312bcbd7c6fb12003efa8a8457078f1a83d77aea", "7e9a6ccf07581c89f425eab4ee7ec17be9c9c35b", "a3fde958cd56d6f760d5e59b84927601ba708f17", "a38170c9c866a05033b23238398660b35e333eee", "01493c8864dda8a8c82b46d0aff141df0b017aad", "3a5be015d2fa96ef14a55109098fe869cf56ae13"]
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

## Revision Round 1 (findings F1–F5, specs/reviews/task-097.md)

Product fixes in `d5fe703a` (+ `7968dcf1` test-import fix):

- **F1 (mem uniqueness contract):** `MemSecretRepository::create` now rejects duplicates mirroring the SQLite `UNIQUE(id)` + `UNIQUE(tenant, scope, scope_id, name)` failure mode; contract tests in `mem.rs` (`secret_contract_tests`: reject dup id, reject dup scope/name same tenant, allow same name cross-tenant, allow diff name same scope). `mem-port-contracts-exemptions.txt` drained to 0 (frozen count lowered 1→0).
- **F2 (spawn integration coverage):** five spawn-level tests in `api/spawn.rs` deliver secrets through a real spawned process (env-dump compute target): all four scopes delivered, nearest-scope-wins, unresolvable workspace skips all injection, non-UTF-8 secret skipped while others deliver, resolve-error does not fail spawn.
- **F3 (fabricated "default" tenant):** spawn resolves the tenant from the workspace record and skips (warns on) all scoped secret resolution when the workspace is unresolvable — no `"default"` fallback. Same-class sibling in `constraint_check.rs::create_violation_notifications` fixed the same way. `fabricated-scope-defaults-exemptions.txt` drained 8→6 (both task-097-owned lines removed, frozen count lowered).
- **F4 (lossy secret conversion):** injection path uses fallible `String::from_utf8`; non-UTF-8 values are skipped with a warn naming the secret, never the value. `lossy-secret-conversion-exemptions.txt` drained 1→0.
- **F5 (silent key downgrade):** `load_encryption_key` emits a startup `warn!` (both fresh-generation and existing-persisted-key paths) stating that encryption-at-rest degrades to obfuscation when `GYRE_SECRET_ENCRYPTION_KEY` is unset, with remediation. No spec amendment needed — spec wording "encrypted at rest" still holds; the degraded default is now operator-visible.

`commits:` now lists the five task-labeled product commits reachable from main (the previous entries were unreachable rebase duplicates, invisible to review scoping). A temporary `[patch.crates-io]` pq-sys build shim added during the round was reverted in `7968dcf1` (confirmed absent from Cargo.toml).

## Shipped

- Platform Model §7 secrets: `Secret`/`SecretScope`/`SecretType` domain types (no value field in metadata), `SecretRepository` port (create/get/list/delete/rotate/resolve_for_agent with nearest-scope-wins cascade and expired-secret exclusion), SQLite adapter with AES-256-GCM at rest via `ring` (per-value nonce, `GYRE_SECRET_ENCRYPTION_KEY` hex/passphrase or auto-generated persisted key with operator-visible degradation warning), and the `secrets` migration (000053) with scope index.
- Agent spawn now resolves scoped secrets (tenant → workspace → repo → task) from the repository and injects them as `GYRE_CRED_*` env vars — the hardcoded `GYRE_AGENT_CREDENTIALS`/`GYRE_AGENT_GCP_SA_JSON` injection is gone; unresolvable workspace, resolve errors, and non-UTF-8 values each skip-and-warn (naming the secret, never the value) instead of fabricating tenants or corrupting values.
- Both adapters enforce the port's duplicate-rejection contract (SQLite via UNIQUE constraints, mem via an in-code guard with contract tests), and five end-to-end spawn tests deliver secrets through a real spawned process and pin every fallback branch of the injection path.
