# Review — task-108 (Implement admin user impersonation)

Spec: `specs/system/identity-security.md` §Impersonation, plus `specs/system/observability.md` §Audit Event Types (`ImpersonationStarted`/`ImpersonationEnded`).
Candidate under review: `9db0dd279` (branch `pipeline/task-108/a64fd0ef3103448bb85b9eed07cedc76-1`, assigned base `65c470ea`). The candidate is the merge of base `65c470ea` (task-104 access/source-control audit lineage) into the previously verified branch head, plus the process record commit.
Verdict: **approved**.

## Round 1 (this review)

All probes ran on the exact candidate checkout with `/tmp/stage/bin/cargo`; full evidence with commands and outputs in `/tmp/stage/review-evidence/task-108-review-9db0dd279.txt`.

Merge-repair verification (the shipped notes' claims were independently re-derived, not trusted):

- `git diff 65c470ea..HEAD -- crates/gyre-domain/src/audit.rs` is exactly the 2-line impersonation comment; the base's task-104 variants (`GitPushAccepted`…`RbacDenied`) and the impersonation variants are both present in the enum, `as_str`/`from_str` and the round-trip test array (both sides' tests pass — `gyre-domain` 443/443).
- `crates/gyre-server/src/siem.rs` is byte-identical to the base (diff empty): the merged tree carries the base's CEF severity 7/7 for `ImpersonationStarted`/`ImpersonationEnded`, and the base's test asserting it passes (`siem` 11/11). Dropping this branch's earlier 6/4 is not a regression — no test in either lineage asserts 6/4, and the shared-main mapping is the one both trees must satisfy.
- `crates/gyre-server/src/{impersonation.rs,api/impersonation.rs,api/mod.rs,jobs.rs,main.rs}` are byte-identical to the pre-merge verified head `b044276be` (diff empty). `auth.rs` and `abac_middleware.rs` diffs vs `b044276be` contain zero impersonation-line changes (grep-verified; reference counts identical at 99 and 29) — the diffs are base-side task-104 additions only.
- `scripts/` is byte-identical to the base; no verifier or exemption file was touched by this round.

Behavioral probes:

- `api::impersonation` 14/14, `auth::` 52/52, `abac_middleware` 14/14, `jobs::` 6/6, `ws::` 90/90, `siem` 11/11, `gyre-domain --lib` 443/443, `gyre-common --lib` 120/120 — all green on the exact candidate.
- Static guards all exit 0: arch, abac-route-registry, abac-exempt-handlers, fabricated-scope-defaults, scope-literal-defaults, dead-message-kinds, in-memory-state-stores, inert-enforcement, mem-port-contracts, task-commit-attribution.
- **Independent mutation probe** (restored afterwards; tree clean): disabling the `session.ended || now >= expires_at` check in `resolve_impersonation_auth` makes `end_session_revokes_token_and_emits_ended` fail (assertion 200 ≠ 401) — the pre-`exp` token revocation is real, enforced enforcement, not a structural claim. Suite re-run green after restore.

Substantive design points checked against the spec and found correct:

- **Target consent before impersonation**: one-time CSPRNG 8-digit code, only its SHA-256 hash persisted, raw code delivered solely to the target (Inbox `ImpersonationRequest` notification + WS push matched by server-verified user id). The plan's response shape was deliberately corrected: the admin-facing response is `{request_id, expires_at}` because returning the code to the requester would let the admin self-approve and defeat the spec's consent requirement. This deviation is correct and documented.
- **Per-handler authorization on the ABAC-exempt approve/deny routes**: caller's verified user id must equal the request's target, checked before any status/expiry is revealed (non-target probing gets 403, not a status-leaking 409 — covered by `non_target_cannot_probe_request_status`). Constant-time code comparison. Admin targets prohibited, no self-impersonation, max-1-active-session rate limit enforced under a shared session-write lock that also serializes approval/deny/expiry-sweep transitions (concurrency covered by `concurrent_approvals_create_exactly_one_session`, previously mutation-verified by the implementer with saved evidence).
- **Token**: EdDSA JWT, `sub` = target (permissions match), `impersonator_id`/`target_user_id`/`session_id`/`impersonating` claims, TTL 900 s configurable via `GYRE_IMPERSONATION_TTL`, zero-leeway `exp`, revocation via the session record. The impersonation branch exists in all three auth entry points (`require_auth_middleware`, `FromRequestParts` step 1.5, WS `authenticate_token`); an impersonation token cannot ride the agent-token path (step 1.5 intercepts it; the agent path additionally requires `agent_tokens` kv membership, which impersonation tokens never have; `ImpersonationJwtClaims` requires the `impersonating` field so agent JWTs cannot deserialize as impersonation claims).
- **Full audit trail with impersonator attribution**: `ImpersonationStarted`/`ImpersonationEnded` carry the spec's exact detail fields (`impersonator_id`, `target_user_id`, `approval_token` = consumed code's hash, `duration_secs`, `actions_taken`), both attributed (`user_id`) to the impersonator. Every authenticated call under the token is audited as `impersonation_action` across all surfaces (evaluated `/api/v1` routes with the ABAC decision outcome; exempt/unresolved routes before passthrough; git HTTP/MCP/conversations/explorer/ws-ticket at the auth-resolution layer; WS/TTY connections), exactly-once per request via a request-extensions marker — asserted end-to-end through `build_router` in `impersonation_works_through_production_router` (two calls → exactly 2 events, `actions_taken == 2`, both attributed to `admin-1`). Server-side verified-claim attribution replaces the plan's client-supplied `X-Gyre-Impersonator` header — a strictly stronger construction.
- **Expiry**: enforced at validation time (zero leeway) regardless of the sweep; the `impersonation_expiry` job (30 s, registered in the job registry and spawned from `main.rs`) ends expired sessions, emits `ImpersonationEnded` (reason `expired`) and marks stale pending requests; covered by `expired_sessions_auto_end_via_sweep`.
- **Durable state**: requests/sessions persist through the `KvJsonStore` port (no bare in-memory store; guard green).

Documented, accepted non-implementation: email notification on impersonation start — the plan conditions it on "if email is configured"; the platform has no email infrastructure at all and spec §Impersonation does not require it.

No findings. Full workspace suites, all-target Clippy and remote CI are owned by the verification stage.
