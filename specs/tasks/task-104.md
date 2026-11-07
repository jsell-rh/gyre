---
title: "Add access control and source control audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["ac5f8836d97a4c05d46cba66507c199847db772bf"]
---

## Spec Excerpt

The spec defines source control events (4) and access control events (6) that must be emitted:

**Source Control Events:**

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `GitPushAccepted` | Smart HTTP push accepted | `repo_id`, `branch`, `commit_sha`, `agent_id`, `push_gate_results` |
| `GitPushRejected` | Smart HTTP push rejected by gate | `repo_id`, `branch`, `gate_name`, `reason` |
| `GitClone` | Agent clones repo | `repo_id`, `agent_id` |
| `SpecChanged` | Spec file modified in default branch push | `repo_id`, `spec_path`, `change_kind`, `task_id` |

**Access Control Events:**

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `AuthSuccess` | Valid token accepted | `token_kind` (global, agent_jwt, uuid_token, api_key), `agent_id` |
| `AuthFailure` | Invalid or revoked token | `token_kind`, `rejection_reason` |
| `AbacDenied` | ABAC policy rejects request | `agent_id`, `policy_id`, `resource`, `action`, `claims` |
| `RbacDenied` | Role check rejects request | `user_id`, `required_role`, `actual_role`, `endpoint` |
| `ImpersonationStarted` | Admin begins impersonating user | `impersonator_id`, `target_user_id`, `approval_token` |
| `ImpersonationEnded` | Impersonation session ends | `impersonator_id`, `target_user_id`, `duration_secs`, `actions_taken` |

## Implementation Plan

1. **Add event type variants (`gyre-domain/src/audit.rs`):**
   - Add all 10 event types to `AuditEventType` enum
   - Update `as_str()` and `from_str()` methods

2. **Wire source control emission points:**
   - **GitPushAccepted**: In `gyre-server/src/git_http.rs` after successful push processing, record with repo_id, branch, commit_sha, agent_id, gate_results
   - **GitPushRejected**: In git_http.rs when a push gate rejects, record with repo_id, branch, gate_name, reason
   - **GitClone**: In git_http.rs on git-upload-pack (clone), record with repo_id, agent_id
   - **SpecChanged**: In the spec lifecycle hook (git_http.rs process_spec_lifecycle), record for each spec change

3. **Wire access control emission points:**
   - **AuthSuccess**: In `gyre-server/src/auth.rs` or middleware after successful token validation, record with token_kind
   - **AuthFailure**: In auth.rs on token rejection (expired, revoked, invalid), record with token_kind, reason
   - **AbacDenied**: In `gyre-server/src/abac_middleware.rs` when policy evaluation denies, record with policy_id, resource, action
   - **RbacDenied**: In RBAC middleware when role check fails, record with user_id, required_role, actual_role, endpoint
   - **ImpersonationStarted/Ended**: Placeholder — will be wired when impersonation (task-108) is implemented

4. **Update SIEM CEF severity mapping:**
   - `AuthFailure` → severity 6
   - `AbacDenied` / `RbacDenied` → severity 5
   - `GitPushRejected` → severity 5
   - `AuthSuccess` / `GitPushAccepted` / `GitClone` → severity 3
   - `ImpersonationStarted` / `ImpersonationEnded` → severity 7
   - `SpecChanged` → severity 4

## Acceptance Criteria

- [x] All 10 event types added to `AuditEventType` enum
- [x] `GitPushAccepted` emitted on successful push with gate results
- [x] `GitPushRejected` emitted when push gate rejects
- [x] `GitClone` emitted on clone operations
- [x] `SpecChanged` emitted via spec lifecycle hook
- [x] `AuthSuccess` emitted on valid token acceptance
- [x] `AuthFailure` emitted on token rejection with reason
- [x] `AbacDenied` emitted on ABAC policy denial
- [x] `RbacDenied` emitted on role check failure
- [x] `ImpersonationStarted`/`ImpersonationEnded` variants exist (emission wired in task-108)
- [x] CEF severity mapping updated
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Audit Event Types for the full event type tables. Git push handling is in `gyre-server/src/git_http.rs`. Auth middleware is in `gyre-server/src/auth.rs`. ABAC middleware is in `gyre-server/src/abac_middleware.rs`. Spec lifecycle hooks are in `git_http.rs` (search for `process_spec_lifecycle`). SIEM CEF mapping is in `gyre-server/src/siem.rs` (search for `format_cef`). This task depends on task-102 (schema expansion) being completed first. Impersonation emission points are deferred to task-108.

