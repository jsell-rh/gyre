# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-09
**Verdict:** approved

**Candidate:** `254a3723964264797103f15774fc720fef71e0ec` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`)
**Spec:** `identity-security.md` §Impersonation; `observability.md` §Audit Event Types

## Verification performed

- Read the full task diff (26 files, +3239/−31): `impersonation.rs` module, `api/impersonation.rs` handlers+tests, `auth.rs` (EdDSA impersonation JWT mint/validate, `ImpersonationContext`, extractor step 1.5), `abac_middleware.rs` (attribution on evaluated/exempt/unresolved routes, exactly-once marker), `ws.rs`/`tty.rs` (connection-time attribution, verified-identity push map), `require_auth_middleware` branch, `jobs.rs`+`main.rs` sweep, domain/common enum additions, docs, coverage row.
- **Static guards (15/15 green):** arch, abac-route-registry, abac-exempt-handlers, scope-literal-defaults, fabricated-scope-defaults, dead-message-kinds, in-memory-state-stores, inert-enforcement, forwarded-header-trust, forged-scope-fields, byte-slice-truncation, lossy-secret-conversion, mem-port-contracts, migration-versions, unbounded-external-http, plus task-commit-attribution.
- **Focused tests (all green):** `api::impersonation` 14/14, `auth::` 45/45, `abac_middleware` 12/12, `ws::` 89/89, `jobs::` 6/6, `gyre-domain` audit 8/8, `gyre-common` notification 4/4.
- **Mutation probes (source restored byte-identically, hash `a9274a60…` verified, tree clean):**
  1. Disabling the approval-code constant-time check in `approve_request` → `only_target_with_code_can_approve` FAILS (wrong-code approve would mint a token). The consent gate is load-bearing.
  2. Disabling the target-identity check in `approve_request` → `non_target_cannot_probe_request_status` and `only_target_with_code_can_approve` both FAIL. The per-handler authorization on the ABAC-exempt approve route is load-bearing.
- **Exemption hygiene:** branch entries are a strict subset of the base's (22→13, 0 added); the nine deleted entries are the pre-existing task-099 F1 legacy sites, re-expressed via the check script's own documented `// scope-literal:ok` inline-marker mechanism (check-scope-literal-defaults.sh line 26) — no new exemptions, ceiling untouched.
- **rustfmt changed-line gate** vs base: clean (21 files).
- **Code-delivery confidentiality:** verified no admin-readable surface exposes another user's notifications (`get_my_notifications` lists by caller's own user id; dismiss/resolve check ownership) — the approval code reaches the target only.
- **ws-ticket attribution claim:** `WsTicketStore.issue/consume` round-trips the full `AuthenticatedAgent` including `ImpersonationContext`, so the "ws-ticket" surface recording in extractor step 1 is real.

Evidence: `/tmp/stage/review-evidence/` (guard outputs `g-*.txt`, test runs `t-*.txt`, mutation probes `mutation-*.txt`, exemption subset check, rustfmt-diff).

## Assessment against acceptance criteria

All ten criteria verified with real behavior through the production router (`build_router`): request flow, target-only code delivery (Inbox `ImpersonationRequest` + WS `NotificationPush` matched by server-verified user identity), approve/deny with per-handler target authorization checked before any status is revealed, EdDSA token with `sub`=target / `impersonator_id` / `session_id` claims and zero-leeway `exp`, `ImpersonationStarted`/`Ended` audit events with the spec's detail fields attributed to the impersonator, `impersonation_action` attribution on every authenticated surface (evaluated/exempt/unresolved `/api/v1`, git HTTP/MCP/conversations/explorer/ws-ticket per-handler, WS/TTY connections) with exactly-once counting via a request-extensions marker, immediate revocation on explicit end (session-record check before `exp`), TTL expiry sweep job, Admin-only request/end with defense in depth, max-1-active-session, no self/Admin targets, no impersonation chaining.

## Documented deviations (accepted)

- Response omits `approval_code` (plan shape was `{request_id, approval_code}`): returning the code to the requesting admin would let the admin self-approve, defeating the spec's target-consent requirement. The code is delivered to the target only — this deviation *improves* spec fidelity.
- Email notification not implemented: the platform has no email infrastructure at all and the spec section does not require it (task plan conditioned it on "if email is configured").
- Server-side attribution from the verified token replaces the plan's client-supplied `X-Gyre-Impersonator` header — strictly stronger (caller-controlled header vs cryptographically verified claim).

## Non-blocking observations

- `deliver_approval_code` skips the in-app notification when the target has no workspace membership (logged); such a request can never be approved — fail-safe.
- Session read-modify-write serialization is a single-process lock (documented; the kv store remains the record of truth).
- On per-handler/exempt surfaces the action event's outcome reflects successful authentication, not the handler's response — honestly documented in code comments and the coverage note.

## Deferred to verification

`cargo test --all` (workspace-wide) and the clippy changed-line gate (the sandbox wrapper rejects workspace-wide builds here; rustfmt changed-line gate ran clean). The last verification round's clippy findings were repaired in `64536dbcc`.
