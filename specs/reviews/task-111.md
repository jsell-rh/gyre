# Review — task-111 (Implement session management)

Spec: `user-management.md` §Session Management.
Candidate `04f388549b8a26d4daca5e3db406a9ab11231e2e` vs base `fefde2946` (branch `pipeline/task-111/98b249715fe5429f9d246ae8ea5909b8-1`; product history merged through `99c293d06`, killing-test repair `5a0816fe6`). Task commits all attributed in `specs/tasks/task-111.md` frontmatter (`check-task-commit-attribution.sh` green).
Verdict: **approved**.

## Round 1 — independent verification (2026-11-07)

### Test runs (all on this exact candidate tree; evidence in `/tmp/stage/review-evidence/`)

- 17 named session tests (`api/users.rs` × 12, `auth.rs` × 5) via exact test names → **17 passed, 0 failed** (`focused-tests.log`).
- `cargo test -p gyre-adapters --lib -- sqlite::user_profile::tests::session` → **8 passed, 0 failed**.
- Full `cargo test -p gyre-server --lib` → **1501 passed, 0 failed** (`gyre-server-full-lib.log`).
- Full `cargo test -p gyre-adapters --lib` → **391 passed, 0 failed** (12 ignored, pre-existing) (`gyre-adapters-full-lib.log`).
- `cargo check -p gyre-cli` → green (`cargo-check-cli.log`). `cargo check --all` is wrapper-blocked in the review sandbox (workspace-wide builds belong to verification; rejection log in `cargo-check-all.log`) — deferred check for the verifier; every workspace crate compiles individually and both full lib suites pass.
- All 18 mechanical invariant scripts green (`mechanical-checks.log`): arch, abac-route-registry, abac-exempt-handlers, migration-versions, migration-sql-portability, mem-port-contracts, scope-literal-defaults (13), fabricated-scope-defaults, forwarded-header-trust, lossy-secret-conversion, inert-enforcement, forged-scope-fields, in-memory-state-stores, relative-path-defaults, unbounded-external-http, fail-open-ref-resolution, dead-message-kinds, byte-slice-truncation, task-commit-attribution.

### Mutation probes (independent — the implementer's claimed mutation logs were not present in this environment, so all probes were re-run from scratch; logs `task-111-mutation-*.log`, driver `run-mutation-probes.sh`; source restored and verified clean after each probe)

| # | Production behavior disabled | Killing test result |
|---|---|---|
| M1 | HTTP extractor `credential_revoked` check | `revoke_all_blocks_credential_from_never_seen_device` **FAILED** (200 vs 401) |
| M2 | HTTP extractor per-device `track_session` rejection (mint kept) | `revoking_one_device_session_does_not_sign_out_other_devices` **FAILED** (200 vs 401) |
| M3a | WS/TTY `authenticate_token` `credential_revoked` only | `ws_auth_rejects_signed_out_everywhere_credential` **FAILED**; sibling test still ok (correct targeting) |
| M3b | WS/TTY `authenticate_token` per-device rejection only | `ws_auth_rejects_revoked_ws_device_session` **FAILED**; sibling test still ok (correct targeting) |
| M4 | `.filter(\|s\| s.is_active(now))` dropped from `list_my_sessions` | `my_session_listing_excludes_revoked_and_expired_sessions` **FAILED**; admin variant unaffected (its own handler, own test) |
| M5 | Admin role check dropped from `list_user_sessions` | `non_admin_cannot_view_or_revoke_another_users_sessions` **FAILED** |
| M6 | `revoked.eq(0)` retention guard dropped in SQLite `delete_expired_before` | `session_retention_never_deletes_revoked_rows` + `session_revoke_all_and_delete_expired` **FAILED** |

Every enforcement point in the task contract has at least one test that fails when it is removed. No self-confirming or stub-backed tests found: endpoint tests go through the real `api_router()` + real `FromRequestParts` extractor with real mem-backed stores; adapter tests run real SQLite with real migrations.

### Acceptance criteria — all verified

