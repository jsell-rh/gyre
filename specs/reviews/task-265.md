# Review — task-265 (SCIM user lifecycle completion: PATCH, deactivation semantics, enterprise extension, production wiring)

Spec: `specs/system/identity-security.md` §User Identity / SCIM Provisioning (gap spec-31d3ae72e12e22c52442e309 / scim-users-lifecycle).
Candidate: `efbde16d136392487e61244389144351e6b117ed` (base `67cc03988b938c4635973761c08108cee8f8459d`), product commit `156d268f`.
Verdict: **needs-revision** — one blocking finding (F1: RFC 7644 PatchOp bodies are a silent no-op); the rest of the implementation is verified clean by inspection, type-check, and focused probes.

## Round 1

### Probes run (evidence under `/tmp/stage/review-evidence/`)

- `bash scripts/check-abac-route-registry.sh` → exit 0 (`abac-route-registry.out`)
- `bash scripts/check-abac-exempt-handlers.sh` → exit 0 (`abac-exempt-handlers.out`)
- `bash scripts/check-migration-versions.sh` → exit 0 — `000067_user_manager_id` is the next unused sequence (`migration-versions.out`)
- `bash scripts/check-migration-sql-portability.sh` → exit 0 — plain `ALTER TABLE users ADD COLUMN manager_id TEXT;`, both backends (`migration-portability.out`)
- `bash scripts/check-arch.sh` → exit 0 (`arch.out`)
- `cargo test -p gyre-adapters sqlite::user` → **37 passed, 0 failed**, including the new `enterprise_manager_id_round_trips` create/update/clear round-trip (`adapters-sqlite-user.out`)
- `cargo check -p gyre-server` → exit 0 (`server-check.out`)
- `cargo check -p gyre-server --test scim_lifecycle` → exit 0 — the new integration test target type-checks (`server-check-scim-test.out`)
- `cargo test -p gyre-server --test scim_lifecycle` → **blocked by the build wrapper** (gyre-server codegen exceeds the 4 GiB sandbox budget). Not run here; see "Verifier must run" below.
- **Serde deserialization probe** (`serde-operations-key-defect.txt` + `serde-operations-key-defect-probe.rs`): a standalone `rustc` binary compiled against the workspace's own cached `serde`/`serde_json`/`serde_derive` artifacts, replicating `ScimPatchRequest`/`ScimPatchOp` verbatim from `crates/gyre-server/src/api/scim.rs` and the exact `raw.get("Operations")` branch logic of `scim_patch_user`, fed the exact PATCH body the candidate's own test sends. Output: **`scim_patch_user applied operations: 0`** — the RFC 7644 wire key `"Operations"` is silently dropped.

### Verified working (no findings)

- **Production wiring (requirement 1).** `require_auth_middleware` (lib.rs ~line 848) now passes `/scim/v2/*` through before the global token ladder, with an accurate comment. The exemption is fail-closed: every `/scim/v2` handler enforces `check_scim_auth`, which returns 403 when `GYRE_SCIM_TOKEN` is unset and now compares the bearer in constant time via `auth::tokens_equal` (replacing the previous early-exit `!=`). ABAC resolver has pre-existing `RouteResourceMapping::exempt` entries for all five `/scim/v2` route patterns (including `/scim/v2/Users/:id`, which covers the new PATCH method — route-registry and exempt-handler gates confirm green). `last_seen_middleware` degrades to `user_id: None` on failed auth extraction (no rejection), so the full `build_router()` stack passes SCIM requests through to the handlers.
- **Deactivation cascade (requirement 3).** `deactivate_via_scim` performs a real lifecycle event: soft `User::deactivate` (row remains resolvable — SCIM DELETE no longer hard-deletes; the old `users.delete` + comment admitting the deviation is gone), `sessions.revoke_all_for_user` (first invocation from the SCIM path), `impersonation::end_active_sessions_for_user`, agents' `spawned_by` cleared (all attributed agents — justified in-comment: dead agents can auto-restart), `workspace_ownership::on_user_removed`, and a `user_deactivated` audit event tagged `source: "scim"`. Serialized `active` is now `!user.is_deactivated()` instead of the hardcoded `true`. The supporting auth.rs reorder (credential_revoked check hoisted above the deactivation check in the API-key branch) is behavior-preserving for the admin path (no revoked sessions → falls through to the accurate 403) and gives SCIM-deactivated credentials the AC's expected 401.
- **Enterprise extension (requirement 4).** `ScimUser`/`ScimUserRequest` carry the URN-renamed `enterprise` field; `user_to_scim` emits `organization` (tenant id) and `manager.value`; create/PUT persist via `apply_enterprise`; SQLite and Postgres row/record/update mappings plus migration `000067` and domain field `User.manager_id` (`#[serde(default)]` — old rows deserialize fine) are all wired; Schemas/ResourceTypes discovery advertises the extension. Adapter round-trip proven by the passing `enterprise_manager_id_round_trips` test.
- **Core-schema preservation (requirement 5).** PUT still never writes `username`/`external_id`; PATCH ignores `userName`/`externalId`/`name` (documented as lenient, not 409 — consistent with PUT); ServiceProviderConfig advertises `patch.supported: true`; idempotent create, emails, meta, no password fields unchanged.
- **Test design.** The new `tests/scim_lifecycle.rs` genuinely goes through `build_router()` (not a directly mounted `api_router()`), uses a real SQLite temp-file DB, drives user provisioning through the real admin API, and includes control assertions (pre-deactivation credential live, session row exists pre-deactivation) that make the lifecycle assertions non-self-confirming. The wrong-token/unset-token/global-token tests cover both directions of the wiring fix.

