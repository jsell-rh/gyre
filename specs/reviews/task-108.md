# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (R2 — repair round)
**Date:** 2026-09-30
**Verdict:** needs-revision

**Candidate under review:** `7d06ac781` (branch `pipeline/task-108/f3d54b356dcc44abad74f3f642c763b1-1`, base `be43a1b7`)
**Commits:** `cad88393d` (implementation), `2d511c216` (test fix), `a1194a81a` (R1 repair)
**Spec:** `identity-security.md` §Impersonation; `observability.md` §Audit Event Types

**Verification performed:** read the full base→candidate diff and all new/changed sources (impersonation.rs, api/impersonation.rs, auth.rs, abac_middleware.rs, middleware.rs, lib.rs require_auth_middleware, ws.rs, tty.rs, api/audit.rs, jobs.rs, notification/protocol/audit type additions); ran `cargo test -p gyre-server --lib api::impersonation` (13/13), plus `auth::` 37/37, `abac` 30/30, `ws::` 82/82, `middleware` 22/22, `api::audit` 12/12, `tty` 3/3, `gyre-domain --lib` 412/412, `gyre-common notification` 4/4; ran all 20 repo mechanical guard scripts individually. Evidence: `/tmp/stage/review-evidence/task-108-r2-evidence.txt`.

---

## R1 repair verification (previous round's findings)

All three R1 findings are genuinely fixed, plus the production defect the repair exposed:

- **[fixed] F1 attribution coverage.** `require_auth_middleware` now has the impersonation-JWT branch (EdDSA + active session), so impersonated `/api/v1/*` calls work through the full production layering — proven by `impersonation_works_through_production_router`, which drives `build_router` (require_auth → last_seen → abac → handler) and asserts both an evaluated and an ABAC-exempt call are audited with `surface` = `abac` / `abac-exempt` and `actions_taken == 2` (exactly-once via the request-extensions marker). Per-handler surfaces (git HTTP, MCP, conversations, explorer, ws-ticket) record at the auth-resolution layer (`per_handler_surfaces_are_attributed`); WS/TTY connections record at connection time. The `?token=` query-param path cannot bypass attribution on `/api/v1` because `require_auth_middleware` requires an `Authorization: Bearer` header, and non-`/api/v1` surfaces have no pre-set marker, so the extractor records them.
- **[fixed] F2 approve ordering.** `approve_request` checks `req.target_user_id != caller_user_id` before any status/expiry is revealed; `non_target_cannot_probe_request_status` covers both approved and denied requests (403, not 409).
- **[fixed] F3 atomic `actions_taken`.** The increment and session end are serialized by `SESSION_WRITE_LOCK`; `concurrent_impersonated_actions_are_counted_atomically` uses a yielding KV adapter and asserts 8/8 strictly sequential counts — a test that genuinely models the interleaving the sync mem adapter cannot reproduce.

The implementation itself remains solid against the spec: approval code delivered only to the target (hashed at rest, constant-time compare, never returned to the requesting admin — the documented deviation from the plan's response shape is correct and spec-preserving), `ImpersonationStarted`/`Ended` with the spec detail fields and impersonator attribution, target notified in-app + WS push by verified identity, token revocation on end, zero-leeway expiry, Admin-only request/end with per-handler defense in depth, Admin-target prohibition, no chaining, restart-survivable KV persistence, expiry sweep job.

## Findings (this round)

- [-] **F4: `check-fabricated-scope-defaults.sh` fails on the candidate — stale exemption line remap.** The pre-existing exempted site `claims.tenant_id.as_deref().unwrap_or("default")` sat at `auth.rs:945` when commit `f57ffefcb` remapped its exemption (correctly at the time, and still correct through `cad88393d`/`a1194a81a`). The final base merge (`baaebf558`, merging `be43a1b7`) inserted 38 lines above the site, shifting it to `auth.rs:983`; the exemption file still says 945. Reproduce on the candidate: `./scripts/check-fabricated-scope-defaults.sh` → exit 1, `FAIL: ... crates/gyre-server/src/auth.rs:983: let tenant_id = claims.tenant_id.as_deref().unwrap_or("default")`. The script header states it is "Run by pre-commit and CI", so this candidate breaks CI, and the shipped-notes claim that this guard passes is false for the final candidate (it was true before the merge). The base passes (site and exemption both at 773), so the failure is introduced by this branch. The flagged *site* is pre-existing legacy debt, not a new violation — the fix is to re-remap the exemption entry to line 983 (the exemption count is frozen at 5, so it must be an edit of the existing entry, not an addition).

- [-] **F5 (minor): `approve_request` session-creation TOCTOU bypasses the max-1-active-session rate limit.** The critical section in `approve_request` (Pending-status check → `active_session_for_admin` check → `save_session` → `req.status = Approved; save_request`) is a read-modify-write with no lock, while the sibling session RMWs this round serialized (`record_impersonated_action`, `end_session`) do hold `SESSION_WRITE_LOCK`. Two concurrent `POST /api/v1/admin/impersonate/:request_id/approve` calls from the target user (both loading the request as Pending before either persists Approved) both pass the rate-limit check and both create and save sessions, minting two live tokens for the same admin — violating the task plan §6 constraint "Rate limit: max 1 active impersonation per admin at a time" and emitting two `ImpersonationStarted` events for one consent. `kv_set` is an unconditional put, so nothing at the store level backstops it. Narrow window (requires the consenting target to double-submit concurrently), so severity is minor, but the constraint is explicitly required and the fix pattern already exists in this file: hold `SESSION_WRITE_LOCK` across the status-check→session-creation→request-status-save section (and cover `create_request`'s rate-limit read the same way).

## Non-findings (checked, acceptable)

- Response shape omits `approval_code` for the requesting admin: deliberate, documented, and *required* by the spec's target-consent rule; returning the code would let the admin self-approve.
- `X-Gyre-Impersonator` client header replaced by server-side attribution from the verified JWT claim — strictly stronger than the plan's design.
- Email notification not implemented: no email infrastructure exists in the codebase; the spec section does not require it (plan conditioned it on "if email is configured").
- Admin-target impersonation prohibited (no SuperAdmin in the role model) — the plan's "requires SuperAdmin **or is prohibited**" disjunction resolves to prohibited.
- Tenant handling: the impersonated principal carries the session tenant (captured from the admin at request time), so impersonation grants no scope expansion beyond the target's identity; per-handler tenant checks on the target's resources still apply.
- Exemption-file changes otherwise: `scope-literal-defaults-exemptions.txt` and the `fabricated-scope-defaults` remap are line-number remaps of pre-existing entries only (no new exemptions; `check-scope-literal-defaults.sh` passes).
