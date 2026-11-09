# Review — task-104 (Add access control and source control audit event types)

Spec: `observability.md` §Audit Event Types (4 source control + 6 access control events).
Candidate under review: `df52e2a62` (round-15 final review; implementation `ac5f88365` repaired through rounds 5–14, all 8 attributed commits verified ancestors of the candidate head).
Base: `fb1f82e31`.
Verdict: **approved** (round 15; findings from all prior rounds verified repaired).

## Evidence (all runs against the exact candidate tree; logs in /tmp/stage/review-evidence/task-104-final-review.log)

- `cargo test -p gyre-server --lib`: **1553 passed, 0 failed**.
- `cargo test -p gyre-server --test git_integration`: **17 passed, 0 failed**.
- `cargo test -p gyre-server --lib git_http::tests`: **54 passed** (5 task-104 audit tests + task-141 persona tests in one suite — round-14 union intact).
- `cargo test -p gyre-server --lib -- auth::tests siem::tests rbac::tests abac_middleware`: **83 passed**; `audit_event` filter: **31 passed**; `siem`: **11 passed**; `cargo test -p gyre-domain --lib audit`: **9 passed**.
- Mutation probes (source restored and verified byte-identical to candidate afterward):
  - removing the extractor-path `record_auth_failure` at the API-key tenant-binding rejection → `auth_failure_audit_event_recorded_for_api_key_user_without_tenant_binding` **FAILS**;
  - disabling the `emit_git_push_accepted_audit` loop → `push_records_git_push_accepted_audit_event` **FAILS**.
  Both regression tests prove the production behavior, not test inflation.
- `scripts/check-arch.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`: all exit 0.

## Verified against the acceptance criteria

- **All 10 event types** in `AuditEventType` with `as_str`/`from_str` + roundtrip test (`gyre-domain/src/audit.rs`).
- **GitPushAccepted** — emitted per rev-parse-*verified* applied ref update in post-receive with `repo_id`/`branch`/`commit_sha`/`agent_id`/`push_gate_results` (structured per-gate outcomes); `verify_applied_ref_updates` (git_http.rs:1643) guards against git-declined updates being audited as accepted, with `--end-of-options` option-injection protection and non-`refs/` refname rejection.
- **GitPushRejected** — all four gate-rejection paths (pre-accept gates, constraint-enforcement, manifest-coverage, spec-link-cycles) with `gate_name`/`reason`, outcome Blocked.
- **GitClone** — `git_upload_pack` after a successful upload-pack, with `repo_id`/`agent_id`.
- **SpecChanged** — spec-lifecycle hook at all three arms (dedup-skip, create-Err, create-Ok with task_id), workspace scope from the per-push repo-row resolution; unresolvable scope ⇒ `None`, never a fabricated "default" identity (task-097 F3 class respected).
- **AuthSuccess/AuthFailure** — extractor (all token kinds incl. user_jwt/federated_jwt superset), global `require_auth_middleware` (via `record_auth_failure_for_middleware`), WebSocket path, and the round-12 API-key tenant-binding rejection; failures carry `token_kind` + `rejection_reason`; request-extension marker dedupes AuthSuccess to one event per request (regression-tested).
- **AbacDenied** — middleware deny path (with `matched_policy` as `policy_id`, `resource`, `action`, `claims`), git push repo-level ABAC, and the nine round-11 per-handler G6 guards.
- **RbacDenied** — `RequireAgent` extractor, all swept per-handler pure role guards (rounds 8–10), and the MCP tools/call write-role gate, with `user_id`/`required_role`/`actual_role`/`endpoint`.
- **ImpersonationStarted/Ended** — variants + CEF severity 7 exist; emission deferred to task-108 per the task plan (impersonation unimplemented).
- **CEF severity mapping** — exact per the task table (AuthFailure 6; AbacDenied/RbacDenied/GitPushRejected 5; AuthSuccess/GitPushAccepted/GitClone 3; Impersonation 7; SpecChanged 4), asserted field-by-field.
- Shared `api::audit::emit_server_audit_event` persists through the AuditRepository port + SSE broadcast — the same store the query/stats/SIEM paths read.

## Sweep for uninstrumented denial paths (no findings)

- A function-level scan of every `FORBIDDEN`/`Forbidden(` production site in `api/` + `mcp.rs` for role-check conditions without a nearby `record_rbac_denied`/`record_abac_denied` yields only owner-or-admin and agent-identity containment guards (`check_ownership` in saved_views/explorer_views, key_binding revoke, messages poll/ack/list inbox containment, specs revoke_spec_approval) — the denial class the task explicitly excludes from RbacDenied (ownership, not role).
- The SCIM static-token gate (`check_scim_auth`) is not an uninstrumented auth rejection: SCIM routes sit behind `require_auth_middleware`, so invalid platform tokens are rejected there with an AuthFailure event; the handler-level check is a second-factor value mismatch on an already-authenticated request, outside the spec's token_kind enumeration.
- The round-14 merge (`bd81b48e4`) introduces no new auth/gate denial surfaces (explorer_ws.rs / notification.rs / persona push code add no FORBIDDEN paths); the `parse_spec_changes` change in the merge diff is the base's refactor into `parse_name_status` with watched-path filter semantics preserved as a wrapper, and both sides' tests are present and passing at the merge head.

## Notes for the record (non-blocking)

- Coverage table row 5 (`specs/coverage/system/observability.md`) accurately reports 26/34 event types implemented; the remaining 8 (6 container, 2 file/network eBPF) belong to other tasks.
- Full workspace suite, remote verification, and CI are owned by the verification stage.
