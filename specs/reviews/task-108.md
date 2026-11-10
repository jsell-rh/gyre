# Review: TASK-108 — Implement admin user impersonation

**Reviewer:** Independent review (pi runtime)
**Date:** 2026-09-30 (base-402142b1c checkpoint-continuation round)
**Verdict:** approved

**Candidate:** `b044276be0b41242274c771d999dbed740edde87` (base `402142b1c8d214b03977d689742baae6a1af7910`)
**Spec:** `identity-security.md` §Impersonation; `observability.md` §Audit Event Types

**Verification performed (all on the exact candidate; evidence under `/tmp/stage/review-evidence/`):**

- Read the full base→candidate diff (26 files, +3251/−10) and all new/changed sources end to end: `impersonation.rs` (915 lines), `api/impersonation.rs` (1615 lines incl. tests), `auth.rs` (mint/validate/EdDSA claims, `ImpersonationContext`, `FromRequestParts`/`authenticate_token` step 1.5), `abac_middleware.rs` (resolver entries + passthrough/evaluated attribution), `lib.rs` (`require_auth_middleware` impersonation branch), `middleware.rs` (last_seen marker deferral), `ws.rs`/`tty.rs` (connection-time attribution, `ws_connection_users`), `api/audit.rs` (event re-attribution), `jobs.rs`/`main.rs` (`impersonation_expiry` sweep), `notification.rs`/`protocol.rs`/`audit.rs`/`siem.rs` enums.
- `cargo test -p gyre-server --lib api::impersonation` — 14/14 green. `cargo test -p gyre-server --lib` — **1588/1588 green**. Focused adjacent suites (`auth::`, `abac_middleware`, `ws::`, `jobs::`, `tty`, `mcp::`, `siem::`) — 252/252 green. `gyre-common` 116/116, `gyre-domain` 442/442 (+8 integration).
- All static guards green on the candidate: arch, abac-route-registry, abac-exempt-handlers, dead-message-kinds, scope-literal-defaults, fabricated-scope-defaults, in-memory-state-stores, mem-port-contracts, inert-enforcement, byte-slice-truncation, forwarded-header-trust, forged-scope-fields, migration-versions, relative-path-defaults, fail-open-ref-resolution, lossy-secret-conversion, unbounded-external-http, mcp-write-tools, task-commit-attribution. `scripts/` is byte-identical to the base (no exemption growth, no verifier edits).
- `check-rustfmt-diff.py` vs base: clean (22 files). `check-clippy-diff.py` needs a workspace-wide build (rejected by the executor wrapper — deferred to verification); focused `cargo clippy -p gyre-server --lib` shows **zero diagnostics in any file changed by this task** (128 pre-existing warnings elsewhere in the package).
- **Mutation probe** (restored after run; tree verified clean): disabling the `session.ended || expired` check in `resolve_impersonation_auth` makes `end_session_revokes_token_and_emits_ended` fail (200 where 401 is asserted) — the revocation test kills a real bug, it is not self-confirming. Concurrent-approvals and atomic-counter tests use a yielding KV adapter that genuinely interleaves the read-modify-write, and prior rounds' mutation evidence showed both fail with the lock removed.

**Findings:** none blocking. Notes:

- The two deviations from the task plan are correct and documented: (1) the admin-facing response returns `{request_id, expires_at}` without the approval code — returning the code to the requester would let the admin self-approve, defeating the spec's target-consent requirement; (2) server-side attribution from the cryptographically verified token replaces the plan's client-supplied `X-Gyre-Impersonator` header, which is strictly weaker. Both satisfy the spec text better than the plan's literal shape.
- Email notification is not implemented — the codebase has no email infrastructure and the spec section does not require it (plan mention is conditional on configuration). Acceptable.
- Observation (not a contract violation; spec is silent): no same-tenant constraint between admin and target. The session tenant is the admin's; the target's consent is still required and no cross-tenant data of the target is exposed through the session. Worth a future spec decision if multi-tenant impersonation policy is wanted.
- The single-process `SESSION_WRITE_LOCK` is honestly documented in-code (kv store remains record of truth); the platform is a single-process server.

**Acceptance criteria:** request/approve/deny/end routes real and tested end-to-end through the production router; one-time hashed code delivered only to the target (Inbox + WS push); token with correct claims (sub=target, impersonator_id, zero-leeway exp, session revocation); ImpersonationStarted/Ended with the spec's detail fields; per-call `impersonation_action` attribution on every authenticated surface (evaluated/exempt/unresolved ABAC, git HTTP, MCP, conversations, explorer, ws-ticket, WS/TTY) with exactly-once request marking; TTL auto-expiry (JWT validation + sweep job); Admin-only with defense in depth, Admin-target and self-impersonation prohibited, max-1-active enforced. `cargo test --all` is deferred to the verification stage (executor wrapper rejects workspace-wide runs); every touched crate passes its full suite here.
