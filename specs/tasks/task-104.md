---
title: "Add access control and source control audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["07794b483ab9bddab2f38e2e32310823c5b79f7d", "48edaaf9a1bb9e24e4f9ddaf2e452402ad478c83", "0e487df7fb413c2d57998cf1d0c2c53818a2ba28", "47190ed901d978fa05f6e377fe436cba52f4be7b", "8e6ed8eccd3ce7243d411c2378d95cccf473655a", "ca905fe432f76092f9914513d736b53bfd800631", "ac5f88365dfcde25d5ae325fd24bebb90203c99b"]
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

All 10 event types from observability.md §Audit Event Types are implemented with real production emission points (implementation `ac5f88365`, repaired through rounds 5–11; current head `07794b483`). Every event is recorded through the shared `api::audit::emit_server_audit_event` helper (AuditRepository port + SSE broadcast), so it lands in the same store the query/stats/SIEM paths read.

### Review repair (round 11, commit `07794b483`): per-handler repo/workspace-level ABAC guards now record AbacDenied

Review finding on candidate `1345a7cac` (checkpoint `4864719564`): the G6 kv-store repo-level ABAC checks enforced per-handler — `create_worktree` (POST /api/v1/repos/:id/worktrees), the four jj mutation endpoints, repo orchestrator spawn, the briefing/ask repo-scope guard, `spawn_agent_core` (REST agents/spawn + MCP gyre_spawn_worker), and the conversations workspace-read guard — rejected with 403 while recording no `abac_denied` audit event, leaving those denials invisible to the audit trail and SIEM forwarding. This was the ABAC-side equivalent of the round-8/9/10 per-handler RBAC sweep.

Every deny site now records the spec'd detail fields via the shared `abac_middleware::record_abac_denied` helper (the same helper the git push path already used): `resource`, `action`, `endpoint`, with the caller's JWT claims attached for forensics. `spawn_agent_core` takes the calling-surface endpoint label so the REST route (`POST /api/v1/agents/spawn`) and the MCP tool (`POST /mcp tools/call gyre_spawn_worker`) are named accurately.

Regression test `uninstrumented_abac_guards_record_abac_denied_audit_event` drives all nine surfaces against the full `build_router` with production-seeded builtin policies (`seed_builtin_policies`, as main.rs does), each sub-case on fresh state asserting exactly one `abac_denied` event with the expected resource/action/endpoint and outcome Blocked — including the reviewer's exact probe (Developer JWT, repo-level G6 policy requiring team=infra, POST /api/v1/repos/repo-1/worktrees → 403 + event). Mutation-verified: removing the worktrees or conversations emission fails the test.

### Source control events

- **GitPushAccepted** — emitted per *verified-applied* ref update in `git_receive_pack` post-receive, with `repo_id`, `branch`, `commit_sha`, `agent_id`, and `push_gate_results` (structured per-gate outcomes from `check_pre_accept_gates`). `verify_applied_ref_updates` (round 5) confirms via `git rev-parse` that each requested update actually landed before any acceptance signaling, so a git-declined (non-fast-forward/stale-client) push is not audited as accepted; `GitPushRejected` stays reserved for gate rejections per the spec trigger table.
- **GitPushRejected** — emitted at every push-gate rejection path (pre-accept gates with the rejecting `gate_name`; constraint, manifest-coverage, and spec-link cycle gates with their gate labels), with `reason`, outcome `Blocked`.
- **GitClone** — emitted by `git_upload_pack` after a successful smart-HTTP clone/fetch.
- **SpecChanged** — emitted by the spec-lifecycle hook for each spec change on the default branch, with `spec_path`, `change_kind`, and the auto-created `task_id` (null when deduped against an existing task).

### Access control events

