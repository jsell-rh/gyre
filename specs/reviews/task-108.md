# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-10-01 (R1)
**Verdict:** approved

**Candidate:** `7eaa7d1fbef7a9c4713fe12fe380482a48a60ae4` (base `c5deca801`, merge-repair head)
**Commits under review:** `cad88393d`, `2d511c216`, `a1194a81a`, `14af7abd7` (all verified ancestors of the candidate; the two later merge/process commits carry no product-surface changes)
**Spec:** `identity-security.md` §Impersonation + `observability.md` §Audit Event Types

---

## Verification performed

- Read the full diff base→candidate (`git diff c5deca801..7eaa7d1f`): `impersonation.rs` (service, 917 lines), `api/impersonation.rs` (handlers + 14 tests), auth.rs (mint/validate/ImpersonationContext, FromRequestParts + authenticate_token + require_auth_middleware branches), abac_middleware.rs (resolver entries + attribution for evaluated/exempt/unresolved routes), ws.rs/tty.rs (connection-time attribution + verified-user push registry), jobs.rs/main.rs (`impersonation_expiry` job registered and spawned), domain/common event+notification types, api/audit.rs re-attribution, docs, coverage matrix row.
- Ran all 19 mechanically-enforced guards on the candidate tree (arch, abac-route-registry, abac-exempt-handlers, fabricated-scope-defaults, scope-literal-defaults, in-memory-state-stores, migration-versions, migration-sql-portability, dead-message-kinds, byte-slice-truncation, fail-open-ref-resolution, inert-enforcement, mem-port-contracts, forwarded-header-trust, lossy-secret-conversion, relative-path-defaults, unbounded-external-http, mcp-write-tools, task-commit-attribution) — all pass. The two exemption-file diffs are pure line remaps (line shifts from inserted code), no entries added, counts not raised.
- Focused tests on the exact candidate (via `/tmp/stage/bin/cargo`): `api::impersonation` 14/14, `auth::` 44/44, `abac_middleware` 11/11. Evidence: `/tmp/stage/review-evidence/probe-impersonation-tests.txt`, `probe-auth-jobs-abac-tests.txt`.
- **Independent mutation probes** (source restored after each, tree verified clean):
  - Disabling the post-policy-evaluation attribution block in `abac_middleware` → `impersonated_calls_are_attributed_and_counted` FAILS (evidence: `mutation-a-attribution-disabled.txt`). The attribution test is not self-confirming.
  - Removing the `session.ended` revocation check in `resolve_impersonation_auth` → `end_session_revokes_token_and_emits_ended` FAILS (evidence: `mutation-b-revocation-disabled.txt`). Token revocation on explicit end is genuinely enforced by a killing test.
  - Post-restore rerun of the full impersonation suite: 14/14 green.

## Spec-contract assessment

- **Target consent (code/explicit approval):** one-time 8-digit CSPRNG code, only its SHA-256 stored on the request; raw code delivered exclusively to the target (user-scoped Inbox notification `ImpersonationRequest` + WS `NotificationPush` matched by server-verified user identity). The admin-facing response deliberately omits the code — the documented deviation from the task plan's response shape is correct: returning the code to the requester would defeat the spec's target-consent requirement. Verified: admin cannot read the target's notifications (`/users/me/notifications` is auth-user-scoped), and non-target approvers get 403 before any status or code check is revealed.
- **Notification to impersonated user:** `ImpersonationAlert` (priority 1) persisted + WS push when online. Email honestly not implemented (no email infrastructure exists in the platform; the spec section does not require it — only the task plan mentions it, conditionally).
- **Full audit trail with attribution:** `ImpersonationStarted`/`ImpersonationEnded` with the spec's exact detail fields, both attributed (`user_id`) to the impersonator; `approval_token` = consumed code's hash (raw single-use code never persisted outside the target-scoped notification). Every authenticated call under the token is recorded as `impersonation_action` attributed to the impersonator and counted in `actions_taken` — evaluated `/api/v1` routes after policy evaluation with the decision outcome, exempt/unresolved routes before the passthrough, per-handler surfaces (git HTTP, MCP, conversations, explorer, ws-ticket) at the auth-resolution layer, WS/TTY connections at connection time — exactly once per request via a request-extensions marker (the production-router test asserts `actions_taken == 2` for two calls across the require_auth → last_seen → abac → handler chain). `POST /api/v1/audit/events` re-attributes agent-pushed events. Server-side verified-token attribution replaces the plan's client-supplied `X-Gyre-Impersonator` header — strictly stronger.
- **Token:** EdDSA JWT, `sub` = target (permissions match), `impersonator_id`/`session_id`/`impersonating` claims, TTL from `GYRE_IMPERSONATION_TTL` (default 900 s), zero-leeway `exp` validation, session-record revocation on explicit end. Accepted at all three auth entry points (middleware, extractor, WS `authenticate_token`) — the production-router test proves the full layering, not just ABAC-layered test routers.
- **Authorization:** Admin role + real user identity required (shared system token rejected); Admin targets prohibited (no SuperAdmin in the role model — the plan's "or is prohibited" disjunct, documented); max 1 active session per admin, enforced under a session-write lock shared with the approval critical section (concurrent-approval test with a yielding KV adapter proves exactly one session for one consent); no impersonation chaining (impersonated callers cannot request/approve/deny).
- **Tests:** meaningful and mutation-verified; concurrency tests use a yielding KV adapter to model real async interleaving.

## Non-blocking observations

- Approve attempts are not rate-limited per request; only the target can call approve (403 otherwise) and the code grants only what the target can already grant, so there is no brute-force threat surface from other principals.
- A target with no workspace membership cannot receive the approval code (request is created but unapprovable; logged). Honest, logged limitation.
- On exempt/per-handler surfaces the action outcome reflects successful authentication, not the handler's response — documented in-code and in the coverage row.

**Conclusion:** the implementation satisfies every requirement of `identity-security.md` §Impersonation and the observability audit-event contract with real production code (real crypto, KV-persisted sessions/requests, real audit emission and per-call attribution). Approved.
