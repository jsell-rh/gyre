# Review — task-104 (Add access control and source control audit event types)

Spec: `observability.md` §Audit Event Types.
Candidate `bf6d39569` on base `414fe9aa1b`; product commits `ac5f88365` (implementation) and `ca905fe43` (round-5 repair: git-declined ref updates no longer record GitPushAccepted). Merge commits bring in bases `233664cf8`/`72ad2a128`/`a884329d7`/`414fe9aa1b`; attribution frontmatter lists both product commits.
Verdict: **approved** (round 6; independent re-verification of the round-5 repair and the full task contract).

## Probes (evidence in `/tmp/stage/review-evidence/`: `t104-audit-tests.log`, `t104-cef-test.log`, `t104-domain-audit.log`, `t104-server-lib-full.log`, `t104-regression-restored.log`, `t104-abac-exempt.log`, `t104-dead-kinds.log`, `task-104-check-*.log`, `task-104-mutation-test.log`, `task-104-full-diff.txt`; the harness truncates longer names to colliding `task-1....log` dirents whose contents are the same runs)

- `cargo test -p gyre-server --lib audit_event` → **20 passed, 0 failed** — all task-104 emission tests: `push_records_git_push_accepted_audit_event` (real smart-HTTP push with configured conventional-commit gate, asserts `push_gate_results` content, branch, commit SHA, envelope scoping), `push_declined_by_git_records_no_git_push_accepted_audit_event` (round-5 regression: stale-client non-fast-forward push → HTTP 200 with git's report, ref unchanged at `tip`, zero accepted/rejected events after a 2 s grace period), `push_rejected_by_gate_records_git_push_rejected_audit_event` (403 + ref rollback + rejected event with gate_name/reason, no accepted event), `push_spec_change_records_spec_changed_audit_event` (spec_path/change_kind/task_id), `clone_records_git_clone_audit_event` (real `git clone` over a local TCP listener through the smart-HTTP handlers), 4 auth tests (global-token success, once-per-request dedupe through `build_router`, invalid-token and revoked-JWT failures with reasons), 1 ABAC denial (policy_id/resource/action/claims), 2 RBAC denials (RequireAgent extractor + per-handler tenant guard).
- `cargo test -p gyre-server --lib cef_severity` → **1 passed** — all 10 new CEF severities asserted field-by-field against the task plan's mapping.
- **Mutation check (independent):** forced the pre-fix behavior in `verify_applied_ref_updates` (`is_applied = true`) → the round-5 regression test FAILS on the recorded false `git_push_accepted` event (detail shows the stale commit SHA that never landed); restored the file (tree verified clean via `git status --porcelain`) → test passes again. The regression test is not self-confirming.
- `cargo test -p gyre-server --lib` (full) → **1431 passed, 0 failed** — matches the shipped claim.
- `cargo test -p gyre-domain --lib audit` → **9 passed** — enum as_str/from_str round-trip for all 10 new types.
- Mechanical gates: `check-arch.sh`, `check-abac-exempt-handlers.sh`, `check-dead-message-kinds.sh`, `check-fabricated-scope-defaults.sh`, `check-forwarded-header-trust.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh` → all exit 0.

## Verified against the acceptance criteria

- **All 10 event types** in `AuditEventType` with `as_str`/`from_str` (round-trip test) — done.
- **GitPushAccepted** — per *verified-applied* ref update in the post-receive block, with `repo_id`, `branch`, `commit_sha`, `agent_id`, `push_gate_results` (structured `PushGateOutcomeEntry` per configured gate). The round-5 `verify_applied_ref_updates` uses `git rev-parse --verify --end-of-options <refname> == new_sha` (with a `refs/` prefix guard — refnames are client-controlled) before any acceptance signaling; all post-receive consumers (agent-commit recording, PushAccepted message, audit loop, spec lifecycle, ledger sync, graph extraction) now process only applied updates.
- **GitPushRejected** — at every gate-rejection path (pre-accept gates with the rejecting `gate_name`; constraint-enforcement, manifest-coverage, spec-link-cycle gates with their labels), outcome `Blocked`; no accepted event for a rejected push (asserted).
- **GitClone** — in `git_upload_pack` after `run_git_stateless` succeeds (failure path returns before emission), detail `repo_id`/`agent_id`.
- **SpecChanged** — in the spec-lifecycle hook for each spec change on the default branch, with `spec_path`, `change_kind`, and the auto-created `task_id` (null on dedup/failure paths, which still emit the event — the spec file changed regardless).
- **AuthSuccess/AuthFailure** — extractor, global `require_auth_middleware`, and WS `authenticate_token`. `token_kind` covers global/agent_jwt/uuid_token/api_key/user_jwt/federated_jwt; failures carry `rejection_reason` (invalid, expired, revoked). A request-extension `AuthAuditMark` dedupes AuthSuccess to exactly one event per request (proven through `build_router` with both ABAC middleware and handler extraction). Failure paths are single-shot in practice: extraction failure terminates the request at the first extraction point (the ABAC middleware returns the error response; the global middleware rejects before extractors run). Forensic `source_ip` comes from `ConnectInfo`, never forwarded headers (CWE-348 gate passes).
- **AbacDenied** — ABAC middleware deny path (with `policy_id` from the matched policy) and the repo-level push ABAC check; detail `agent_id`/`policy_id`/`resource`/`action`/`claims`.
- **RbacDenied** — `RequireAgent` extractor and every live per-handler role guard (budget, compute targets, llm config/prompts, meta-specs, network peers, release, spec/stack policy, tenants, trust anchors, users) with `required_role`/`actual_role`/`endpoint`; all additions follow the identical at-deny-site pattern (verified by inspection of every hunk).
- **ImpersonationStarted/Ended** — variants, round-trip, CEF severity 7; emission deferred to task-108 per the task plan.
- **CEF mapping** — matches the plan exactly (AuthFailure 6; AbacDenied/RbacDenied/GitPushRejected 5; AuthSuccess/GitPushAccepted/GitClone 3; Impersonation* 7; SpecChanged 4).
- **cargo test** — full `gyre-server --lib` 1431/0 at the candidate head (this review); prior-round full-workspace evidence in the shipped notes.

Bookkeeping: the `fabricated-scope-defaults` exemption change only re-points the unchanged `auth.rs` JWT tenant fallback site (773→1041, shifted by this branch's auth.rs additions) and lowers `FROZEN_EXEMPTION_COUNT` 4→3 to match the actual entry count — count went down, no entries added, script passes. Coverage row 5 updated to `implemented` with an honest 19/34 note retaining the task-103 gaps.

## Non-findings (checked, judged acceptable)

- **Branch-deletion pushes emit no GitPushAccepted.** `verify_applied_ref_updates` confirms updates via `rev-parse <refname> == new_sha`; a deletion (new SHA = zeros) fails that check and is excluded, so a deletion-only push emits no audit event and a misleading "git receive-pack declined" warn. Spec-ambiguous edge: the spec trigger table has no deletion event type and requires a `commit_sha` detail field that does not exist for a deletion; pre-change post-receive consumers were already no-ops for deletions (`git log old..0000…` fails and warns). Not a contract violation; worth handling (e.g. a deletion-aware check or a distinct event type) if the spec ever grows one.
- **`record_auth_failure` calls are not guarded by the dedup mark.** Unreachable double-count: a failed extraction aborts the request at the first extraction point, so no second extraction follows.
- **Repo-level push ABAC `AbacDenied` carries `policy_id: null`** — that check is a repo-permission deny, not a named policy; the middleware path names the matched policy. Acceptable.
- WebSocket auth events are wired (`authenticate_token`) but not directly tested; the HTTP paths exercising the same emission helpers are.

## Verdict

Every acceptance criterion is met by real production code with emission points at the actual decision sites, persisted through the shared `emit_server_audit_event` helper into the AuditRepository store the query/stats/SIEM paths read. Tests drive real git subprocesses over the real smart-HTTP handlers and assert against the real audit store; the round-5 regression test was independently mutation-verified in this review. No new exemptions, no gate weakening, no unrelated changes. **progress: complete.**

— Reviewer, 2026-10-09
