# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-11-09
**Verdict:** approved

**Candidate under review:** `4932fdbe9b79f592f90b28dbacf31bae82d86e20` (assigned base `fefde2946f3a0af1f212588f54591516fee0f7b2`)
**Spec:** `identity-security.md` §Impersonation; `observability.md` §Audit Event Types
**Task commits:** `cad88393d` (implementation), `2d511c216`, `a1194a81a`, `14af7abd7` (repairs) + documented merge-repair commits

---

## Verification performed

Read the full base→candidate diff (26 files, +3222/−19) and the new sources in full: `impersonation.rs` (core, 917 ln), `api/impersonation.rs` (handlers + tests, 1609 ln), the `auth.rs` mint/validate/context additions, `abac_middleware.rs` attribution + route-registry entries, `lib.rs` `require_auth_middleware` branch + `ws_connection_users`, `middleware.rs` last_seen marker, `ws.rs`/`tty.rs` connection-time attribution, `jobs.rs` + `main.rs` expiry sweep, `api/audit.rs` re-attribution, enum unions in `notification.rs`/`audit.rs`/`siem.rs`, and both exemption-file changes. Verified router layering (`require_auth` → `last_seen` → `abac` on the `/api/v1` merge only; git HTTP / MCP / explorer / ws-ticket outside it, matching the per-handler attribution design).

Focused probes (evidence: `/tmp/stage/review-evidence/task-108-*.txt`):

- `cargo test -p gyre-server --lib api::impersonation` — 14/14 (full production-router flow, per-handler surfaces, concurrency, non-target probing, sweep expiry)
- `cargo test -p gyre-server --lib` — **1499/1499** (merge-repair regression across auth/mcp/siem/tty/ws/jobs/middleware/mem)
- `cargo test -p gyre-server --lib auth::` — 45/45; `abac_middleware` 12/12; `jobs::` 6/6; `siem::` 9/9
- `cargo test -p gyre-domain` — 436/436; `gyre-common` notification — 4/4
- All 17 mechanical guards green (fabricated-scope, abac-route-registry, dead-message-kinds, in-memory-state-stores, scope-literal-defaults, mem-port-contracts, migration-versions, arch, mcp-write-tools, inert-enforcement, lossy-secret-conversion, forwarded-header-trust, unbounded-external-http, fail-open-ref-resolution, byte-slice-truncation, relative-path-defaults, forged-scope-fields)

**Fresh mutation probes (both caught by the suite; sources restored, `git status` clean, re-run green):**

1. Removed the `session.ended` revocation check in `resolve_impersonation_auth` → `end_session_revokes_token_and_emits_ended` fails (200 vs 401). Token revocation before `exp` is genuinely enforced.
2. Disabled the post-evaluation attribution block in `abac_middleware` → `impersonation_works_through_production_router` fails (1 vs 2 audited events). The audit-trail attribution requirement is genuinely tested.

## Spec conformance

- **Target consent (code or explicit approval)**: one-time 8-digit CSPRNG code, SHA-256-hashed at rest, constant-time compare, delivered **only** to the target (Inbox notification + WS push); admin-facing response omits the code. Approve/deny enforce caller == target before any status is revealed (403 for non-targets, verified by `non_target_cannot_probe_request_status`).
- **Notification**: `ImpersonationRequest` (code delivery) and `ImpersonationAlert` ("Admin {name} is impersonating your account") at session start; WS pushes matched by server-verified user identity (`ws_connection_users`). Email not implemented — plan-conditional ("if email is configured"), no email infrastructure exists, spec section does not require it. Accepted.
- **Full audit trail attributed to the impersonator**: `ImpersonationStarted`/`ImpersonationEnded` with the observability-spec detail fields (approval_token = consumed code's hash — raw single-use code never persisted); every authenticated call under the token recorded as `impersonation_action` attributed to the impersonator across all surfaces (evaluated/exempt/unresolved ABAC, per-handler extractors, WS/TTY connections) with exactly-once per request via a request-extensions marker; `actions_taken` counted atomically under a session-write lock; agent-pushed audit events re-attributed. Mutation-verified by this review.
- **Token**: EdDSA JWT, `sub` = target (permissions match), `impersonator_id`/`session_id`/`impersonating: true` always present; TTL 900 s default via `GYRE_IMPERSONATION_TTL`; zero-leeway `exp`; session-record revocation on explicit end (mutation-verified); accepted by `require_auth_middleware`, `FromRequestParts`, and the WS auth path (full-router test proves the production chain).
- **Authorization**: Admin role + real user identity only (system token rejected); Admin targets prohibited (no SuperAdmin in the role model — plan's "or is prohibited" resolves to prohibited); max 1 active session per admin enforced under the shared write lock (concurrency test with a yielding kv adapter); no impersonation chaining; cannot impersonate self.
- **Auto-expiry**: validation-time rejection (zero leeway) + `impersonation_expiry` sweep job (30 s) emitting `ImpersonationEnded` with duration and action count; stale pending requests expired.

## Task-plan deviations (all spec-over-plan, documented in the shipped notes)

1. Admin response is `{request_id, expires_at}` without `approval_code` — returning the code to the requester would enable self-approve, contradicting the spec's target-consent requirement. Correct resolution; the plan shape is the weaker contract.
2. Server-side attribution from the verified token replaces the client-supplied `X-Gyre-Impersonator` header — strictly stronger (a caller-controlled header is spoofable); the spec section requires attribution, not the header.
3. Approve response additionally carries `session_id` (additive).

## Merge-repair verification (base `fefde2946`)

- `notification.rs`/`audit.rs` enum unions complete: both sides' variants present in enum, `as_str`/`parse`/`from_str`/`default_priority`, round-trip tests — no stranded emitters (dead-message-kinds guard green).
- `siem.rs` CEF severity arms for both sides kept.
- `fabricated-scope-defaults-exemptions.txt` identical to base (3 entries; guard green). `scope-literal-defaults-exemptions.txt` is a pure line remap (9 → 9 entries, no additions).
- The MCP test-helper `impersonation: None` additions are correct for direct (non-impersonated) test auth; full mcp suite covered by the 1499-test run.

## Findings

None. Every acceptance criterion is backed by production code and a mutation-hard test; all focused suites and all 17 mechanical guards pass on the exact candidate `4932fdbe`.
