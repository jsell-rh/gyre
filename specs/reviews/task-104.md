# Review — task-104 (Add access control and source control audit event types)

Spec: `observability.md` §Audit Event Types (4 source control + 6 access control events, detail fields, CEF severities).
Candidate under review: `e633c9a16` (round-12 head; assigned commits `2d4faab3a`, `07794b483`, `48edaaf9a`, `0e487df7f`, `47190ed90`, `8e6ed8ec`, `ca905fe43`, `ac5f88365` all present on the branch between base `7373979e8` and head; diff base..candidate touches auth.rs, git_http.rs, rbac.rs, abac_middleware.rs, mcp.rs, siem.rs, api/* deny guards, coverage row 5).
Verdict: **approved**.

## Probes run (this round, at candidate head)

- `cargo test -p gyre-server --lib audit_event` → **31 passed, 0 failed** (covers git push accepted/declined/gate-rejected, spec-change push, real clone, auth success dedupe, invalid/revoked-token and API-key tenant-binding auth failures, ABAC middleware + nine per-handler guards, RBAC extractor + handler guards + rounds 8/9/10 regressions, MCP write-role gate). Log: `/tmp/stage/review-evidence/task-104-focused-audit-event-tests.log`.
- `cargo test -p gyre-server --lib cef_severity_source_control` → **1 passed, 0 failed** (all 10 severities asserted field-by-field on the CEF header). Log: `/tmp/stage/review-evidence/task-104-cef-severity-test.log`.
- **Fresh mutation check (mine, not the implementer's):** disabled the `emit_git_push_accepted_audit` call in the `git_receive_pack` post-receive loop → `push_records_git_push_accepted_audit_event` **FAILED** ("no 'git_push_accepted' audit event was recorded within 30s"). Restored; `git status` clean; test re-passed. The acceptance tests are real proof, not self-confirming. Log: `/tmp/stage/review-evidence/task-104-mutation-git-push-accepted.log`.
- `bash scripts/check-fabricated-scope-defaults.sh` → OK (exemption file: documentation-only diff, same 2 entries, frozen count enforced).
- `bash scripts/check-arch.sh` → OK.

## Verified against the acceptance criteria

- **All 10 event types** in `AuditEventType` with `as_str`/`from_str` round-trips (`gyre-domain/src/audit.rs:19-95`) — confirmed by reading and by the variant list at audit.rs:216-225.
- **GitPushAccepted** — emitted per ref update in the post-receive loop, only for updates `verify_applied_ref_updates` confirmed landed via `git rev-parse` (round-5 repair); wire-level test asserts `repo_id`/`branch`/`commit_sha`/`agent_id`/`push_gate_results` (per-gate entries with a real configured conventional-commit gate) plus envelope scoping. Mutation-verified by me this round.
- **GitPushRejected** — emitted at all four gate-rejection sites (pre-accept gates, constraint, manifest-coverage, spec-link cycle) with `gate_name`/`reason`, outcome Blocked; git-declined (non-fast-forward) pushes emit neither accepted nor rejected (test asserts both).
- **GitClone** — `git_upload_pack` emits after a successful smart-HTTP clone with `repo_id`/`agent_id`; test does a real `git clone` over the router.
- **SpecChanged** — spec-lifecycle hook emits per spec change on the default branch with `spec_path`/`change_kind`/`task_id` (null when deduped or task creation fails — correct: no task exists to name); workspace scope looked up from the repo record, skipped (never fabricated) when unresolvable.
- **AuthSuccess/AuthFailure** — extractor, global middleware, and WebSocket `authenticate_token` paths; `token_kind` distinguishes global/agent_jwt/uuid_token/api_key/user_jwt/federated_jwt; failures carry `rejection_reason` (invalid, expired, revoked, no-tenant-binding). Round-12 ordering verified in source: the fail-closed API-key tenant-binding check runs *before* `record_auth_success` (auth.rs:815-849), and the rejection records AuthFailure first — a 403 never emits AuthSuccess. A request-extension marker dedupes AuthSuccess to one event per request (test asserts exactly one).
- **AbacDenied** — middleware deny path (with `policy_id`, `resource`, `action`, `claims`), repo-level push ABAC, and the nine round-11 per-handler G6 guards (worktrees, jj×4, repo orchestrator, briefing/ask, agent spawn REST+MCP, conversations), all via the shared `record_abac_denied` helper.
- **RbacDenied** — `RequireAgent` extractor, all pure per-handler role guards in api/ (rounds 8/9 sweeps), and the MCP tools/call write-role gate (round 10), with `required_role`/`actual_role`/`endpoint`.
- **ImpersonationStarted/Ended** — variants, string forms, and CEF severity 7 exist; emission deferred to task-108 per the task plan (impersonation itself unimplemented). Conformant with the task's explicit deferral.
- **CEF severity mapping** — `format_cef` matches the spec table exactly (AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, Impersonation×2 7, SpecChanged 4); test asserts the 7th CEF header field for all 10.

## Non-findings (checked, judged acceptable)

- Remaining `FORBIDDEN` sites without `record_*_denied` emissions (e.g. saved_views ownership/tenant-match/membership guards, `is_system` view guards) reject on ownership, tenant containment, or membership — not a role check (no `required_role`/`actual_role` exist to report) and not an ABAC policy evaluation (no `policy_id`). The spec triggers are "Role check rejects request" / "ABAC policy rejects request"; the ownership/containment class is a different denial type, consistent with the classification accepted across rounds 8-11 and documented in coverage row 5.
- `source_ip` on auth events comes from `ConnectInfo` (socket peer), not forwarded headers — CWE-348 compliant.
- `cargo test --all` (acceptance criterion) is owned by verification per the standing probe-size constraint; the focused lib suite (31 audit-event tests + severity test) plus the mechanical checks above are the meaningful focused evidence.

## Summary

Every acceptance criterion is met by production code I read and exercised at the candidate head: emission call sites sit on the real deny/accept paths, detail fields match the spec tables, the git-declined push is not audited as accepted, the round-12 rebase resolution preserves both sides' semantics with correct AuthFailure-before-403 ordering, and my own mutation check confirms the headline test kills a real regression. No findings.

— Reviewer, 2026-11-14 (round 13, candidate `e633c9a16`)
