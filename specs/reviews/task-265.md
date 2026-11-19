# Review — task-265 (SCIM user lifecycle: PATCH, deactivation semantics, enterprise extension, production wiring)

Spec: `specs/system/identity-security.md` §User Identity / SCIM Provisioning (gap spec-31d3ae72e12e22c52442e309).
Commits under review: `156d268f` (implementation), `d835ff67` (review repair — RFC 7644 `"Operations"` key), `3cf47643`/`d58ea0e4`/`efbde16d`/`e3723c02` (docs/process). Base `61b6080d`, candidate `e3723c02`.
Verdict: **approved** — all five requirement areas verified in production code; no blocking findings. Two non-blocking observations recorded below.

## Round 2 (this review, fresh verdict on candidate e3723c02)

Round 1 found the RFC 7644 `"Operations"` serde defect (silent no-op PATCH). The repair (`d835ff67`, `#[serde(alias = "Operations")]`) was re-verified from source: with `rename_all = "camelCase"` the field name stays `"operations"` and the alias adds the capital-O RFC form; the handler dispatch gates on `raw.get("Operations")` (the exact RFC key) and `ScimPatchRequest` has no `deny_unknown_fields`, so the `schemas` member is tolerated. The delta from the repair commit to the candidate is docs-only (`specs/tasks/task-265.md`).

### Probes run (logs under `/tmp/stage/review-evidence/`)

- `cargo check -p gyre-server` → exit 0 (7m45s cold).
- `cargo check -p gyre-server --test scim_lifecycle` → exit 0 (new 596-line integration binary type-checks against `build_router()`/`build_state()`).
- `cargo test -p gyre-adapters sqlite::user::tests::enterprise_manager_id_round_trips` → **1 passed** (real SQLite round-trip of the new `users.manager_id` column: create → find → change → clear).
- `cargo test -p gyre-domain` → all passed.
- All 21 mechanical invariant scripts → PASS (arch, ABAC route registry, ABAC exempt handlers, migration versions/SQL portability, mem port contracts, forged-scope fields, scope-literal/fabricated defaults, inert enforcement, lossy secrets, byte-slice truncation, relative path defaults, dead message kinds, MCP write tools, forwarded-header trust, in-memory state stores, unbounded external HTTP, fail-open ref resolution, task commit attribution).
- `rustfmt --check` on all seven touched Rust files → clean.
- `cargo test -p gyre-server --test scim_lifecycle` → **blocked by the build wrapper** (gyre-server codegen + model exceeds 4 GiB in this sandbox — infrastructure restriction, not a code defect). Deferred to the independent verifier.

### Verified (per acceptance criterion)

- **AC1 wiring:** `require_auth_middleware` (lib.rs) passes `/scim/v2/*` through; all 10 SCIM handlers enforce `check_scim_auth` first (403 unset token, constant-time `tokens_equal` — the plain `!=` was also fixed); SCIM routes are ABAC resolver-exempt with per-handler auth, and both ABAC gates pass. No non-SCIM route lives under `/scim/`; no path-traversal/percent-encoding bypass (matchit matches literal segments). `tests/scim_lifecycle.rs` drives everything through the full `build_router()` stack over a real SQLite temp-file DB — the pre-task test blind spot (directly mounted `api_router()`) is gone, with controls asserting the credential was live before asserting post-deactivation rejection.
- **AC2 PATCH deactivation:** `active:false` in both RFC PatchOp and bare-body forms runs `deactivate_via_scim`: soft `User::deactivate`, `sessions.revoke_all_for_user`, `impersonation::end_active_sessions_for_user`, agents' `spawned_by` cleared, `workspace_ownership::on_user_removed`, `user_deactivated` audit event (source "scim"). Serialized `active` mirrors `!is_deactivated()`. The auth.rs hoist (`credential_revoked` above the deactivation branch, API-key path) yields the AC's 401 "Session has been revoked" on credential replay without changing the admin-deactivation 403 path (revocation state is filtered by credential hash).
- **AC3 DELETE:** deprovisions via the same cascade — no hard `users.delete`; the unit test was inverted to assert the row stays resolvable with `active:false`.
- **AC4 enterprise extension:** RFC 7643 §4.3 URN serialized in `schemas` and as a sibling attribute (`organization` → tenant_id, `manager.value` → new `users.manager_id`, migration `2026-11-13-000067`, next unused number, portable SQL, SQLite+Postgres+mem round-trip); create/PUT/PATCH all persist; Schemas/ResourceTypes advertise the extension.
- **AC5 immutability + advertisement:** userName/externalId ignored on PUT/PATCH (adapter-enforced immutability unchanged); ServiceProviderConfig advertises `patch.supported: true`; idempotent create, emails, meta, no-password behavior untouched.
- **Coverage honesty:** `specs/coverage/system/identity-security.md` row 10 records the user-lifecycle half as real with source evidence and explicitly keeps the six `/scim/v2/Groups` endpoints and the group-to-workspace mapping open (task-266) — no inflated completion.

### Required verifier execution (deferred from this sandbox)

```
cargo test -p gyre-server --test scim_lifecycle
cargo test -p gyre-server --lib scim::
cargo test -p gyre-server --lib deactivate_user_cascades
cargo test -p gyre-server --lib auth::
cargo test -p gyre-server --lib workspace_ownership
cargo check -p gyre-server --all-targets
```

### Non-blocking observations (no action required for this task; candidates for task-266 hardening)

1. **Lowercase `"operations"` PATCH body is a silent 200 no-op through the handler.** The dispatch requires the exact RFC key `"Operations"`; a body with lowercase `"operations"` falls into the bare-body branch where the key is an ignored unknown attribute. The `#[serde(alias = "Operations")]` is therefore unreachable in production (dead through the dispatch), and the shipped note "the lowercase form still works" is accurate only at the serde-struct level. No conformant IdP sends the lowercase form (RFC 7644 §3.5.2 mandates `"Operations"`), and neither the RFC form nor the task-required bare-body form is affected — hence non-blocking. Suggested hardening: reject (400) a body whose only member is a non-`Operations` `operations` key, or dispatch on the alias too.
2. **`apply_enterprise` doc comment vs. behavior:** the comment claims an explicit JSON `null` clears `organization`, but `Option<String>` maps JSON null to `None` = leave-untouched (only nested `manager.value: null` clears). Comment inaccuracy only; no AC requires null-clearing.
