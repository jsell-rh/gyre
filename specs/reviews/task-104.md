# Review — task-104 (Add access control and source control audit event types)

Spec: `specs/system/observability.md` §Audit Event Types (source control events + access control events tables).
Candidate: `1345a7cac` (code head `48edaaf9a`, plus process commits; base `4e895f766`). Round 11 of review.
Verdict: **needs-revision** (1 finding).

## What was verified working

- All 10 event types present in `AuditEventType` with correct `as_str`/`from_str` round-trips (`gyre-domain/src/audit.rs`), including the task-103 agent-lifecycle union after the base merge.
- Emission points are real and land in the shared `AuditRepository` store via `emit_server_audit_event`:
  - `GitPushAccepted` per verified-applied ref update (`verify_applied_ref_updates` rev-parse check — git-declined updates emit nothing), with structured `push_gate_results`.
  - `GitPushRejected` at every push-gate rejection path (pre-accept, constraint, manifest-coverage, spec-link cycle).
  - `GitClone` after successful `git-upload-pack`.
  - `SpecChanged` from the spec-lifecycle hook (added/modified/deleted/renamed; task_id null when deduped).
  - `AuthSuccess`/`AuthFailure` in the extractor, the global `require_auth_middleware`, and the WebSocket path, with per-request AuthSuccess dedupe (`AuthAuditMark` extension).
  - `AbacDenied` from the ABAC middleware deny path and the git-push repo-level G6 check.
  - `RbacDenied` from the `RequireAgent` extractor, the per-handler guards swept in rounds 8–9, and the MCP tools/call write-role gate (round 10).
- CEF severity mapping matches the spec table exactly (field-by-field test).
- Source IPs for auth events come from `ConnectInfo`, not forwarded headers (CWE-348 respected).
- Full suite at the candidate head: `cargo test -p gyre-server --lib` → **1492 passed, 0 failed** (log: `/tmp/stage/review-evidence/task-104-round11-full-lib-suite.log`), matching the shipped claim.
- Remaining uninstrumented role-check sites in `api/` (key_binding, explorer_views, saved_views, workspaces) are owner-or-tenant-scope hybrid guards that reject on ownership/scope, not role — correctly not RbacDenied emissions, consistent with the classification held since round 8.

## Round 11 finding

- [ ] **F1 (major): repo-level ABAC denials on `/api/v1` handler surfaces record no `abac_denied` audit event.** `crate::abac::check_repo_abac` — the G6 kv-store repo policies, the *same* policy layer the task instrumented on the git push path (`git_http.rs` G6 block calls `record_abac_denied`) — is enforced per-handler in `agent_tracking.rs:160` (POST `/api/v1/repos/:id/worktrees`), `jj.rs:103/124/182/201`, `orchestrator.rs:311` (repo orchestrator spawn), `graph.rs:1331`, `spawn.rs:348`, and `conversations.rs:46` (`check_workspace_abac_for_read`). When these deny, the caller gets 403 but no `abac_denied` event reaches the audit trail or SIEM forwarding, contradicting the spec trigger "ABAC policy rejects request" and the acceptance criterion "AbacDenied emitted on ABAC policy denial".

  **Live reproduction** (probe source `/tmp/stage/review-evidence/task-104-round11-probe-source.rs`, log `task-104-round11-repo-abac-denial-no-audit-event.log`): full `build_router`, `seed_builtin_policies` as production `main.rs` does, repo record `repo-1`, G6 policy `[{"resource_type":"repo","required_claims":{"team":"infra"}}]` in the `abac_policies` kv namespace, Developer JWT (route-policy middleware layer allows Developer write on repo resources) → `POST /api/v1/repos/repo-1/worktrees` → **403 `{"error":"ABAC denied: no policy satisfied for repo repo-1"}`** with **`abac_denied` audit-event count = 0**. The request provably passes the ABAC middleware (a 422 body-validation error was observed first when the body was malformed, i.e. the middleware layer allowed it through), so the denial comes solely from the uninstrumented handler-level policy check.

  This is the ABAC-side equivalent of the round-8/9/10 per-handler RBAC-guard sweep; the shipped notes themselves set the standard ("AbacDenied — emitted ... and from the repo-level ABAC check that guards git push"). Remedy: `record_abac_denied` (or an equivalent carrying agent_id, policy_id where known, resource, action, claims) at each `check_repo_abac` / `check_workspace_abac_for_read` deny site, with a regression test in the round-8/9/10 pattern.

## Notes (not findings)

- The global middleware's best-effort `token_kind` classification labels any rejected `ey`-prefixed token `agent_jwt`; a rejected user/federated JWT is misclassified. Cosmetic — the event and rejection reason are still recorded.
- A request with no `Authorization` header returns 401 without an `AuthFailure` event; the spec trigger is "Invalid or revoked token", and no token was presented. Defensible reading, consistent with prior rounds.

## Evidence

- `/tmp/stage/review-evidence/task-104-round11-repo-abac-denial-no-audit-event.log` — probe run (FAILED on the missing-event assertion; the failure *is* the finding).
- `/tmp/stage/review-evidence/task-104-round11-probe-source.rs` — temporary probe test (removed from the tree after the run; working tree restored to the candidate, `git status` clean).
- `/tmp/stage/review-evidence/task-104-round11-full-lib-suite.log` — 1492 passed, 0 failed.
