---
title: "Add access control and source control audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["ac5f88365dfcde25d5ae325fd24bebb90203c99b", "ca905fe4389a020e924a28e24b5b15e02d6a48e6"]
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

- [ ] All 10 event types added to `AuditEventType` enum
- [ ] `GitPushAccepted` emitted on successful push with gate results
- [ ] `GitPushRejected` emitted when push gate rejects
- [ ] `GitClone` emitted on clone operations
- [ ] `SpecChanged` emitted via spec lifecycle hook
- [ ] `AuthSuccess` emitted on valid token acceptance
- [ ] `AuthFailure` emitted on token rejection with reason
- [ ] `AbacDenied` emitted on ABAC policy denial
- [ ] `RbacDenied` emitted on role check failure
- [ ] `ImpersonationStarted`/`ImpersonationEnded` variants exist (emission wired in task-108)
- [ ] CEF severity mapping updated
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Audit Event Types for the full event type tables. Git push handling is in `gyre-server/src/git_http.rs`. Auth middleware is in `gyre-server/src/auth.rs`. ABAC middleware is in `gyre-server/src/abac_middleware.rs`. Spec lifecycle hooks are in `git_http.rs` (search for `process_spec_lifecycle`). SIEM CEF mapping is in `gyre-server/src/siem.rs` (search for `format_cef`). This task depends on task-102 (schema expansion) being completed first. Impersonation emission points are deferred to task-108.

## Shipped

All 10 event types from observability.md §Audit Event Types are implemented with real production emission points (commit `ac5f88365`). Every event is recorded through the shared `api::audit::emit_server_audit_event` helper (AuditRepository port + SSE broadcast), so it lands in the same store the query/stats/SIEM paths read.

### Review repair (round 5, commit `ca905fe43`): git-declined ref updates no longer record GitPushAccepted

The review found that the post-receive block emitted `git_push_accepted` (outcome=Success) for every *requested* ref update whenever receive-pack exited 0 — but receive-pack reports per-ref rejections (non-fast-forward, stale client, `receive.denyNonFastForwards`, lock contention) in its report-status stream, not via exit status. A stale-client push was therefore audited (and SIEM-forwarded at severity 3) as an accepted push with a commit SHA that never became the branch head.

Fix in `git_http.rs`:

- New `verify_applied_ref_updates` confirms each requested update actually landed via `git rev-parse --verify --end-of-options <refname> == new_sha` (refnames are client-controlled, so a `refs/` prefix guard plus `--end-of-options` prevents option injection), run right after receive-pack completes and before any acceptance signaling.
- The spawned post-receive block (agent-commit recording, conversation-turn links, PushAccepted message-bus event, GitPushAccepted audit loop, strategy-implied constraint evaluation, spec lifecycle, lineage re-evaluation, spec-ledger sync, dependency/breaking-change detection, graph extraction) now processes only verified-applied updates. A push git declined in full emits no PushAccepted message and no audit event; `GitPushRejected` remains reserved for gate rejections per the spec trigger table ("push rejected by gate"), so a git-level decline emits neither event — it is logged at `warn!`.
- Handler-side gate checks (pre-accept gates, constraints, manifest, cycles) and the M13.3 response header/sideband are unchanged.

Regression test `push_declined_by_git_records_no_git_push_accepted_audit_event` reproduces the reviewer's probe (server ref at `tip`, client pushes stale `base -> divergent`): asserts HTTP 200 with git's own report, the ref unchanged at `tip`, and no `git_push_accepted`/`git_push_rejected` audit events after a bounded grace period. Mutation-verified: forcing `is_applied = true` (the pre-fix behavior) makes the test fail on the recorded false event.

### Source control events

- **GitPushAccepted** — emitted per *verified-applied* ref update in `git_receive_pack` post-receive, with `repo_id`, `branch`, `commit_sha`, `agent_id`, and `push_gate_results`. `check_pre_accept_gates` returns structured per-gate outcomes (`PushGateOutcomeEntry`) so the accepted event carries each configured gate's pass/fail.
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

Focused tests drive real behavior: a real smart-HTTP push with a configured gate (accepted event asserts gate results), a gate-rejected push (403 + rollback + rejected event, no accepted event), a git-declined non-fast-forward push (no accepted/rejected events, ref unchanged — the round-5 regression test), a real `git clone` over a local listener (git_clone event), a spec-file push (spec_changed with task_id), invalid/revoked token rejections (auth_failure with reason), the per-request AuthSuccess dedupe, an ABAC policy denial (policy_id/resource/action/claims), and both RbacDenied paths (extractor + handler guard). All 10 CEF severities asserted field-by-field.

- `cargo test -p gyre-server --lib` (full suite, superset of the task-104 filters): **1431 passed, 0 failed** at repair head `ca905fe43` on merge base `414fe9aa1b` (evidence: `/tmp/stage/review-evidence/task-104-repair-server-lib.log`).
- Mutation check: with `is_applied` forced true (pre-fix behavior) the regression test fails on the false `git_push_accepted` event; with the fix it passes.
- `cargo clippy -p gyre-server --lib`: exit 0; git_http.rs warning count identical to the pre-change tree (13), no new warnings.
- `scripts/check-arch.sh`: exit 0.
- Prior-round evidence (original candidate and merge heads): `cargo test --all` 24 suites ok; `cargo test -p gyre-domain --lib audit` 9/9. Full workspace suites remain owned by verification.

### Merge history

- Rounds 1–2 (original candidate `bad0e3114` / merge head `4c6319db3`): implementation `ac5f88365`, preserved unchanged through merges of bases `233664cf8` (task-087) and `72ad2a128` (task-096); exemption-file conflicts resolved with upstream site fixes holding and `FROZEN_EXEMPTION_COUNT` lowered 4→3.
- Round 5 (this repair, merge of base `414fe9aa1b`, task-120 — merge commit `13a71b428`): fixed the review finding above in `ca905fe43`; no other task-104 file touched.
