# Review — task-104 (Observability §Audit Event Types — source control + access control events)

Spec: `specs/system/observability.md` §Audit Event Types (Source Control Events + Access Control Events tables).
Candidate: `e137baf14` (merge head; production work in `ac5f88365`, preserved through the base-`233664cf8` merge `26760789b`). Base: `233664cf8`.
Verdict: **approved**.

## What was reviewed

Full diff `233664cf8..e137baf14` (23 files, ~1.8k insertions): all 10 event-type variants + as_str/from_str round-trips in `gyre-domain/src/audit.rs`; the shared `emit_server_audit_event` helper (AuditRepository port + SSE broadcast) in `api/audit.rs`; emission wiring in `git_http.rs` (push accepted/rejected, clone, spec-changed), `auth.rs` (extractor + WebSocket `authenticate_token`), `lib.rs` `require_auth_middleware` (AuthFailure before extractors run), `abac_middleware.rs` (`record_abac_denied`, shared with the repo-level push ABAC check), `rbac.rs` (`record_rbac_denied` from the `RequireAgent` extractor and every live per-handler role guard in `api/`); CEF severity mapping in `siem.rs`.

## Verified against the acceptance criteria

- **All 10 variants** present with round-trips; `source_control_and_access_control_event_types_roundtrip` asserts each string form.
- **GitPushAccepted** — emitted per accepted ref update in the post-receive spawn, after all gate paths pass, with `repo_id`, `branch`, `commit_sha`, `agent_id`, `push_gate_results`. `check_pre_accept_gates` was restructured to return structured per-gate outcomes (`PushGateOutcomeEntry`) so the accepted event carries each configured gate's pass/fail — real data, not a placeholder.
- **GitPushRejected** — emitted at every rejection path: pre-accept gates (with rejecting `gate_name`), constraint-enforcement, manifest-coverage, and spec-link-cycle gates. Emission occurs after `undo_ref_updates` rollback; test asserts refs rolled back and no `git_push_accepted` event exists for the rejected push.
- **GitClone** — emitted in `git_upload_pack` only after `run_git_stateless("upload-pack", ...)` succeeds (failure returns early via `git_err`), with `repo_id`, `agent_id`. Test drives a real `git clone` against a real local TCP listener through the smart-HTTP route.
- **SpecChanged** — emitted by `process_spec_lifecycle` for every spec change on the default branch, with `spec_path`, `change_kind` (A/M/D/R mapped), and `task_id` (Some on task creation, None on dedup or creation failure — the event records the spec change either way). Workspace scope is looked up from the repo; on lookup failure the event is recorded unscoped rather than fabricating a "default" identity (task-097 F3 class respected).
- **AuthSuccess** — wired into the `AuthenticatedAgent` extractor (all token kinds: global, agent_jwt, uuid_token, api_key, user_jwt, federated_jwt) and the WebSocket path. A request-extension `AuthAuditMark` dedupes to exactly one event per request; the dedupe test runs the full `build_router` stack where ABAC middleware + handler extractor both validate the token.
- **AuthFailure** — wired into the extractor (invalid/expired agent JWT, revoked agent JWT with valid signature, generic invalid token), the global `require_auth_middleware` (which rejects before extractors run), and the WebSocket path, each with `token_kind` and `rejection_reason`. Forensic `source_ip` comes from `ConnectInfo` (socket peer), never forwarded headers — CWE-348 check passes.
- **AbacDenied** — from the ABAC middleware deny path with `policy_id` (matched policy), `resource`, `action`, `claims`, plus the repo-level push ABAC guard.
- **RbacDenied** — from the `RequireAgent` extractor and the per-handler role guards (budget, compute targets, llm config/prompts, meta-specs, release, spec/stack policy, tenants, trust anchors, users, network peers) with `user_id`, `required_role`, `actual_role`, `endpoint`.
- **ImpersonationStarted/Ended** — variants + round-trips + CEF severity 7 exist; emission deferred to task-108 exactly as the task plan states (impersonation itself is unimplemented).
- **CEF severity** — AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4, asserted field-by-field against the header.

## Independent evidence (logs under /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib audit_event` → **19 passed, 0 failed** (`task-104-server-audit-event-tests.log`; covers the 11 audit-event tests including 3 pre-existing ones also matching the filter).
- `cargo test -p gyre-server --lib cef_severity` → **1 passed** (`task-104-siem-cef.log`); `... --lib rbac_denied` → **2 passed** (`task-104-rbac-denied.log`); `cargo test -p gyre-domain --lib audit` → **9 passed** (`task-104-domain-audit.log`).
- **Mutation probes** (each: break production, test FAILS, restore): CEF AuthFailure severity 6→3 kills `cef_severity_source_control_and_access_control_events`; retyping `record_abac_denied`'s event kills `abac_denial_records_abac_denied_audit_event`; emptying the post-receive ref loop kills `push_records_git_push_accepted_audit_event` — the tests are not self-confirming (`task-104-mutation-probes.md`).
- Check scripts: `check-fabricated-scope-defaults.sh` OK, `check-abac-exempt-handlers.sh` OK (88 handlers), `check-inert-enforcement.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-forwarded-header-trust.sh` all OK.
- The single touched exemption (`fabricated-scope-defaults-exemptions.txt` auth.rs:773→1041) was verified to re-point the *same unchanged code site* (JWT claims.tenant_id fallback, present verbatim in base at 233664cf8) — only the line number moved due to insertions above it. Not a new exemption.
- Tree pristine after probes: `git status --short` empty, HEAD = `e137baf14`.

## Notes

- The merge of base `233664cf8` (task-087 trace-capture work) touched none of the task-104 emission files; all emission points verified present at the merge head by direct code inspection.
- `git_upload_pack` was refactored from `resolve_repo_path_by_slug` to `resolve_repo_by_slug`; the new function performs the identical tenant-scoped workspace+repo lookup (workspace resolved under the caller's tenant before repo lookup), so no scoping regression.
- Best-effort token classification in the middleware failure path ("ey" → agent_jwt, else unknown) is a reasonable heuristic for an event detail field, not an enforcement decision.