## Shipped

All 10 event types from observability.md §Audit Event Types are implemented with real production emission points (commit `ac5f8836`). Every event is recorded through the shared `api::audit::emit_server_audit_event` helper (AuditRepository port + SSE broadcast), so it lands in the same store the query/stats/SIEM paths read.

### Source control events

- **GitPushAccepted** — emitted per accepted ref update in `git_receive_pack` post-receive, with `repo_id`, `branch`, `commit_sha`, `agent_id`, and `push_gate_results`. `check_pre_accept_gates` was restructured to return structured per-gate outcomes (`PushGateOutcomeEntry`) instead of an opaque string, so the accepted event carries each configured gate's pass/fail.
- **GitPushRejected** — emitted at every push-gate rejection path (pre-accept gates with the rejecting `gate_name`; constraint enforcement, manifest coverage, and spec-link cycle gates with their gate labels), with `reason`, outcome `Blocked`. Verified: no `git_push_accepted` event exists for a rejected push and refs are rolled back.
- **GitClone** — emitted by `git_upload_pack` after a successful smart-HTTP clone/fetch.
- **SpecChanged** — emitted by the spec-lifecycle hook for each spec change on the default branch, with `spec_path`, `change_kind`, and the auto-created `task_id` (null when the change dedups against an existing task).

### Access control events

- **AuthSuccess / AuthFailure** — wired into the `AuthenticatedAgent` extractor, the global `require_auth_middleware`, and the WebSocket `authenticate_token` path. `token_kind` distinguishes global / agent_jwt / uuid_token / api_key / user_jwt / federated_jwt; failures carry `rejection_reason` (invalid, expired, revoked). A request-extension marker dedupes AuthSuccess to exactly one event per request even when the token is validated by both the ABAC middleware and the handler extractor.
- **AbacDenied** — emitted from the ABAC middleware deny path (with `policy_id` from the matched policy, `resource`, `action`, `claims`) and from the repo-level ABAC check that guards git push.
- **RbacDenied** — emitted from the `RequireAgent` extractor and from every live per-handler role guard (budget, compute targets, llm config/prompts, meta-specs, release, spec/stack policy, tenants, trust anchors, users, network peers) with `required_role`, `actual_role`, `endpoint`.
- **ImpersonationStarted / ImpersonationEnded** — enum variants, as_str/from_str, and CEF severity 7 exist; emission points are deferred to task-108 (impersonation itself is not implemented yet), per the task plan.

### CEF severity mapping

`format_cef` maps per spec: AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4.

### Test evidence

- `cargo test --all`: 24 suites, all ok, 0 failed (evidence: /tmp/stage/review-evidence/task-104-cargo-test-all.txt).
- Focused tests (59 passed, evidence: task-104-focused-tests.txt) drive real behavior: a real smart-HTTP push with a configured gate (accepted event asserts gate results), a gate-rejected push (403 + rollback + rejected event, no accepted event), a real `git clone` over a local listener (git_clone event), a spec-file push (spec_changed with task_id), invalid/revoked token rejections (auth_failure with reason), the per-request AuthSuccess dedupe, an ABAC policy denial (policy_id/resource/action/claims), and both RbacDenied paths (extractor + handler guard). All 10 CEF severities asserted field-by-field.
- Pre-existing baseline note: `cargo clippy -p gyre-domain` fails on `never_loop` in rust_extractor.rs:1071 — reproduced on the base commit, file untouched by this task. Repo check scripts: failing set identical to base; the only touched exemption re-points the frozen auth.rs JWT tenant fallback entry (same unchanged code site, line 773 → 1041).
