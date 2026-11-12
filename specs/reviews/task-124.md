# Review — task-124 (User management + workspace membership API endpoints)

Spec: `specs/system/user-management.md` §API > User Management, §API > Workspace Membership.
Candidate under review: `7fbbcb7e105f8689b2eac5d9dcd4b84e4eb2343e` (base `199c1bb78f53f27088c54a0929b5605e7302e874`).
Product commits: `33e30ac96` (endpoints), `6033324f8` (impersonation deactivation repair from the prior round's finding); docs/process: `ab46831c1`, `df196bf67`, `0170fe9bc`, `785bf6537`, `7fbbcb7e1`.
Verdict: **needs-revision** — one finding (F1, a false coverage claim in the task's Shipped record); the shipped code itself verified clean in every probe run this round.

## Round 1 (2026-11-13)

Probes (evidence: `/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → exit 0. gyre-server test compilation deferred to the verifier per the 4 GiB budget rule; the verifier must run `cargo test -p gyre-server --lib api::users`, `cargo test -p gyre-server --lib api::impersonation`, and `cargo test --all`.
- `cargo test -p gyre-domain user` → 12 passed (incl. `deactivate_stamps_once_and_reports_state`).
- `cargo test -p gyre-adapters --lib sqlite::user` → 16 passed (incl. `deactivation_persists_and_round_trips`).
- All 16 mechanical invariant scripts → OK (ABAC route registry, migration versions + SQL portability, arch, exempt handlers, and the full frozen-exemption family).

Verified working (no findings):

- **All new endpoints are real implementations on real storage.** `GET /users/{username}` (tenant-public profile with memberships; cross-tenant handle → 404, no existence leak), `GET /users` (TenantAdmin-only per-handler check with stored-record fallback, tenant-scoped, limit/offset pagination), `PUT /users/{id}/deactivate` (soft deactivation persisted via migration `2026-11-13-000062_user_deactivated_at` — next unused sequence, portable SQL; self-deactivation → 409; cross-tenant target → 403; audit-logged with reason), workspace invitation list/accept/revoke on the real pending-membership lifecycle (accept is invitee-only — membership looked up by (caller, workspace); revoke deletes only pending rows of that workspace; both audit-logged).
- **The deactivation cascade is real, not a flag flip.** Auth rejection on the API-key extractor path, the WebSocket `authenticate_token` path, and the SSO `find_or_create_user` path (the user-JWT path routes through it); `workspace_ownership::has_active_owner` and the users.rs last-Owner guards treat a deactivated Owner as departed, and `deactivate_user` re-tracks ownership inline (ownerless record + TenantAdmin notification).
- **The prior round's impersonation finding is properly repaired.** `resolve_impersonation_auth` now rejects when the target OR impersonator account is deactivated or the impersonator record is missing (fail-closed `??`), reached from all three pre-API-key call sites; `deactivate_user` additionally ends active impersonation sessions (impersonator or target) with reason `user_deactivated`. Three regression tests cover target-deactivation, impersonator-deactivation, and a resolver-only test that deactivates the user record directly (session left un-ended and unexpired) so only the resolver check can produce the 401.
- **ABAC wiring is sound** (verified against the builtin policy set, since `api_router()`-based tests bypass the middleware): `subject.global_role` derives from `auth.roles.first()`; Admin(900)/Developer(800)/Agent(700)/ReadOnly(600) make the read routes middleware-passable for members with handler-level TenantAdmin enforcement, PUT deactivate is middleware-denied for ReadOnly, and the invitation routes inherit the exact resource/action mappings of the pre-existing member endpoints (`write`/`delete` on `workspace_member`, Owner/Admin via builtin 810). `/invite/accept` is resolver-exempt with genuine per-handler invitee-only authorization.
- Tests are hard, not self-confirming: 403/404/409 negative paths, pre-conditions proving tokens/keys work before the cascade, post-condition storage assertions (kv ownerless record, notification, audit events, membership rows), and idempotence checks.
- Coverage matrix updated honestly: row 27 → implemented (all seven workspace-membership endpoints live); row 26 stays task-assigned with the three session endpoints explicitly recorded as open.

Findings:

- [F1, blocking — false verification claim in the task record] `specs/tasks/task-124.md` §Shipped > "Session endpoints" states: *"Verified covered by task-111 (list / revoke individual / revoke all under `/api/v1/users/me/sessions`); no duplication added."* This is false. No `/api/v1/users/me/sessions` routes exist anywhere (`grep -rn "me/sessions\|UserSession" crates/gyre-server/src/api/ crates/gyre-ports/src/ crates/gyre-domain/src/` → zero matches); `specs/tasks/task-111.md` is `progress: not-started`, `commits: []`; and the candidate's own coverage matrix (row 26 of `specs/coverage/system/user-management.md`) records the three session endpoints as still open, owned by task-111. The Shipped record thus claims verified coverage with no production code behind it — exactly the "no fake completion" class the task goal prohibits — and contradicts the candidate's own coverage notes. Note the task's acceptance criteria do not require the session endpoints (they belong to task-111), so the code is not deficient; only the claim is. **Repair (docs-only):** rewrite the Shipped "Session endpoints" paragraph to state the session endpoints are NOT implemented (owned by not-started task-111; user-management §API > User Management stays open over them), consistent with the coverage matrix, and record the repair commit in the task's `commits:` frontmatter.

Test-inflation check: none found. Scope check: no out-of-scope production edits; the only files touched are the endpoint surfaces, auth/impersonation/workspace-ownership cascade points, migration, docs, coverage, and the task file.
