---
title: "Add access control and source control audit event types"
spec_ref: "observability.md §Audit Event Types"
depends_on:
  - task-102
progress: ready-for-review
coverage_sections:
  - "observability.md §Audit Event Types"
commits: ["8e6ed8eccd3ce7243d411c2378d95cccf473655a", "ca905fe432f76092f9914513d736b53bfd800631", "ac5f88365dfcde25d5ae325fd24bebb90203c99b"]
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

### Verification repair (round 7, commit `8e6ed8ecc`): gate findings on candidate `6336881de`

Verification finding `0a67d2762112405b8b1110b32debeb81` (checks.sh exit 1) had three failure classes, all repaired without touching any task-104 emission behavior:

- **fabricated-scope-defaults / exemption freeze**: the legacy auth.rs JWT `claims.tenant_id` use-site fallback (exempted at line 773 in base `c5deca801a`) was renumbered to 1041 by this branch's AuthSuccess/AuthFailure wiring. The exemption freeze reads a renumbered entry as a new entry, and the upstream verifier overlay flags the renumbered site as unexempted. Fixed per the task-103 precedent (`cead5a881`, same site on its own branch): the "absent claim = default tenant" single-tenant IdP contract is now the claim's serde schema default (`#[serde(default = "default_tenant_claim")]` applied at deserialization), not a use-site `unwrap_or` fallback. Behavior unchanged and test-pinned (`jwt_missing_tenant_id_defaults_to_default`). Exemption entry deleted, `FROZEN_EXEMPTION_COUNT` lowered 3→2.
- **clippy-diff vs base**: `siem.rs` `assert_eq!(.., true)` bool comparison (now `assert!`); `abac_middleware.rs` unused `Result` from `block_on(seed_builtin_policies)` in the task-104 ABAC denial test (now `.expect("seed built-in policies")`, matching sibling tests).
- **rustfmt-diff vs base**: unformatted changed lines in `auth.rs`, `git_http.rs`, and the task-104 RbacDenied role-guard call sites; `cargo fmt --all` applied (formatting-only diff).

Verified on this checkout: `check-fabricated-scope-defaults.sh` passes with both the candidate and the base (overlay-simulated) script version; arch/scope-literal/inert-enforcement/lossy-secret checks OK; `cargo fmt --all --check` clean; `cargo clippy -p gyre-server --all-targets` has zero findings in any touched file; focused tests 28 passed / 0 failed (JWT tenant resolution 7, CEF severity 4, task-104 audit-event suite 17). Evidence: `/tmp/stage/review-evidence/task-104-verification-repair-*.log`.

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
- Round 5 (merge of base `414fe9aa1b`, task-120 — merge commit `13a71b428`): fixed the review finding above in `ca905fe43`; no other task-104 file touched.
- Round 6 (merge of base `c5deca801a`, task-105 — merge commit `0aad7a51a`): rebase conflict resolved in `specs/coverage/system/observability.md`; only the Coverage summary line conflicted (task-105's upstream commit `cd430c8b1` left a stale `6/9` fraction whose own parenthetical already summed to 7). Resolved to `8/9 (0 n/a, 0 not-started, 1 task-assigned, 5 implemented, 3 verified)`, recomputed row-by-row from the merged table (row 5 implemented via task-104, row 9 implemented via task-105; convention coverage = implemented + verified). No task-104 source file touched by the merge. Post-merge probes at `0aad7a51a`: `cargo test -p gyre-server --lib` 1443 passed / 0 failed (evidence: `/tmp/stage/review-evidence/task-104-rebase-repair.md`).
- Round 7 (verification repair of candidate `6336881de`, finding `0a67d2762`): gate findings above fixed in `8e6ed8ecc` on head `6336881de`; no merge involved, no task-104 emission logic changed.
