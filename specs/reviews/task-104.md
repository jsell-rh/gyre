# Review — task-104 (Observability §Audit Event Types: source control + access control events)

Spec: `specs/system/observability.md` §Audit Event Types. Candidate: `bad0e3114` (task commit `ac5f88365`, merge head `4c6319db3` over base `72ad2a128`). Verdict: **needs-revision** (one finding).

## What was verified working (no findings)

- **All 10 variants** present in `AuditEventType` with `as_str`/`from_str` round-trips; `cargo test -p gyre-domain --lib audit` → 9/9 ok, including `source_control_and_access_control_event_types_roundtrip`.
- **Emission points are real and land in the audit port store** (via `api::audit::emit_server_audit_event` → `state.audit.record` + SSE broadcast):
  - `GitPushAccepted` per accepted ref update in `git_receive_pack` post-receive, carrying `push_gate_results` from the restructured `check_pre_accept_gates` (`PushGateOutcomeEntry`).
  - `GitPushRejected` at all four gate rejection paths (pre-accept gates, constraint enforcement, manifest coverage, spec-link cycles), outcome `Blocked`, with rollback.
  - `GitClone` in `git_upload_pack`; `SpecChanged` in the spec-lifecycle hook with `task_id`.
  - `AuthSuccess`/`AuthFailure` in the extractor + global middleware + WS path, with `token_kind`/`rejection_reason`; the `AuthAuditMark` request-extension dedupe holds (test asserts exactly one `auth_success` per request through the full router).
  - `AbacDenied` from the ABAC middleware deny path (with `policy_id`/`resource`/`action`/`claims`) and the repo-level push ABAC check; `RbacDenied` from the `RequireAgent` extractor and the per-handler role guards.
  - `ImpersonationStarted`/`Ended` variants + CEF severity 7 only, per the task plan (emission deferred to task-108).
- **CEF severity mapping** matches the spec table exactly; asserted field-by-field in `cef_severity_source_control_and_access_control_events` (ok).
- **Tests are real behavior tests**, not stub-backed: smart-HTTP pushes with real git packfiles against a real listener, real `git clone` over TCP, real token minting/revocation, real policy denial. Focused run at the candidate: `cargo test -p gyre-server --lib audit_event` → 19/19 ok (includes all 12 task-104 tests), `cef_severity` → ok (logs in `/tmp/stage/review-evidence/`).
- **Exemption handling is tightening, not grandfathering**: `FROZEN_EXEMPTION_COUNT` 4→3 removes the off-by-one slack the base left (base had 3 entries frozen at 4); the `auth.rs:773→1041` re-point is the same `validate_tenant_id`-guarded site shifted by merge-local line changes; `check-fabricated-scope-defaults.sh` passes with zero slack.

## Finding (needs revision)

**F1 (major): `GitPushAccepted` fires for pushes git rejected — the audit trail records a false `Success` event with a SHA that never landed.** The post-receive block emits `emit_git_push_accepted_audit` unconditionally for every *requested* ref update whenever the `git receive-pack` process exits 0; it never verifies the ref update was applied (no report-status parsing, no rev-parse confirmation). The spec trigger is "Smart HTTP push accepted".

Reproduced with a temporary probe test (restored afterward; tree verified clean at `bad0e3114`; evidence `/tmp/stage/review-evidence/task-104-nonff-probe.log` + `task-104-probe-source.rs`): server ref at `tip`, client pushes stale old-sha (`base` → divergent commit). Git rejects the update — `git rev-parse refs/heads/main` still returns `tip`, and a real client negotiating report-status would receive the `ng` report — yet the audit store records `git_push_accepted {branch: "main", commit_sha: <divergent>, outcome: Success}`. This is the everyday stale-client/concurrent-push case; with `receive.denyNonFastForwards` configured, every rejected non-FF push is logged as accepted. The false event also flows to the SIEM forwarder (severity 3) and audit stats. Remedy: confirm each ref update applied (e.g. `git rev-parse <refname> == new_sha`, or parse the report-status) before emitting; skip (or route to the rejection path) updates git declined.

Everything else in the task contract is genuinely implemented and independently exercised; with F1 fixed this task should be approvable.
