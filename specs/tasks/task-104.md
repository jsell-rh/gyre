---
title: "Add access control and source control audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: complete
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["738e4fed58a2134e2db4b1bf4ca30595786719b7", "2d4faab3a1d1038a4a6354a50823645c25e01257", "07794b483ab9bddab2f38e2e32310823c5b79f7d", "48edaaf9a1bb9e24e4f9ddaf2e452402ad478c83", "0e487df7fb413c2d57998cf1d0c2c53818a2ba28", "47190ed901d978fa05f6e377fe436cba52f4be7b", "8e6ed8eccd3ce7243d411c2378d95cccf473655a", "ca905fe432f76092f9914513d736b53bfd800631", "ac5f88365dfcde25d5ae325fd24bebb90203c99b"]
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

All 10 event types from observability.md §Audit Event Types are implemented with real production emission points (implementation `ac5f88365`, repaired through rounds 5–15; current head `738e4fed` on merge `4c62a8064`). Every event is recorded through the shared `api::audit::emit_server_audit_event` helper (AuditRepository port + SSE broadcast), so it lands in the same store the query/stats/SIEM paths read.

### Verification repair (round 15, commit `738e4fed`): task-106 base merge — 4-arg `spawn_agent_core` test calls and the round-11 clippy borrows

Verification finding (checks exit 1) on the candidate after merging base `402142b1c` (task-106 automatic jj rebase and jj workspace provisioning, carrying task-116 meta-spec prompt assembly) into the branch (merge `4c62a8064`): `crates/gyre-server/src/api/spawn.rs` failed to build and lint —

- Five test call sites that arrived with the base drove `spawn_agent_core(&state, req, &auth)` with 3 arguments, but the round-11 repair had given the core a 4th `endpoint` parameter (the calling-surface label for the AbacDenied audit event), so `gyre-server` (lib test) failed to compile with five E0061 errors. Each site now passes a truthful `"spawn_agent_core (direct test drive)"` label — inert in these tests (no JWT claims → ABAC bypass); it only names the surface if the guard ever denies.
- The clippy gate flagged the round-11 `check_repo_abac` call in the core twice (`&state` and `&auth` — double borrows on the `state: &Arc<AppState>` / `auth: &AuthenticatedAgent` params). Both borrows dropped; call semantics unchanged.
- The three remaining `needless_borrow` findings in spawn.rs (`check_spawn_budget(&state, …)` line 408, `create_derived_input_for_agent(&state, …)` line 1443, `increment_active_agents(&state, …)` line 1463) are byte-identical at both merge parents and outside the verifier's changed-line scope; left untouched.

No emission logic changed: the production `spawn_agent_core` signature and both production callers (REST `POST /api/v1/agents/spawn`, MCP `gyre_spawn_worker`) were already correct — only the merged test call sites and the two borrows were repaired.

Evidence at head `738e4fed`: `cargo check -p gyre-server --all-targets` clean; clippy has no findings on any line touched by this repair; `cargo test -p gyre-server --lib` **1591 passed, 0 failed** (run twice; includes the five repaired task-106 spawn tests and every task-104 audit event test); `--test git_integration` 17 passed; `api::spawn::tests` 46 passed; `cargo fmt --all --check` clean. One filtered `--lib audit` subset run flaked once (49/50) and did not reproduce in four subsequent runs — the change is semantically inert (borrow removal + inert endpoint label), so that flake is pre-existing timing sensitivity, not a regression. Probe logs: `/tmp/stage/review-evidence/task-104-round15-*.log`.

### Rebase repair (round 14, merge `bd81b48e4`): task-141/task-177 base merge, union of the divergent git_http.rs test tails

