# Review: TASK-108 — Admin User Impersonation

**Reviewer:** Independent review agent
**Date:** 2026-11-09
**Comparison base:** 77473763887f249f71d6402d4fc712d51bd247c5
**HEAD (candidate):** 264ece0d7cd8de3ec6f752147c81030e8b00d559
**Verdict:** approved

---

## Scope

Diff vs base: 25 files, +3239/−10. Core: `gyre-server/src/impersonation.rs` (flow,
sessions, audit, notifications), `api/impersonation.rs` (4 routes + 14 tests),
`auth.rs` (ImpersonationJwtClaims/ImpersonationContext, mint/validate, chain
integration in `FromRequestParts`, `authenticate_token`, `require_auth_middleware`),
`abac_middleware.rs` (route mappings + action attribution on all surfaces), plus
`ws.rs`/`tty.rs` connection attribution, audit-event/notification/WS-message enum
variants, `impersonation_expiry` job, docs, and the coverage row
(identity-security.md §12 → implemented).

Since the previously reviewed head `f7e7601f2`, only `specs/tasks/task-108.md`
process bookkeeping changed (commit list + shipped-note references).

## Findings

None blocking.

### Spec contract (identity-security.md §Impersonation)

- **Target consent before impersonation:** one-time 8-digit CSPRNG code, only its
  SHA-256 hash persisted, raw code delivered exclusively to the target via Inbox
  `ImpersonationRequest` notification + WS push. The admin-facing response omits
  the code (documented, justified deviation from the task plan's response shape —
  returning it would let the admin self-approve). Approve requires the target's
  verified user id (checked before any status is revealed — no consent-state
  probing by non-targets) plus a constant-time code match. Explicit deny path
  exists; denied/expired requests cannot be approved.
- **Target notified:** `ImpersonationAlert` (priority 1) on session start.
- **Full audit trail, attributed to the impersonator:** `ImpersonationStarted`/
  `ImpersonationEnded` with the spec's detail fields (`impersonator_id`,
  `target_user_id`, `approval_token` = consumed code's hash; `duration_secs`,
  `actions_taken`); every authenticated call under the token recorded as
  `impersonation_action` attributed to the impersonator across all surfaces
  (evaluated ABAC routes with the decision outcome, exempt/unresolved routes,
  per-handler surfaces, WS/TTY connections), exactly-once per request via a
  request-extensions marker; `POST /audit/events` re-attribution with session
  context. Server-side attribution from the verified token replaces the plan's
  caller-controlled `X-Gyre-Impersonator` header — strictly stronger.
- **AP2 pattern:** intent (request) → signed mandate (EdDSA JWT, `sub` = target,
  `impersonator_id` always present, zero-leeway `exp`, session-record revocation
  on end) → receipt (audit events).

### Constraints

Admin role + real user identity required (shared system token rejected);
impersonating Admin users prohibited (no SuperAdmin in the role model — the plan's
"or is prohibited" branch); max one active session per admin (409); no
impersonation chaining; cannot impersonate yourself; TTL sweep job every 30 s.

### Independent probes (evidence: /tmp/stage/review-evidence/task-108-review-264ece0d.md)

- Focused suites all green on the exact candidate: `api::impersonation` 14/14,
  `impersonation` (incl. JWT round-trip/expiry) 15/15, `auth::` 47/47,
  `abac_middleware` 12/12, `ws::` 90/90, `mcp::` 85/85, `jobs::` 6/6, `siem::` 9/9,
  `api::audit` 12/12, `api::users` 17/17, `tty` 3/3, `gyre-common` 116/116,
  `gyre-domain` 442/442.
- Invariant guards all exit 0: abac-route-registry, abac-exempt-handlers, arch,
  dead-message-kinds, in-memory-state-stores, scope-literal-defaults,
  fabricated-scope-defaults, mem-port-contracts, inert-enforcement,
  task-commit-attribution.
- **Mutation-verified test strength** (source restored after each probe,
  `git status --porcelain` clean):
  1. Disabling the session-active check in `resolve_impersonation_auth` →
     `end_session_revokes_token_and_emits_ended` FAILS (200 vs 401) — revocation
     is genuinely asserted.
  2. Disabling the target-identity check in `approve_request` →
     `only_target_with_code_can_approve` and `non_target_cannot_probe_request_status`
     both FAIL — per-handler authorization is genuinely asserted.
- All 6 attributed commits exist on the branch; base is an ancestor of the
  candidate; the two newer commits are `process:`-prefixed non-product commits.

### Non-blocking observations

- `require_auth_middleware` resolves impersonation auth and discards it (the
  extractor resolves it again) — a redundant session lookup per request, no
  correctness impact.
- `users.find_by_id` is not tenant-scoped, so a cross-tenant target can be asked
  to consent; the resulting principal is scoped to the admin's tenant with the
  target's non-Admin roles and requires the target's explicit consent — no
  privilege above the admin's own, and the spec imposes no tenant constraint here.
- Email notification is not implemented: no email infrastructure exists in the
  codebase and the spec section does not require it (task-plan-only, conditional).
  Honestly documented in the task notes and the coverage row.
