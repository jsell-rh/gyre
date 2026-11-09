---
title: "Add access control and source control audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["0e487df7fb413c2d57998cf1d0c2c53818a2ba28", "47190ed901d978fa05f6e377fe436cba52f4be7b", "8e6ed8eccd3ce7243d411c2378d95cccf473655a", "ca905fe432f76092f9914513d736b53bfd800631", "ac5f88365dfcde25d5ae325fd24bebb90203c99b"]
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

All 10 event types from observability.md §Audit Event Types are implemented with real production emission points (implementation `ac5f88365`, repaired through rounds 5–9; current head `0e487df7f`). Every event is recorded through the shared `api::audit::emit_server_audit_event` helper (AuditRepository port + SSE broadcast), so it lands in the same store the query/stats/SIEM paths read.

### Review repair (round 9, commit `0e487df7f`): five remaining uninstrumented role guards now record RbacDenied

Review finding `301748df18` F1/F2 (on candidate `0fbf50527`): the update_peer and delete_peer Admin guards (network.rs) and the put_rollout_policy Admin guard (meta_specs.rs, arrived via the task-157 base merge) rejected with 403 while recording no `rbac_denied` audit event — the same class as the round-8 finding. A same-class sweep of every handler in `api/` found two more sites that would keep the "all live per-handler role guards" claim false: the secrets `require_admin` helper (four endpoints, arrived via the task-098 base merge) and the send_message broadcast guard. All five now record the spec'd detail fields (`user_id`, `required_role`, `actual_role`, `endpoint`) via the shared `rbac::record_rbac_denied` helper:

- `PUT /api/v1/network/peers/:id` (Admin guard for non-JWT callers, update_peer, network.rs)
- `DELETE /api/v1/network/peers/:id` (Admin guard for non-JWT callers, delete_peer, network.rs)
- `PUT /api/v1/workspaces/:id/meta-specs/rollout-policy` (Admin guard, put_rollout_policy, meta_specs.rs)
- `POST /api/v1/secrets`, `GET /api/v1/secrets`, `DELETE /api/v1/secrets/:id`, `POST /api/v1/secrets/:id/rotate` (Admin guard via the shared `require_admin` helper, secrets.rs)
- `POST /api/v1/workspaces/:workspace_id/messages` with a Broadcast destination (Server-or-Admin guard, messages.rs)

Hybrid owner-or-Admin guards that reject on ownership rather than role (messages inbox poll/ack, saved views, explorer views, key bindings, spec approval revocation) are a different denial class and remain uninstrumented as RbacDenied.

Regression test `round9_uninstrumented_role_guards_record_rbac_denied_audit_event` covers all five sites against the full `api_router`, including the reviewer's exact probes (uuid agent token → PUT/DELETE on another agent's peer → 403 with exactly one `rbac_denied` event; Developer JWT → PUT rollout-policy → 403 with exactly one `rbac_denied` event). Mutation-verified: deleting the rollout-policy or secrets emission makes the test fail on the missing event.

### Source control events

- **GitPushAccepted** — emitted per *verified-applied* ref update in `git_receive_pack` post-receive, with `repo_id`, `branch`, `commit_sha`, `agent_id`, and `push_gate_results` (structured per-gate outcomes from `check_pre_accept_gates`). Round-5 repair (`ca905fe43`): `verify_applied_ref_updates` confirms via `git rev-parse` that each requested update actually landed before any acceptance signaling, so a git-declined (non-fast-forward/stale-client) push is not audited as accepted; `GitPushRejected` stays reserved for gate rejections per the spec trigger table.
- **GitPushRejected** — emitted at every push-gate rejection path (pre-accept gates with the rejecting `gate_name`; constraint, manifest-coverage, and spec-link cycle gates with their gate labels), with `reason`, outcome `Blocked`.
- **GitClone** — emitted by `git_upload_pack` after a successful smart-HTTP clone/fetch.
- **SpecChanged** — emitted by the spec-lifecycle hook for each spec change on the default branch, with `spec_path`, `change_kind`, and the auto-created `task_id` (null when deduped against an existing task).