Rebase finding `f74348f3b` on candidate `c4b978846`: merging base `fb1f82e31` (task-141 repo-defined persona push detection and approval reset, carried forward through the task-177 web-asset build and the deletion-compute-accounting PR #751) conflicted in `crates/gyre-server/src/git_http.rs`. The conflict was confined to the `mod tests` tail: both sides had appended divergent test blocks after the same shared last test (the MR lineage re-evaluation test), and the merge aligned their similar git-command scaffolding into six interleaved conflict hunks. Resolved as a plain union of both sides:

- HEAD's task-104 test helpers (`audit_events_of`, `wait_for_audit_event`, `git_with_input`) and all five source-control audit event tests (`push_records_git_push_accepted_audit_event`, `push_declined_by_git_records_no_git_push_accepted_audit_event`, `push_rejected_by_gate_records_git_push_rejected_audit_event`, `push_spec_change_records_spec_changed_audit_event`, `clone_records_git_clone_audit_event`) kept.
- The base's task-141 test helpers (`persona_content_hash`, `git_bare`, `write_blob`, `write_tree`, `commit_files`, `push_ref`, `wait_for_task_title`) and all persona push detection tests (`parse_persona_changes_*`, `persona_slug_from_path_direct_files_only`, `persona_approval_task_shape`, `push_persona_change_updates_entity_resets_approval_and_notifies_owner`, `push_new_persona_file_registers_pending_repo_scoped_entity`, `push_without_persona_changes_leaves_personas_untouched`) kept.
- No helper-name collisions between the two tails; the shared test scaffolding (`pkt_line`, `git_app_with_repo`) is unchanged and serves both.
- The production code auto-merged cleanly with both sides intact: all task-104 emission points (`emit_git_clone_audit`, `emit_git_push_rejected_audit` at the four gate-rejection paths, `emit_git_push_accepted_audit`, `emit_spec_changed_audit` in `process_spec_lifecycle`) coexist with the base's `process_persona_changes` call in post-receive. The base's production changes were independently reviewed (task-141, task-177, PR #751); no new emission repair was required — the base's new code paths introduce no auth rejection or gate denial surfaces.

Regression coverage over the resolved region is both sides' test suites in one module: all five task-104 source-control audit event tests and all nine task-141 persona push detection tests pass at the merge head, so neither side's assertions were weakened by the union.

### Rebase repair (round 13, merge `f32cd89d7`): task-138 base merge, union resolution in the spec-lifecycle hook

Rebase finding `0b70463af` on candidate `e633c9a16`: merging base `22f275e26a` (task-138 spec approval ledger) conflicted in `git_http.rs` `process_spec_lifecycle`. The two sides had restructured the same region differently: task-104 resolves the repo's workspace scope per spec change (`ws_id_opt`) and `continue`s past the workspace events when the repo row is unresolvable; task-138 resolves `ws_id` once per push above the change loop and emits the workspace message events conditionally (`if let Some`), so cross-workspace notification still runs when the repo row is unresolvable. Resolved as a union of both semantics:

- The base's once-per-push `ws_id` resolution is kept (warn + `None` on lookup failure); task-104's redundant per-change `ws_id_opt` block — the same lookup resolved later and with worse logging — is removed.
- All three task-104 `SpecChanged` audit emissions (dedup-skip, create-Err, create-Ok) are scoped via `ws_id.as_ref()`: a real scope when the repo resolves, no event scoping when it does not, never a fabricated identity (task-097 F3 class). The dedup-skip emission, previously unscoped only because the old lookup sat after the dedup check, is now scoped like the others.
- The create-Ok arm keeps the audit emission with the auto-created `task_id` and adopts the base's conditional workspace-event emission (no `continue`); the base's duplicate inner `change_kind` re-computation is dropped in favor of the outer per-change binding that already feeds the audit events.
- The base's approval-invalidation block (auto-merged) and its auto-merged `api/specs.rs` / `mem.rs` / `git_refs.rs` / `merge_processor.rs` changes are taken as-is. The base's new revoke-approval deny path is an Owner-or-Admin ownership guard — the denial class explicitly excluded from RbacDenied emissions — and task-138 was independently reviewed, so no new emission repair was required.

Regression coverage is the existing pair of tests over the resolved region: `push_spec_change_records_spec_changed_audit_event` (task-104: exactly one `spec_changed` event with repo_id/spec_path/change_kind/task_id) and `push_modifying_spec_revokes_ledger_approvals` (task-138: push invalidates stale approvals) both pass at the merge head.

### Rebase repair (round 12, merge `451407535` + commit `2d4faab3a`): task-100 base merge, union resolutions, and AuthFailure on the new fail-closed API-key rejection

Rebase finding `c112f2c10` on candidate `cec4cb448`: merging base `7373979e8` (task-100 secrets rotation) conflicted in `auth.rs`, `git_http.rs`, and `scripts/fabricated-scope-defaults-exemptions.txt`. All three resolved as unions of both sides' semantics:

- `auth.rs` — the task-104 auth-audit helpers and the base's `impl AuthenticatedAgent::is_system_principal` (task-099 spoof-discriminator, called by the auto-merged ABAC/tenant-containment guards) both kept; in the two API-key paths, the base's task-099 F1 fail-closed tenant-binding check now runs *before* the task-104 `record_auth_success` emission, so a user without a tenant binding (403) never emits AuthSuccess.
- `git_http.rs` — `git_upload_pack` keeps the task-104 `resolve_repo_by_slug` call (the repo record feeds the GitClone event) with the base's `&auth` resolver signature (system-principal bootstrap fallback).
- `fabricated-scope-defaults-exemptions.txt` — HEAD's superset documentation kept; both sides agree on the same 2 remaining entries and frozen count 2.

The base's fail-closed API-key rejection was a *new* auth rejection path with no audit emission — the same uninstrumented-rejection class as the round-8..11 sweeps. Commit `2d4faab3a` records an `AuthFailure` audit event (`token_kind=api_key`, `rejection_reason="user has no tenant binding; re-provision via POST /api/v1/users"`) on both API-key paths (handler extractor and the WebSocket `authenticate_token` path) before the 403. Regression test `auth_failure_audit_event_recorded_for_api_key_user_without_tenant_binding` asserts exactly one `auth_failure` event with the spec'd detail fields; mutation-verified (removing the emission fails the test).

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

- **AuthSuccess / AuthFailure** — wired into the `AuthenticatedAgent` extractor, the global `require_auth_middleware`, and the WebSocket `authenticate_token` path. `token_kind` distinguishes global / agent_jwt / uuid_token / api_key / user_jwt / federated_jwt; failures carry `rejection_reason` (invalid, expired, revoked, and — round 12 — user-has-no-tenant-binding). A request-extension marker dedupes AuthSuccess to exactly one event per request even when the token is validated by both the ABAC middleware and the handler extractor.
- **AbacDenied** — emitted from the ABAC middleware deny path (with `policy_id` from the matched policy, `resource`, `action`, `claims`), from the repo-level ABAC check that guards git push, and (round 11) from every per-handler repo/workspace-level G6 guard: worktrees, jj new/squash/undo/bookmark, repo orchestrator spawn, briefing/ask repo scope, agent spawn (REST + MCP), and the conversations workspace-read guard.
- **RbacDenied** — emitted from the `RequireAgent` extractor, from every live pure per-handler role guard in `api/` (budget, compute targets, llm config/prompts, meta-specs incl. preview, registry list and rollout policy, release, spec/stack policy, spec approve/reject, tenants, trust anchors, users, secrets, network peers incl. register/update/delete, message broadcast, MR approve guard), and from the MCP tools/call write-role gate (round 10), with `required_role`, `actual_role`, `endpoint`. Owner-or-Admin hybrid guards that reject on ownership, not role, are a different denial class and are not RbacDenied emissions.
- **ImpersonationStarted / ImpersonationEnded** — enum variants, as_str/from_str, and CEF severity 7 exist; emission points are deferred to task-108 (impersonation itself is not implemented yet), per the task plan.

### CEF severity mapping

`format_cef` maps per spec: AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4. All 10 severities asserted field-by-field in tests.

### Test evidence

Focused tests drive real behavior: a real smart-HTTP push with a configured gate (accepted event asserts gate results), a gate-rejected push (403 + rollback + rejected event, no accepted event), a git-declined non-fast-forward push (no accepted/rejected events, ref unchanged), a real `git clone` over a local listener, a spec-file push (spec_changed with task_id), invalid/revoked token rejections, the round-12 API-key tenant-binding rejection (AuthFailure with reason), the per-request AuthSuccess dedupe, the ABAC policy denial paths (middleware, git push, and the nine round-11 per-handler guards), and the RbacDenied paths (extractor, handler guards, the round-8/9 guards, and the round-10 MCP tools/call gate).

At the round-14 head `bd81b48e4` (merge of base `fb1f82e31` task-141/task-177/deletion-compute-accounting, on top of the round-13 head lineage):

- `cargo test -p gyre-server --lib`: **1553 passed, 0 failed** (the round-8/9/10/11/12 regression tests plus the task-100, task-103, task-089, task-138, and task-141 tests that arrived with the base merges; 1544 before the round-14 base merge).
- `cargo test -p gyre-server --test git_integration`: 17 passed, 0 failed (includes the base's `push_modifying_spec_revokes_ledger_approvals`).
- `git_http::tests`: 54 passed, 0 failed — both sides' tests in one suite (five task-104 audit event tests + nine task-141 persona detection tests over the resolved tail).
- Mutation checks: removing the round-12 extractor-path AuthFailure emission fails `auth_failure_audit_event_recorded_for_api_key_user_without_tenant_binding`; the round-11 worktrees/conversations checks and earlier rounds' checks (rollout-policy, secrets, MCP-gate RbacDenied) still hold.
- `cargo fmt --all --check` clean; `cargo clippy -p gyre-server --lib` findings in `git_http.rs` are the same 13 pre-existing at both the candidate and the merged base (verified by running clippy on each side's version of the file); none in code touched by the round-14 resolution.
- `scripts/check-arch.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh`, `check-migration-versions.sh`, `check-byte-slice-truncation.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, and `check-forwarded-header-trust.sh` all pass.
- Round-14 probe evidence: `/tmp/stage/review-evidence/task-104-rebase-round14*.log`.
- Full workspace suites, arch checks, and GitHub CI are owned by verification.

### Repair and merge history (rounds 1–14, summarized)

- Round 15 (verification repair, commit `738e4fed`, on merge `4c62a8064` of base `402142b1c` task-106/task-116): five merged task-106 spawn tests adapted to the 4-arg `spawn_agent_core` (truthful direct-test-drive endpoint label); the round-11 `check_repo_abac` call's `&state`/`&auth` double borrows dropped for the clippy gate (details in the section above).
- Round 14 (rebase finding `f74348f3b`, merge `bd81b48e4` of base `fb1f82e31` task-141/task-177/PR #751): git_http.rs test-module tail conflict resolved as a plain union — both sides' helpers and tests kept, no collisions, production emission points and the base's `process_persona_changes` auto-merged intact (details in the section above).
- Round 13 (rebase finding `0b70463af`, merge `f32cd89d7` of base `22f275e26a` task-138): spec-lifecycle hook conflict resolved as a union — base's once-per-push `ws_id` resolution kept, task-104's redundant per-change lookup removed, all SpecChanged audit emissions scoped from the per-push resolution, base's conditional workspace-event emission adopted (details in the section above).
- Round 11 (commit `07794b483`, finding on candidate `1345a7cac`): nine uninstrumented per-handler repo/workspace ABAC guards now record AbacDenied via the shared `record_abac_denied` helper; regression test `uninstrumented_abac_guards_record_abac_denied_audit_event` (details in the section above). Round 12 (rebase finding `c112f2c10`) is described in its own section above.
- Rounds 1–2: implementation `ac5f88365` preserved through merges of bases `233664cf8` (task-087) and `72ad2a128` (task-096).
- Round 5 (base `414fe9aa1b`, task-120): git-declined ref updates no longer record GitPushAccepted (`ca905fe43`, `verify_applied_ref_updates`).
- Round 6 (base `c5deca801a`, task-105, merge `0aad7a51a`): coverage-table conflict recomputed row-by-row; no source touched.
- Round 7 (verification repair `8e6ed8ecc` on candidate `6336881de`, finding `0a67d2762`): exemption-free JWT tenant-claim serde default (task-103 precedent), clippy and rustfmt findings fixed; no emission logic changed.
- Round 8 (commit `47190ed90`, finding `a42d6a7254` on candidate `90ee7e745`): five uninstrumented role guards (meta-specs preview, meta-specs registry list, spec approve/reject, MR approve) now record RbacDenied; regression test `uninstrumented_role_guards_record_rbac_denied_audit_event`.
- Round 9 (commit `0e487df7f`, finding `301748df18` on candidate `0fbf50527`): five more uninstrumented role guards (update_peer / delete_peer Admin guards in network.rs, put_rollout_policy in meta_specs.rs, the secrets `require_admin` helper covering four endpoints, the messages broadcast guard) now record RbacDenied via the shared helper; regression test `round9_uninstrumented_role_guards_record_rbac_denied_audit_event`.
- Round 10 (commit `48edaaf9a`, finding `aa4a2beed` F1 on candidate `ba29036ef`): the MCP tools/call write-role gate records RbacDenied; regression test `mcp_write_tool_role_denial_records_rbac_denied_audit_event`. Also merged task-103 base `4e895f766` (merge `1ba9d2404`), resolving the additive audit-event conflicts as the union of both event sets.
