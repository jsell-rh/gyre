# Review: TASK-110 — Implement tenant & workspace invitation flow

**Reviewer:** Independent review
**Date:** 2026-10-10
**Verdict:** needs-revision

**Candidate under review:** `9568c8f2` (base `8c2d1775`)
**Spec:** `user-management.md` §Tenant-Level User Onboarding, §Workspace Invitation Flow, §Invitation Expiry, §Tenant Invitations
**Evidence:** `/tmp/stage/review-evidence/` (test logs with exit codes, probe source, evidence log `task-110-review-evidence.md`)

---

## Verification performed

Read the full diff and every new/changed source file (domain, ports, sqlite+postgres+mem adapters, migration 000056, API handlers, route registration, ABAC mappings, lib.rs wiring, main.rs job spawn, docs). Ran:

- `cargo test -p gyre-server --lib api::invitations` — 9/9 passed, exit 0
- `cargo test -p gyre-adapters invitation` — 2/2 passed, exit 0 (real temp-file SQLite through migration 000056)
- `cargo test --offline -p gyre-domain --lib invitation` — 4/4 passed, exit 0
- `cargo check -p gyre-server --bins` — clean, exit 0
- 20 mechanical checks — 19 exit 0; the one non-zero (`check-task-commit-attribution`) flags `a781ede2` (task-210), proven an ancestor of the **base** commit, i.e. pre-existing upstream; this lineage adds no new violations and `git diff base..HEAD -- scripts/` is empty
- A purpose-built round-trip probe against the real `SqliteStorage` (source preserved at `task-110-probe-source.rs`, file removed from the repo after the run; tree clean)

No TCP listener is bindable in this sandbox, so the live HTTP smoke was not run; exact host/CI steps are recorded in the evidence log. That restriction is infrastructure, not a code finding.

**What is genuinely good here:** the domain model matches the spec struct field-for-field; tokens are 32-byte CSPRNG with SHA-256-at-rest (known-answer test included); single-use and expiry transitions are real; the pending-duplicate port contract is enforced in all three adapters; bulk invite has real partial-success semantics; the expiry job marks rather than deletes; management routes carry ABAC mappings plus per-handler TenantAdmin/Owner-Admin and tenant containment checks; migration numbering (000056) and SQL portability are correct.

## Findings

- [x] **F1 (critical, blocks approval): `user.tenant_id` / `user.global_role` are not persisted by the DB adapters, so the workspace invitation flow is dead in every DB-backed deployment.** The new `invite_to_workspace` (api/invitations.rs:726-739) requires the invitee's `user.tenant_id` to match the workspace's tenant and rejects with 403 when it is `None` ("has no tenant scope"). But the `users` table has no such column (schema.rs:172-183; migrations 000001/000036), and `UserRow`/`UserRecord` in sqlite/user.rs and postgres/user.rs neither read nor write `tenant_id` or `global_role`. **Reproduced:** a probe creating a user with `tenant_id = Some("default")` through the real `SqliteStorage` and reading it back via `find_by_external_id` returned `tenant_id: None` (probe FAILED, exit 101 — log `task-110-user-tenantid-roundtrip-probe.txt`). With `GYRE_DATABASE_URL` set (the documented persistent mode, docs/server-config.md), every stored user has `tenant_id = None`, hence every workspace invite 403s — the §Workspace Invitation Flow coverage target does not function on SQLite or PostgreSQL. The invited `GlobalRole` set at invitations.rs:525 is likewise dropped on reload (users.rs reports `Member`). The mem adapter clones the entire struct, which is why all 9 API tests pass; single-process in-memory tests cannot catch this. Fix: add the columns via migration and wire them through both adapters' `UserRow`/`UserRecord`/`From<UserRow>`, or stop keying containment on a field no adapter persists.
- [x] **F2 (code): the SSO-mode linking claim is false as implemented.** Doc comment invitations.rs:470-473 says an accepted local-mode account "will merge with the SSO subject on first login via the existing find_or_create_user path only if emails match." `find_or_create_user` (auth.rs:797-822) resolves by `external_id = claims.sub` only and never matches by email — first SSO login creates a second user (Keycloak `sub` external_id), so the invitation's pre-assigned memberships and granted role never attach to the SSO account. Either implement the email-based link within the tenant or correct the documented behavior.
- [x] **F3 (code, minor): per-tenant invitation policy is unreachable.** `load_policy` reads kv namespace `invitation_policy`, but nothing in the repo ever writes it (zero writers: no endpoint, no env var, no admin surface). The spec's "Configurable per-tenant" expiry/cap values therefore always resolve to the hardcoded defaults; only the per-request `expires_in_days` override is live. The configurability requirement is not actually delivered, only its defaults.

## Host verification steps (recorded for CI at the PR head)

1. `cargo test --all`
2. Live smoke with dev token: invite → 201 with `invite_url`; accept → 200 `mode=created`; re-use → 409; revoke pending → 204 and accept → 409; bulk with 2 good + 2 bad → `created_count=2, error_count=2`; `GET /api/v1/tenant/invitations?status=Expired`.
3. The failing case this review found: start with `GYRE_DATABASE_URL=sqlite://gyre.db`, onboard a user via invite/accept, then `POST /api/v1/workspaces/{id}/invite {"user_id":"<user>"}` — currently 403 "has no tenant scope"; must be 201 after the adapter fix.

**Verdict: needs-revision.** The implementation is substantially real and well-tested at the unit level, but F1 makes a coverage-targeted flow (workspace invitations) non-functional in DB-backed deployments — precisely the class of gap (adapter contract split invisible to in-memory tests) this project's rules exist to catch. F1 must be fixed and re-verified (round-trip probe or an equivalent DB-backed integration test) before approval; F2 should be fixed or the comment corrected; F3 should either gain a write path or be documented as defaults-only.
