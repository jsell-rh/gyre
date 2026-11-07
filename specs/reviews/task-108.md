# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-10-01
**Verdict:** approved

**Candidate:** `b19455f2de7a3a06d8d69eaef3b863714e521e7e` (base `0f61b0bff7b4843326baf4e9498063b6e9ed5815`)
**Commits under review:** `cad88393d` (implementation), `2d511c216` (test fix), `a1194a81a` (attribution coverage repair), `14af7abd7` (approval critical-section serialization)
**Spec:** `identity-security.md` §Impersonation, `observability.md` §Audit Event Types

**Verification performed:** read the full candidate-vs-base diff (27 files, ~3200 insertions) — impersonation.rs (core + handlers), auth.rs (mint/validate/ImpersonationContext/extractor step 1.5), lib.rs (`require_auth_middleware` branch, `ws_connection_users`, router layering), abac_middleware.rs (resolver entries, evaluated/exempt/unresolved attribution, exactly-once marker), middleware.rs (last_seen deferral), ws.rs/tty.rs (connection-time attribution + verified-identity push registry), jobs.rs/main.rs (expiry sweep), api/audit.rs (re-attribution), gyre-common/gyre-domain type additions; confirmed non-ABAC surfaces (git HTTP, MCP, ws, tty, explorer, conversations, ws-ticket) sit outside the last_seen/abac stack so the request-extensions marker cannot suppress their per-handler attribution; ran all invariant guards and focused tests; ran an independent mutation probe.

---

## Evidence (saved under /tmp/stage/review-evidence/)

- `cargo-test-impersonation.txt` / `cargo-test-impersonation-restored.txt` — `cargo test -p gyre-server --lib impersonation`: **15/15 passed** (includes `impersonation_works_through_production_router` through the full `build_router` layering, `per_handler_surfaces_are_attributed`, `non_target_cannot_probe_request_status`, both concurrency tests).
- `cargo-test-auth.txt` (37/37), `cargo-test-jobs.txt` (6/6).
- Guards, all exit 0: `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-in-memory-state-stores`, `check-mem-port-contracts`, `check-arch`, `check-dead-message-kinds`, `check-inert-enforcement`, `check-task-commit-attribution`.
- `mutation-probe-approve-lock.txt` — **independent mutation probe**: replaced the `SESSION_WRITE_LOCK` acquisition in `approve_request` with a no-op (`let _guard = ();`); `concurrent_approvals_create_exactly_one_session` fails with `exactly one approval must succeed: left: 2, right: 1` (two live sessions for one consent). Source restored (`git status` clean), test passes again on the pristine candidate. The concurrency test is a real bug-killer, not theater.

## Spec-contract assessment

- **Target consent (code/approval before impersonation):** genuine. One-time 8-digit CSPRNG code, only its SHA-256 hash persisted, constant-time compare, raw code delivered solely to the target user (Inbox notification + WS push matched by server-verified user identity). Returning the code to the requesting admin was deliberately withheld (documented deviation from the task plan's response shape) — correct, since the plan's shape would let the admin self-approve and defeat the spec's consent requirement. Approve requires the caller's verified user id to equal the request target, checked before any status/expiry is revealed (`non_target_cannot_probe_request_status`).
- **Notification of the impersonated user:** `ImpersonationAlert` (priority 1) persisted notification + WS `NotificationPush` when online. Email not implemented — no email infrastructure exists anywhere in the platform and the spec section does not require it (only the task plan, conditioned on "if email is configured"); acceptable and documented.
- **Full audit trail, attributed to the impersonator:** `ImpersonationStarted`/`ImpersonationEnded` with the exact observability.md detail fields (`impersonator_id`, `target_user_id`, `approval_token` = consumed code's hash; `duration_secs`, `actions_taken`), both attributed (`user_id`) to the impersonator. Every authenticated call under the token is recorded as `impersonation_action` across all surfaces — evaluated `/api/v1` routes post-policy-evaluation (with decision outcome), exempt/unresolved routes pre-passthrough, per-handler surfaces (git HTTP, MCP, conversations, explorer, ws-ticket) at auth resolution, WS/TTY at connection — with exactly-once per request via the extensions marker (asserted `actions_taken == 2` for two calls through the production router). `actions_taken` increments and session end are serialized by the session-write lock; the increment atomicity and the approval critical-section are both mutation-verified (the latter independently re-verified this round). Server-side attribution from the verified JWT replaces the plan's client-supplied `X-Gyre-Impersonator` header — a cryptographically stronger construction achieving the same spec intent; documented deviation, accepted.
- **Token:** EdDSA JWT, `sub` = target (permissions match), `impersonator_id`/`target_user_id`/`session_id`/`impersonating` claims, TTL from `GYRE_IMPERSONATION_TTL` (default 900 s), zero-leeway `exp`, session-record revocation on explicit end (token 401s before `exp`, test-verified). Accepted by `require_auth_middleware`, `FromRequestParts` and the WS `authenticate_token` path — verified end-to-end through `build_router` by test, closing the prior round's 401-before-handler gap.
- **End impersonation:** `DELETE` (impersonator-only, in-handler check) + background `impersonation_expiry` job (30 s, registered and spawned from main) + validation-time expiry; `ImpersonationEnded` carries duration and action count.
- **Authorization constraints:** Admin role + real user identity required (system token rejected), self-impersonation and Admin targets prohibited (no SuperAdmin in the role model — the plan's "or is prohibited" branch), max-1-active-session per admin enforced under the write lock in both create and approve paths, no requesting/approving/denying while impersonated (no privilege chaining).

## Non-blocking observations

- `SESSION_WRITE_LOCK` is a single-process lock; the kv store remains the record of truth and the code documents this. Multi-instance deployments would need distributed coordination, consistent with the platform's existing single-process state-machine patterns.
- A target user with no workspace membership receives no in-app notification (there is no honest workspace to label it with); fail-safe — without the delivered code the impersonation cannot be approved.

No structural defects, missing enforcement, unsafe scope handling, fake implementations, or weak tests found. The candidate satisfies the task contract with real production behavior behind every acceptance criterion.
