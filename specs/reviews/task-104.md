# Review — task-104 (Observability §Audit Event Types — source control + access control events)

Spec: `specs/system/observability.md` §Audit Event Types (4 source control + 6 access control event types).
Candidate under review: `c4b97884653a06f47ce12096fb11b20a12dd0c56` (merge head `f32cd89d7` of base `22f275e26a` task-138, plus process commits `0010d4f54`/`c4b978846`). Base for diff: `22f275e26a320cc8ab0f27f016688a138e62be8f`.
Verdict: **approved** (round 14).

## Probes (evidence in `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib` at the candidate HEAD: **1544 passed, 0 failed** (`task-104-lib-tests.log`, exit 0). Includes the task-104 tests: git push accepted (with per-gate `push_gate_results`), git-declined push records no acceptance, gate-rejected push (403 + rollback + rejected event), spec-change push, clone over smart HTTP, AuthSuccess (global token, once-per-request dedupe), AuthFailure (invalid token via `build_router` middleware path, revoked agent JWT, API-key user without tenant binding), AbacDenied (middleware, nine per-handler guards), RbacDenied (extractor, handler guards, rounds 8/9 sweeps, MCP tools/call write-role gate), CEF severity field-by-field for all 10 types.
- `cargo test -p gyre-server --test git_integration`: **17 passed, 0 failed** (`task-104-git-integration.log`, exit 0), including the base's `push_modifying_spec_revokes_ledger_approvals` and `spec_approval_auto_invalidated_on_spec_change` over the round-13 merged region.
- **Mutation check** (`task-04-mutation-spec-changed.log`): removed all three `emit_spec_changed_audit` call sites in `process_spec_lifecycle` (the round-13 merged region) → `push_spec_change_records_spec_changed_audit_event` **FAILED** (1 failed, exit 1). Restored the file from backup; `git status` clean afterwards. The emission is real and load-bearing, not test theater.
- `bash scripts/check-arch.sh` → OK; `bash scripts/check-fabricated-scope-defaults.sh` → OK; `bash scripts/check-inert-enforcement.sh` → OK; `bash scripts/check-task-commit-attribution.sh` → OK (all 8 frontmatter-listed product-surface task-104 commits account for every task-labeled product commit on the branch; merge commits bring reviewed bases).
- `cargo fmt -p gyre-server -p gyre-domain -- --check` → clean.

## Verified

- **Enum completeness**: all 10 variants in `AuditEventType` with `as_str`/`from_str` round-trip tests (`gyre-domain/src/audit.rs`).
- **Real emission points** — every event goes through the shared `api::audit::emit_server_audit_event` (AuditRepository port `record` + SSE broadcast), so events land in the same store that query/stats/SIEM read:
  - `GitPushAccepted`: per *verified-applied* ref update in `git_receive_pack` post-receive, with repo_id, branch, commit_sha, agent_id, and structured `push_gate_results` from `check_pre_accept_gates`. `verify_applied_ref_updates` (round 5) rev-parse-confirms each requested update landed, so git-declined pushes (non-fast-forward) are not audited as accepted; covered by `push_declined_by_git_records_no_git_push_accepted_audit_event`.
  - `GitPushRejected`: every gate-rejection path (pre-accept gates with rejecting `gate_name`; constraint, manifest-coverage, spec-link cycle gates) with reason, outcome Blocked.
  - `GitClone`: `git_upload_pack` after successful clone/fetch, with repo_id + agent_id from the resolved repo row.
  - `SpecChanged`: all three spec-lifecycle arms (dedup-skip, create-Err, create-Ok) with spec_path/change_kind/task_id, workspace-scoped from the per-push `ws_id` resolution; `None` (never a fabricated "default") when the repo row is unresolvable.
  - `AuthSuccess`/`AuthFailure`: extractor paths for all token kinds (global, agent_jwt, uuid_token, api_key, user_jwt, federated_jwt), the global `require_auth_middleware` (via `record_auth_failure_for_middleware` — tested through `build_router` with a bad token), the WebSocket `authenticate_token` path, and the round-12 API-key tenant-binding rejection; per-request dedupe via a request-extension marker; forensic source_ip from `ConnectInfo` (CWE-348 compliant, no forwarded-header trust).
  - `AbacDenied`: middleware deny path (policy_id, resource, action, claims), git push repo-level ABAC, and all nine round-11 per-handler guards via the shared `record_abac_denied`.
  - `RbacDenied`: `RequireAgent` extractor, all live pure per-handler role guards in `api/`, and the MCP tools/call write-role gate, via the shared `record_rbac_denied` with required_role/actual_role/endpoint.
  - `ImpersonationStarted`/`ImpersonationEnded`: variants + as_str/from_str + CEF severity 7; emission deferred to task-108 per the task plan (impersonation not yet implemented).
- **CEF severity mapping** in `siem.rs::format_cef` matches the task plan exactly (AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4), asserted field-by-field.
- **Independent deny-path sweep** (my own scan, not prior rounds' claims): every non-test `StatusCode::FORBIDDEN` in `gyre-server/src` is either an instrumented policy/role rejection, a GitPushRejected/AbacDenied git path, a tenant-containment or ownership guard (Owner-or-Admin `created_by` checks — a denial class outside both spec triggers "ABAC policy rejects request" and "Role check rejects request", consistently scoped out since round 8), a read-only-mirror repository-state rejection (not a gate rejection per the spec trigger table), or a generic `ApiError::Forbidden` mapping. No new uninstrumented policy/role denial found.
- **Round-13 merge resolution**: the union in `process_spec_lifecycle` keeps the base's once-per-push `ws_id` resolution, scopes all three SpecChanged emissions from it, adopts the base's conditional workspace-event emission, and preserves both regression tests over the resolved region (both green at HEAD). The base's approval-invalidation block is intact (`revoke_all_for_path` per stale path).
- **Coverage matrix**: `specs/coverage/system/observability.md` row 5 updated to `implemented` with an accurate 26/34 accounting (remaining 8 types belong to other tasks); no `not-started` rows remain in the file.

## Notes (non-blocking)

- Branch-deletion pushes (new_sha all-zeros) record no `GitPushAccepted` because `verify_applied_ref_updates` requires the ref to resolve to the pushed SHA; a deletion has no commit_sha for the spec'd detail field. Defensible reading of the trigger ("push accepted" with `commit_sha` detail); flagging for task-108/verification awareness only.
- Fetch operations record `GitClone` (same `git-upload-pack` service); a superset of the spec trigger "Agent clones repo".

Full workspace suites, Clippy all-targets, and CI are owned by verification, per the standing scope.
