# Review — task-104 (Observability §Audit Event Types — Source Control + Access Control Events)

Spec: `specs/system/observability.md` §Audit Event Types (source control + access control tables).
Candidate under review: `6336881de` (round 6 — rebase repair merging base `c5deca801a`, task-105).
Implementation commits: `ac5f88365` (original), `ca905fe43` (round-5 repair: git-declined ref
updates no longer record GitPushAccepted).
Verdict: **approved** (round 6; rounds 1–5 findings verified fixed or absent).

## Round 6 (rebase repair)

What this round changed: merge commit `0aad7a51a` merged task-105 (log compression) into the
task-104 branch; the only conflicting file was `specs/coverage/system/observability.md`.

- `git diff bf6d39569..0aad7a51a -- crates/gyre-server/src/git_http.rs` is **empty** — the
  base side made no git_http.rs changes (verified: merge-base `414fe9aa1b`..`c5deca801a`
  touches no git_http.rs), so the merge took the task-104 side verbatim. No merge
  resolution risk in the core emission file.
- Coverage-summary resolution recomputed row-by-row from the merged table: verified rows
  1/3/7 (3), implemented rows 2/5/6/8/9 (5), task-assigned row 4 (1) → `8/9` — the
  resolved summary line is arithmetically correct. `grep -c '| not-started |'` → 0.
- Task frontmatter attribution intact: `ca905fe43` and `ac5f88365` both ancestors of HEAD.

## Independent probes (candidate head `6336881de`, working tree clean)

- `cargo test -p gyre-domain --lib audit` → 9/9 ok (event-type roundtrip included).
- `cargo test -p gyre-server --lib -- records_ cef_severity_source_control` → 12/12 ok,
  plus `-- auth_success_audit_event auth_failure_audit_event` → 4/4 ok. Together these
  drive the real behavior end-to-end: a real smart-HTTP push with a configured gate
  (accepted event asserts repo_id/branch/commit_sha/agent_id/push_gate_results and
  envelope scoping), a gate-rejected push (403 + ref rollback + rejected event, no
  accepted event), a git-declined non-fast-forward push (no accepted/rejected events,
  ref unchanged), a real `git clone` over a local listener, a spec-file push
  (spec_changed with task_id), invalid/revoked token rejections (auth_failure with
  reason), the per-request AuthSuccess dedupe, an ABAC policy denial
  (policy_id/resource/action/claims), both RbacDenied paths, and all 10 CEF severities.
- **Mutation probe**: forcing `is_applied = true` in `verify_applied_ref_updates`
  (the pre-round-5 behavior) makes
  `push_declined_by_git_records_no_git_push_accepted_audit_event` FAIL (exit 101);
  restored and re-run at the pristine candidate → passes. The regression test is
  genuinely sensitive to the production behavior, not self-confirming.
- Mechanical invariants all pass: check-arch, check-abac-route-registry,
  check-abac-exempt-handlers, check-dead-message-kinds, check-mem-port-contracts,
  check-inert-enforcement, check-fabricated-scope-defaults.
- Evidence: `/tmp/stage/review-evidence/task-104-round6-review.md` (+ per-probe logs).

## Verified against the acceptance criteria

- All 10 event types in `AuditEventType` with `as_str`/`from_str` (+ roundtrip test).
- GitPushAccepted emitted per *verified-applied* ref update with gate results;
  GitPushRejected at all four gate-rejection paths with gate_name/reason (outcome
  Blocked, refs rolled back); GitClone on upload-pack; SpecChanged via the spec-lifecycle
  hook with spec_path/change_kind/task_id (null task_id on dedup — spec file still
  changed, correctly still emits).
- AuthSuccess/AuthFailure wired in the extractor, the global auth middleware, and the
  WebSocket path; token_kind + rejection_reason per spec; source_ip taken from the
  socket peer address (ConnectInfo), never caller-controlled forwarded headers.
- AbacDenied from the middleware deny path and the repo-level push ABAC check;
  RbacDenied from the extractor and the live per-handler role guards.
- ImpersonationStarted/Ended variants + CEF severity 7 exist; emission deferred to
  task-108 per the task plan and acceptance criteria.
- CEF severity mapping matches the plan exactly (AuthFailure 6; AbacDenied/RbacDenied/
  GitPushRejected 5; AuthSuccess/GitPushAccepted/GitClone 3; Impersonation* 7;
  SpecChanged 4).
- Exemption-file maintenance is legitimate: fabricated-scope-defaults count **lowered**
  4→3 (a site fixed upstream), one entry re-pointed to a shifted line for the same
  unchanged code site; the check script passes.

Non-blocking note (no action required): the CEF severity test's panic message prints
`header[7]` (extension field) as the "got" value while asserting on `header[6]`
(severity) — assertion logic is correct, failure text only.