- **AuthSuccess / AuthFailure** — wired into the `AuthenticatedAgent` extractor, the global `require_auth_middleware`, and the WebSocket `authenticate_token` path. `token_kind` distinguishes global / agent_jwt / uuid_token / api_key / user_jwt / federated_jwt; failures carry `rejection_reason` (invalid, expired, revoked). A request-extension marker dedupes AuthSuccess to exactly one event per request even when the token is validated by both the ABAC middleware and the handler extractor.
- **AbacDenied** — emitted from the ABAC middleware deny path (with `policy_id` from the matched policy, `resource`, `action`, `claims`), from the repo-level ABAC check that guards git push, and (round 11) from every per-handler repo/workspace-level G6 guard: worktrees, jj new/squash/undo/bookmark, repo orchestrator spawn, briefing/ask repo scope, agent spawn (REST + MCP), and the conversations workspace-read guard.
- **RbacDenied** — emitted from the `RequireAgent` extractor, from every live pure per-handler role guard in `api/` (budget, compute targets, llm config/prompts, meta-specs incl. preview, registry list and rollout policy, release, spec/stack policy, spec approve/reject, tenants, trust anchors, users, secrets, network peers incl. register/update/delete, message broadcast, MR approve guard), and from the MCP tools/call write-role gate (round 10), with `required_role`, `actual_role`, `endpoint`. Owner-or-Admin hybrid guards that reject on ownership, not role, are a different denial class and are not RbacDenied emissions.
- **ImpersonationStarted / ImpersonationEnded** — enum variants, as_str/from_str, and CEF severity 7 exist; emission points are deferred to task-108 (impersonation itself is not implemented yet), per the task plan.

### CEF severity mapping

`format_cef` maps per spec: AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4. All 10 severities asserted field-by-field in tests.

### Test evidence

Focused tests drive real behavior: a real smart-HTTP push with a configured gate (accepted event asserts gate results), a gate-rejected push (403 + rollback + rejected event, no accepted event), a git-declined non-fast-forward push (no accepted/rejected events, ref unchanged), a real `git clone` over a local listener, a spec-file push (spec_changed with task_id), invalid/revoked token rejections, the per-request AuthSuccess dedupe, the ABAC policy denial paths (middleware, git push, and the nine round-11 per-handler guards), and the RbacDenied paths (extractor, handler guards, the round-8/9 guards, and the round-10 MCP tools/call gate).

At the round-11 repair head `07794b483` (on merged base `4e895f766` via merge `1ba9d2404`, plus base `fefde2946` task-089 merge `4b9589c8c`):

- `cargo test -p gyre-server --lib`: **1501 passed, 0 failed** (includes the round-8/9/10/11 regression tests plus the task-103 and task-089 tests that arrived with the base merges).
- Mutation checks: removing the worktrees or conversations `record_abac_denied` emission fails the round-11 regression test; earlier rounds' mutation checks (rollout-policy, secrets, MCP-gate RbacDenied) still hold.
- `cargo fmt --all --check` clean; `cargo clippy -p gyre-server --lib` has zero findings in touched files (remaining warnings pre-existing in explorer_ws.rs / gyre-ports from the task-089 base merge).
- Round-11 probe evidence: `/tmp/stage/review-evidence/task-104-round11-*.log`.
- Full workspace suites, arch checks, and GitHub CI are owned by verification.

### Repair and merge history (rounds 1–10, summarized)

- Rounds 1–2: implementation `ac5f88365` preserved through merges of bases `233664cf8` (task-087) and `72ad2a128` (task-096).
- Round 5 (base `414fe9aa1b`, task-120): git-declined ref updates no longer record GitPushAccepted (`ca905fe43`, `verify_applied_ref_updates`).
- Round 6 (base `c5deca801a`, task-105, merge `0aad7a51a`): coverage-table conflict recomputed row-by-row; no source touched.
- Round 7 (verification repair `8e6ed8ecc` on candidate `6336881de`, finding `0a67d2762`): exemption-free JWT tenant-claim serde default (task-103 precedent), clippy and rustfmt findings fixed; no emission logic changed.
- Round 8 (commit `47190ed90`, finding `a42d6a7254` on candidate `90ee7e745`): five uninstrumented role guards (meta-specs preview, meta-specs registry list, spec approve/reject, MR approve) now record RbacDenied; regression test `uninstrumented_role_guards_record_rbac_denied_audit_event`.
- Round 9 (commit `0e487df7f`, finding `301748df18` on candidate `0fbf50527`): five more uninstrumented role guards (update_peer / delete_peer Admin guards in network.rs, put_rollout_policy in meta_specs.rs, the secrets `require_admin` helper covering four endpoints, the messages broadcast guard) now record RbacDenied via the shared helper; regression test `round9_uninstrumented_role_guards_record_rbac_denied_audit_event`.
- Round 10 (commit `48edaaf9a`, finding `aa4a2beed` F1 on candidate `ba29036ef`): the MCP tools/call write-role gate records RbacDenied; regression test `mcp_write_tool_role_denial_records_rbac_denied_audit_event`. Also merged task-103 base `4e895f766` (merge `1ba9d2404`), resolving the additive audit-event conflicts as the union of both event sets.
