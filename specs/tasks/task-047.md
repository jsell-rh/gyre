# TASK-047: Authorization Provenance — Key Binding Revocation Endpoint and Audit Event

**Spec reference:** `authorization-provenance.md` §2.3, §7.7  
**Depends on:** TASK-006 (key binding creation — complete)  
**Progress:** `not-started`

## Spec Excerpt

From `authorization-provenance.md` §2.3:

> **Key lifecycle:**
> 1. User authenticates to the platform via their tenant IdP (Keycloak, etc.).
> 2. Client generates an ephemeral Ed25519 keypair.
> 3. Client constructs the `KeyBinding` document, signs it with the private key.
> 4. Platform verifies the user's IdP session is valid, countersigns the binding, and stores the public key.
> 5. The private key remains client-side (browser, CLI). It signs `SignedInput` documents.
> 6. On expiry or logout, the binding is invalidated. A new session requires a new binding.

From `authorization-provenance.md` §7.7:

> | Event | Category | Description |
> |---|---|---|
> | `key_binding.revoked` | Identity | Key binding explicitly revoked |

## Current State

- **Key binding creation** exists: `POST /api/v1/auth/key-binding` (`crates/gyre-server/src/api/key_binding.rs`) — creates a key binding, emits `key_binding.created` audit event ✓
- **Key binding expiry** is handled: `key_binding.expired` tracing event emitted in `git_http.rs` when an expired binding is encountered ✓
- **Adapter methods exist but are uncalled:**
  - `KeyBindingRepository::revoke()` — sets `revoked_at` on a specific key binding (`crates/gyre-adapters/src/sqlite/key_binding.rs:173`)
  - `KeyBindingRepository::revoke_all_for_user()` — revokes all bindings for a user (`crates/gyre-adapters/src/sqlite/key_binding.rs:201`)
- **No revocation endpoint** — There is no `DELETE /api/v1/auth/key-binding` or equivalent revocation route
- **No `key_binding.revoked` audit event** — The event name does not appear anywhere in `crates/gyre-server/src/`
- **No logout integration** — No code path triggers revocation on user session termination

## Implementation Plan

1. **Add revocation endpoint** — `DELETE /api/v1/auth/key-binding/:id`:
   - Caller must be the owner of the key binding (match `user_id` from auth) or a tenant admin
   - Call `KeyBindingRepository::revoke()` to set `revoked_at`
   - Emit `key_binding.revoked` structured tracing event with user identity, binding ID, timestamp
   - Return 204 on success, 404 if binding not found, 403 if not owner/admin

2. **Add revoke-all endpoint** — `DELETE /api/v1/auth/key-bindings` (bulk):
   - Revokes all active key bindings for the authenticated user
   - Useful for logout flow ("invalidate all my keys")
   - Calls `KeyBindingRepository::revoke_all_for_user()`
   - Emits `key_binding.revoked` for each revoked binding

3. **Emit `key_binding.revoked` audit event:**
   - Structured tracing: `event = "key_binding.revoked"`, with binding ID, user identity, revoked_by, timestamp
   - Same pattern as existing `key_binding.created` event in `key_binding.rs:149-156`

4. **Register routes in `api/mod.rs`:**
   - Add both DELETE routes alongside the existing `POST /api/v1/auth/key-binding`

5. **Add tests:**
   - Revoke own key binding → 204, binding shows `revoked_at`
   - Revoke non-existent binding → 404
   - Revoke another user's binding (non-admin) → 403
   - Revoke-all revokes all active bindings for user
   - Revoked binding rejected at push time (already handled by `is_null()` filter on `revoked_at`)
   - `key_binding.revoked` event emitted

## Acceptance Criteria

- [ ] `DELETE /api/v1/auth/key-binding/:id` revokes a specific key binding
- [ ] `DELETE /api/v1/auth/key-bindings` revokes all active bindings for the authenticated user
- [ ] Revocation sets `revoked_at` timestamp on the key binding record
- [ ] Only the binding owner or a tenant admin can revoke
- [ ] `key_binding.revoked` structured tracing event emitted on revocation
- [ ] Revoked bindings are rejected at push-time chain verification
- [ ] Tests cover revocation, authorization, and audit event emission
- [ ] `cargo test --all` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `crates/gyre-server/src/api/key_binding.rs` for the existing creation endpoint pattern
3. Read `crates/gyre-adapters/src/sqlite/key_binding.rs` for the `revoke()` and `revoke_all_for_user()` adapter methods
4. Read `crates/gyre-ports/src/key_binding.rs` for the `KeyBindingRepository` port trait
5. Read `crates/gyre-server/src/api/mod.rs` to register the new routes alongside existing key binding routes
6. Follow the same tracing event pattern as `key_binding.created` (key_binding.rs:149-156)
7. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

_(none yet)_