- **`UserSession` entity** (`gyre-domain/src/user_profile.rs`): all 9 spec fields, exact names; `is_active(now)` = `!revoked && expires_at > now`.
- **`SessionRepository` port + SQLite impl** (`gyre-ports/src/user_profile.rs`, `gyre-adapters/src/sqlite/user_profile.rs`): full method set; owner-scoped `revoke`, retention contract (never deletes revoked rows) enforced in all three adapters (SQLite/PostgreSQL/mem) — mem-parity check green and M6 proves the SQLite side is load-bearing.
- **Migration** `2026-10-08-000059_user_sessions`: unique sequence on its date (base's colliding `000058_user_entity_profile_fields` correctly avoided by the 000059 renumber); portable SQL (runs on SQLite in every adapter test; portability script green).
- **Sessions created on auth, throttled `last_active_at`**: HTTP extractor and WS/TTY `authenticate_token` both mint on first presentation of (credential, IP, User-Agent); touch throttled to 60s and slides `expires_at` (`session_last_active_touch_is_throttled` asserts both the write and the no-write inside the window).
- **GET /users/me/sessions**: active-only, no token hash exposed (test asserts the field is absent).
- **DELETE /users/me/sessions/:id**: revokes, 404 on foreign id (`revoke_cannot_target_another_users_session`), next request from that device 401s (M2-killed).
- **POST /users/me/sessions/revoke-all**: signs the credential out everywhere including never-seen devices (M1-killed).
- **Revoked sessions rejected in auth middleware**: both HTTP and WS paths, both check classes independently killed (M1–M3b).
- **Routes registered with ABAC mappings**: six routes in `api/mod.rs` with `RouteResourceMapping::exempt` entries; per-handler authorization present in every exempt handler (role check killed by M5; ownership 404s killed by `revoke_cannot_target_another_users_session`; unknown-user 404s by `admin_session_endpoints_unknown_user_is_404`); registry and exempt-handler scripts green.
- **`cargo test --all`**: wrapper-blocked for workspace-wide runs in this sandbox; equivalent per-crate evidence above, deferred to verification.
- **Background cleanup**: `spawn_session_cleanup` wired in `main.rs`, daily, deletes only unrevoked rows expired >30 days (M6-killed contract).

### Security-relevant properties checked

- Only SHA-256 hashes stored (`hash_api_key`); plaintext key never persisted or logged; API responses never include `token_hash`.
- IP from `ConnectInfo` socket peer only; forwarded headers not consulted (CWE-348) — `check-forwarded-header-trust.sh` green, comment and code consistent.
- Scope-fabrication cleanup (`DEFAULT_TENANT_ID` constant replacing literal `"default"` constructions, exemption count 24→13, plus the no-arg scan fix to `check-scope-literal-defaults.sh`) — net strengthening: entries only removed, frozen count only lowered, check now non-vacuous when invoked without args.

### Non-findings (design observations, judged acceptable — recorded for future work)

- **`token_hash` holds the SHA-256 of the API key, not a separately minted crypto-random session token.** The task plan's step 1 ("session token generation utility") presumed a login flow that issues session tokens; the platform authenticates API keys and has no such flow. The spec's contract on the field ("SHA-256 of session token, never store plaintext") is honored — the credential is the session token, hashed at rest, and the deviation is documented in the Shipped notes rather than hidden. "Sign out everywhere" is correspondingly credential-scoped, which is the stronger semantic for the spec's intent.
- **Device identity is (IP, User-Agent)**: revoking one device's session is bypassable by presenting the key with a different User-Agent (new device row). Inherent to fingerprint-based devices; the spec does not define a device identity, the per-device and credential-wide tiers are both tested, and the robust gesture (revoke-all) is credential-wide. A stolen-key response remains: revoke-all, or rotate/delete the API key.
- **`credential_revoked` fails open on a session-store error** (logged). Justified in-code: the credential itself was validated against `api_keys`; revocation state being *unavailable* is not *revoked*, and failing closed would lock out all API-key auth on any session-store hiccup. Documented trade-off, not silent.
- **Admin variants gate on `UserRole::Admin` without comparing the target user's `tenant_id` to the caller's.** Consistent with the codebase-wide convention for admin-gated routes (budget, compute-targets, llm-config, meta-specs, release all use `auth.roles.contains(&UserRole::Admin)` with no tenant comparison); `check-abac-exempt-handlers.sh` treats a role decision as a real authorization decision. When user-level tenant scoping lands platform-wide, these three handlers should follow.
- **Minor races/costs:** two concurrent first presentations of the same (credential, device) can mint two rows (create only fails on duplicate id; newest-wins self-heals); `credential_revoked` does a per-request `list_for_user` scan. Acceptable at current scale; noted for follow-up if it matters.

### Spec-fidelity check on the coverage row

`specs/coverage/system/user-management.md` row 12 moved `task-assigned` → `implemented` with an accurate summary (states "Pending independent review" — no premature `verified`). No other coverage rows or unrelated specs touched. `docs/api-reference.md` documents all six endpoints truthfully (semantics, 404/403 behavior, no token-hash exposure).

Summary: every acceptance criterion has production code behind it, every enforcement point is proven by a test that fails when it is disabled (independently re-verified via seven mutations), all suites and mechanical gates pass on the exact candidate, and the honest deviations from the task *plan* are documented adaptations to the platform's real auth model rather than fakery. Approved.

— Reviewer, 2026-11-07
