# Review — task-104: Add access control and source control audit event types

**Candidate:** `cec4cb44822610982f5f05336c6a9d2007a03746` (head; process commit on round-11 repair `07794b483`)
**Base:** `fefde2946f3a0af1f212588f54591516fee0f7b2`
**Round:** 12 (independent)
**Date:** 2026-11-09
**Verdict:** Approved

## What was checked

### Acceptance criteria (all confirmed)

- **10 event types** in `AuditEventType` (gyre-domain/src/audit.rs:19-29) with `as_str`/`from_str`
  round-trips; spec'd snake_case strings verified.
- **GitPushAccepted** — emitted per *verified-applied* ref update only
  (`verify_applied_ref_updates` rev-parse check, git_http.rs:507); detail carries
  `repo_id`, `branch`, `commit_sha`, `agent_id`, `push_gate_results` (structured
  per-gate gate/passed/reason, git_http.rs:298-332). A git-declined
  (non-fast-forward) push records no accepted event (regression test covers it).
- **GitPushRejected** — all gate rejection paths (pre-accept, constraint,
  manifest-coverage, spec-link cycle) with `gate_name`/`reason`, outcome Blocked.
- **GitClone** — `git_upload_pack` after successful smart-HTTP clone/fetch.
- **SpecChanged** — spec-lifecycle hook on default-branch pushes with
  `spec_path`, `change_kind`, `task_id`.
- **AuthSuccess/AuthFailure** — auth extractor + global middleware + WS path;
  `token_kind` distinguishes global/agent_jwt/uuid_token/api_key/user_jwt/
  federated_jwt; failures carry `rejection_reason`; request-extension marker
  dedupes AuthSuccess to one per request (test asserts the dedupe).
- **AbacDenied** — middleware deny path (with `policy_id`), git-push repo-level
  check, and all nine round-11 per-handler G6 guards (worktrees, jj ×4,
  repo orchestrator spawn, briefing/ask repo scope, spawn_agent_core REST+MCP,
  conversations workspace-read) via shared `record_abac_denied` with
  resource/action/endpoint + claims.
- **RbacDenied** — `RequireAgent` extractor, all live pure per-handler role
  guards in api/ (rounds 8-9 sweep), MCP tools/call write-role gate (round 10).
- **ImpersonationStarted/Ended** — variants + CEF severity 7 exist; emission
  deferred to task-108 per task plan (impersonation not yet implemented).
- **CEF severity mapping** (siem.rs:156-190): AuthFailure 6, AbacDenied/
  RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3,
  Impersonation 7, SpecChanged 4 — matches spec; asserted field-by-field in
  `cef_severity_source_control_and_access_control_events`.

### Independent sweep for remaining uninstrumented deny sites (round-12 focus)

Enumerated every `check_repo_abac`/`check_workspace_abac_for_read` call site and
every `roles.contains(&UserRole::...)` occurrence in api/ + mcp.rs:

- All 11 ABAC policy-evaluation call sites record `abac_denied` (git_http,
  jj ×4, orchestrator, conversations, spawn, graph briefing/ask,
  agent_tracking worktrees).
- All pure role guards record `rbac_denied` (compute_targets, llm_config,
  llm_prompts, network, users:790, plus the shared-helper sites).
- Remaining un-instrumented 403s are NOT in the spec's trigger tables:
  tenant containment (audit.rs workspace/repo scope validation, workspaces.rs
  Admin-override scoping, explorer_views tenant checks), workspace-membership
  and owner-or-Admin ownership guards (explorer_views check_ownership,
  key_binding revoke), and a data-integrity check (recovery.rs MR-to-repo
  mismatch). Consistent with the classification held since round 8: RbacDenied
  triggers on *role check* rejection, AbacDenied on *ABAC policy* rejection.
  The SCIM bearer-token check (scim.rs) is a separate token mechanism outside
  the spec's `token_kind` list — pre-existing surface, not a task-104 gap.

### Probes run (evidence: /tmp/stage/review-evidence/)

1. `cargo test -p gyre-server --lib` (full): **1501 passed, 0 failed**
   (task-104-r12-full-lib.log, task-104-r12-focused-tests.log).
2. **Independent mutation check**: removed the conversations
   `record_abac_denied` emission in api/conversations.rs →
   `uninstrumented_abac_guards_record_abac_denied_audit_event` FAILED
   ("one abac_denied event for GET /api/v1/conversations/:sha left: 0
   right: 1"); file restored, working tree clean, test green again. The test
   kills real bugs, not self-confirming.
3. Mechanical checks on touched surfaces: `check-fabricated-scope-defaults.sh`
   OK (exemption count correctly *lowered* 3→2 — a fixed site removed, the
   mandated direction), `check-mcp-write-tools.sh` OK (8 tools gated),
   `check-abac-exempt-handlers.sh` OK (88 handlers),
   `check-task-commit-attribution.sh` OK.
4. Round-11 regression test drives the full `build_router` with
   production-seeded builtin policies — real enforcement path, real G6
   kv-store policies, exact event counts and detail fields asserted per
   surface.

## Notes

- The exemption-file change removes an entry (auth.rs JWT tenant claim
  fallback, fixed via serde schema default in round-7 verification repair) and
  lowers the frozen count 3→2 — the allowed direction; no new exemptions.
- Coverage table (observability.md row 5) honestly records 26/34 event types
  with the remaining 6 container + 2 file/network events attributed to other
  tasks; task-104's scope (10 source-control + access-control events) is
  fully delivered.
- `cargo test --all` (workspace) is owned by the verification stage per the
  standing probe-size constraint; gyre-server lib is the crate containing all
  task-104 code.