### Access control events

- **AuthSuccess / AuthFailure** — wired into the `AuthenticatedAgent` extractor, the global `require_auth_middleware`, and the WebSocket `authenticate_token` path. `token_kind` distinguishes global / agent_jwt / uuid_token / api_key / user_jwt / federated_jwt; failures carry `rejection_reason` (invalid, expired, revoked). A request-extension marker dedupes AuthSuccess to exactly one event per request even when the token is validated by both the ABAC middleware and the handler extractor.
- **AbacDenied** — emitted from the ABAC middleware deny path (with `policy_id` from the matched policy, `resource`, `action`, `claims`) and from the repo-level ABAC check that guards git push.
- **RbacDenied** — emitted from the `RequireAgent` extractor and from every live pure per-handler role guard (budget, compute targets, llm config/prompts, meta-specs incl. preview, registry list and rollout policy, release, spec/stack policy, spec approve/reject, tenants, trust anchors, users, secrets, network peers incl. register/update/delete, message broadcast, MR approve guard) with `required_role`, `actual_role`, `endpoint`. Owner-or-Admin hybrid guards that reject on ownership, not role, are a different denial class and are not RbacDenied emissions.
- **ImpersonationStarted / ImpersonationEnded** — enum variants, as_str/from_str, and CEF severity 7 exist; emission points are deferred to task-108 (impersonation itself is not implemented yet), per the task plan.

### CEF severity mapping

`format_cef` maps per spec: AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4. All 10 severities asserted field-by-field in tests.

### Test evidence

Focused tests drive real behavior: a real smart-HTTP push with a configured gate (accepted event asserts gate results), a gate-rejected push (403 + rollback + rejected event, no accepted event), a git-declined non-fast-forward push (no accepted/rejected events, ref unchanged), a real `git clone` over a local listener, a spec-file push (spec_changed with task_id), invalid/revoked token rejections, the per-request AuthSuccess dedupe, an ABAC policy denial, and the RbacDenied paths (extractor, handler guard, the five round-8 guards, and the five round-9 guard sites).

At the round-9 repair head `0e487df7f` on merged base `71a02247c` (task-098 base merge):

- `cargo test -p gyre-server --lib`: **1481 passed, 0 failed** (includes the round-8 and round-9 regression tests).
- Mutation checks: removing the rollout-policy or the secrets `record_rbac_denied` emission fails `round9_uninstrumented_role_guards_record_rbac_denied_audit_event`; with the emissions it passes.
- `cargo fmt --all --check` clean; `cargo clippy -p gyre-server --lib` has zero findings in touched files.
- Round-9 probe evidence: `/tmp/stage/review-evidence/task-104-round9-*.log`.
- Full workspace suites, arch checks, and GitHub CI are owned by verification.

### Repair and merge history (rounds 1–8, summarized)

- Rounds 1–2: implementation `ac5f88365` preserved through merges of bases `233664cf8` (task-087) and `72ad2a128` (task-096).
- Round 5 (base `414fe9aa1b`, task-120): git-declined ref updates no longer record GitPushAccepted (`ca905fe43`, `verify_applied_ref_updates`).
- Round 6 (base `c5deca801a`, task-105, merge `0aad7a51a`): coverage-table conflict recomputed row-by-row; no source touched.
- Round 7 (verification repair `8e6ed8ecc` on candidate `6336881de`, finding `0a67d2762`): exemption-free JWT tenant-claim serde default (task-103 precedent), clippy and rustfmt findings fixed; no emission logic changed.
- Round 8 (commit `47190ed90`, finding `a42d6a7254` on candidate `90ee7e745`): five uninstrumented role guards (meta-specs preview, meta-specs registry list, spec approve/reject, MR approve) now record RbacDenied; regression test `uninstrumented_role_guards_record_rbac_denied_audit_event`.
