# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (GLM-5.3, pi runtime)
**Date:** 2026-10-01
**Verdict:** **approved**

**Candidate under review:** `dbd1e8f13a93c9fe1513375c57bad0bf1b2ad2a2` (assigned), base `72ad2a128`
**Commits (task frontmatter):** `cad88393d`, `2d511c216`, `a1194a81a`, `14af7abd7` (+ documented repair/rebase round-trips on the branch)
**Spec:** `identity-security.md` §Impersonation; `observability.md` §Audit Event Types

---

## Verification performed

Inspected the full diff base→candidate (27 files, +3204/−20) and read all new/changed production sources in full: `impersonation.rs` (service), `api/impersonation.rs` (handlers + tests), `auth.rs` (JWT mint/validate, extractor step 1.5), `lib.rs` (`require_auth_middleware` branch, `ws_connection_users`), `abac_middleware.rs` (resolver entries, passthrough/evaluated attribution, exactly-once marker), `middleware.rs`, `ws.rs`, `tty.rs`, `api/audit.rs`, `jobs.rs`, `main.rs`, domain/common types. Verified the approval code delivery path is target-only (notifications are listed solely via `/api/v1/users/me/notifications`, scoped to the authenticated user; WS pushes match the server-verified `ws_connection_users` identity). Verified the lock discipline around `SESSION_WRITE_LOCK` (no nested acquisition: `expire_sessions` releases per-session via `end_session` before its marking loop) and that durable state goes through the `KvJsonStore` port (`store!` wiring — SQLite/PG in DB-backed deployments).

**Focused probes** (evidence in `/tmp/stage/review-evidence/`):
- `cargo test -p gyre-server --lib api::impersonation` — **14/14 green**, including `impersonation_works_through_production_router` (full `build_router` layering: require_auth → last_seen → abac → handler; asserts both evaluated and exempt surfaces are attributed and `actions_taken == 2`, no double-count), `per_handler_surfaces_are_attributed`, `concurrent_approvals_create_exactly_one_session` and `concurrent_impersonated_actions_are_counted_atomically` (yielding KV adapter modeling a real async backend), `non_target_cannot_probe_request_status`, `expired_sessions_auto_end_via_sweep`.
- `auth::` 37/37, `abac_middleware` 11/11, `jobs::` 6/6, `ws::` 82/82, `tty::` 3/3, `api::audit` 12/12, `gyre-common` notification 4/4 — all green.
- Guard scripts: `check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-in-memory-state-stores`, `check-dead-message-kinds`, `check-mcp-write-tools`, `check-inert-enforcement`, `check-task-commit-attribution`, `check-migration-versions`, `check-migration-sql-portability`, `check-forwarded-header-trust` — **all exit 0** on the merged tree. The exemption-file changes are pure line-number remaps of pre-existing entries (frozen counts unchanged; the rebase conflict resolution in `fabricated-scope-defaults-exemptions.txt` leaves 3 entries ≤ frozen 4, no new exemptions).

## Spec-contract assessment

Every §Impersonation requirement is enforced by production code:
- **Target consent before impersonation** — one-time CSPRNG code, SHA-256-hashed at rest, delivered only to the target (Inbox `ImpersonationRequest` + WS push); constant-time compare; per-handler target-identity check before any status is revealed; deny is terminal. The task plan's admin-facing `{request_id, approval_code}` response is **correctly not shipped** — returning the code to the requester would let the admin self-approve, directly violating the spec's consent requirement. Documented deviation; the spec is the contract and the plan's shape contradicts it.
- **Notification on start** — `ImpersonationAlert` (priority 1) persisted + WS push matched by verified identity.
- **Full audit trail attributed to the impersonator acting as the target** — `ImpersonationStarted`/`ImpersonationEnded` with the spec's `detail` fields (`approval_token` = consumed code's hash; `duration_secs`, `actions_taken`), both attributed (`user_id`) to the impersonator; every authenticated call under the token audited as `impersonation_action` across all surfaces (evaluated/exempt/unresolved `/api/v1`, per-handler extractor surfaces, WS/TTY connections) with exactly-once marker discipline; agent-pushed audit events re-attributed with session context.
- **AP2 pattern** — intent (request) → signed mandate (EdDSA JWT, `sub`=target, `impersonator_id` always present) → receipt (audit events + per-action trail).

Constraints: Admin role + real user identity required (system token rejected); Admin targets prohibited (no SuperAdmin in role model — the plan's "or is prohibited" resolves to prohibited); max 1 active session per admin enforced under the shared write lock; no impersonation chaining; TTL via `GYRE_IMPERSONATION_TTL` (default 900 s) with zero-leeway `exp` validation and session-record revocation before `exp` on explicit end; `impersonation_expiry` sweep job (30 s) closes the audit trail on expiry.

## Non-blocking observations (recorded for completeness, no action required)

- On an expired-but-not-yet-swept session, `DELETE .../impersonate` records `reason: "ended_by_admin"` rather than `"expired"` — still an explicit end; the token was already invalid via zero-leeway `exp`.
- Evaluated-route `impersonation_action` audit `detail.path` carries the route pattern (e.g. `/api/v1/tasks`) rather than the concrete URI; exempt/per-handler surfaces carry the concrete path. Attribution (the spec requirement) is unaffected.
- `resolve_impersonation_auth` does not re-check that the impersonator still exists/holds Admin mid-session — the consent was given and the TTL bounds exposure; not specced.
- No cap on concurrent *pending* requests per admin (only active sessions are rate-limited) — an Admin can generate repeated request notifications to a target; nuisance only, and the spec's rate limit is explicitly about active impersonations.
- Email notification is not implemented — no email infrastructure exists in the platform and the spec section does not require it (plan-conditional only); documented.

Full workspace suites, all-target Clippy and remote/CI verification are owned by the verification stage; nothing in this review observed a reason to expect them to fail.

## Verdict

**Approved.** The implementation is real end-to-end (crypto-verified token, port-backed durable state, enforcement at every gate), the tests are adversarial (production-router probe, yielding-adapter concurrency races, status-probe ordering, negative authorization cases), and the one deliberate plan deviation strengthens rather than weakens the spec's consent requirement.