### Findings

- [**F1 — blocking, code**] `crates/gyre-server/src/api/scim.rs` (~lines 138-145, 610-617): **`scim_patch_user` silently applies zero operations for every RFC 7644 PatchOp body — the standard PATCH deactivation path is a no-op.** `ScimPatchRequest` derives `#[serde(rename_all = "camelCase")]` with field `operations`, so serde looks for the key `"operations"` (lowercase o). RFC 7644 §3.5.2 — and the candidate's own integration tests — send `"Operations"` (capital O). serde is case-sensitive and the struct has `#[serde(default)]`, so `from_value` succeeds with an **empty** operations vec whenever the `"Operations"` branch is taken (the branch guard `raw.get("Operations")` matches the capital-O key, then hands the same value to a deserializer expecting lowercase). Result: `PATCH /scim/v2/Users/{id}` with `{"schemas":[...PatchOp],"Operations":[{"op":"replace","path":"active","value":false}]}` returns 200 with the resource unchanged — no deactivation, no session revocation, no spawned_by cleanup. Only the non-standard simplified bare-body form (`{"active": false}`) works. A lowercase `"operations"` body fares no better (it falls to the bare-body branch, where `operations` is an unknown ignored attribute). Reproduced independently with a probe compiled against the workspace's serde artifacts, using the exact struct, exact branch logic, and exact test body (`/tmp/stage/review-evidence/serde-operations-key-defect.txt`): applied operations = 0.

  Consequences against the acceptance criteria:
  - AC 2 (PATCH `active:false` deactivates) is unmet for the standard wire form an IdP actually sends.
  - The candidate's own test `patch_active_false_deactivates_user_end_to_end` **fails at runtime** (asserts `active == false` after a PatchOp PATCH that applied 0 ops). All gyre-server test executions were deferred to the verifier, so this was never run — `cargo check` cannot catch it.
  - `enterprise_extension_serialized_and_persisted`'s PATCH assertion also fails (PatchOp no-op leaves the manager at the PUT value `id`, test asserts `manager_id`).
  - `patch_cannot_rename_user_name_or_external_id` passes **vacuously** (no operations applied at all) — a test-inflation hazard for exactly the immutability behavior it claims to prove.
  - Coverage row 10's claim "PATCH /Users/{id} implements RFC 7644 PatchOp + simplified bare-body form" is inaccurate for the PatchOp half.

  **Repair path (small, owned by task-265's revision round):** add `#[serde(alias = "Operations")]` to the `operations` field (or rename it explicitly), then actually execute the deferred suite — `cargo test -p gyre-server --test scim_lifecycle` must pass, including the PatchOp-form tests, before this task can claim the PATCH acceptance criterion.

### Verifier must run (deferred from this sandbox — build wrapper blocks gyre-server codegen under the 4 GiB budget)

1. `cargo test -p gyre-server --test scim_lifecycle` — will fail on F1 as shipped; must pass after the alias repair.
2. `cargo test -p gyre-server --lib scim::` — unit tests incl. the updated `scim_delete_user` deprovision semantics (GET after DELETE = 200 + `active:false`).
3. `cargo test -p gyre-server --lib auth::` and `--lib deactivate_user_cascades` — the auth.rs credential_revoked reorder and the admin-path cascade must stay green.
4. Re-run `scripts/check-abac-route-registry.sh` / `check-abac-exempt-handlers.sh` after any repair (they are green on this candidate).

### Non-blocking observations

- The task's `commits:` frontmatter lists only `156d268f`; the two later commits on the branch (`3cf47643` docs, `efbde16d` process) are non-product commits, consistent with the attribution gate.
- No frontend changes; generated `web/dist/` untouched — nothing to observe there.
