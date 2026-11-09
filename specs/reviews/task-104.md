# Review — task-104 (Add access control and source control audit event types)

Spec: `observability.md` §Audit Event Types.
Candidate under review (round 9): `0fbf50527` (= round-8 repair head `47190ed9` + process commit), base `def0add9e`.
Commits: `ac5f8836`, `ca905fe4`, `8e6ed8ec`, `47190ed9` (+ process/merge commits).
Verdict: **needs-revision** (2 findings, same class as round-8 F1).

## Round 9

Test runs (focused, `/tmp/stage/bin/cargo`, evidence in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib uninstrumented_role_guards_record_rbac_denied_audit_event` → **1 passed** (round-8 regression test, all five repaired sites).
- `cargo test -p gyre-server --lib rbac_denied` → **3 passed** (extractor, handler guard, round-8 regression).
- Focused task-104 behavior tests, one per acceptance criterion → **11 passed, 0 failed**: `cef_severity_source_control_and_access_control_events`, `auth_success_audit_event_recorded_for_global_token`, `auth_success_audit_event_recorded_once_per_request`, `auth_failure_audit_event_recorded_for_invalid_token`, `auth_failure_audit_event_recorded_for_revoked_agent_jwt`, `abac_denial_records_abac_denied_audit_event`, `push_records_git_push_accepted_audit_event`, `push_declined_by_git_records_no_git_push_accepted_audit_event`, `push_rejected_by_gate_records_git_push_rejected_audit_event`, `push_spec_change_records_spec_changed_audit_event`, `clone_records_git_clone_audit_event`.
- `cargo test -p gyre-domain --lib audit` → **9 passed** (incl. `source_control_and_access_control_event_types_roundtrip`).
- Probe (temporary tests appended to `rbac.rs`, file restored and tree verified clean): see findings.

Verified working (no findings):

- **Round-8 repair is genuine.** All five F1 sites now call `rbac::record_rbac_denied` before the 403 with the spec'd detail fields; the regression test asserts exactly one `rbac_denied` event per site (`required_role`, `endpoint`, outcome `Blocked`) against the full `api_router`, including the reviewer's exact probe (Developer JWT → `GET /api/v1/meta-specs-registry` → 403 + one event).
- **Emission logic unchanged since the last reviewed candidate `90ee7e74`**: `git diff 90ee7e745..47190ed90 -- git_http.rs auth.rs siem.rs abac_middleware.rs` is 5 lines, all from the task-157 base merge (rollout-policy ABAC route mapping). All previously verified acceptance criteria (10/10 event types, CEF severities field-by-field, GitPushAccepted only for verified-applied ref updates, GitPushRejected on gate paths, GitClone on upload-pack, SpecChanged with task_id, AuthSuccess/AuthFailure with per-request dedupe, AbacDenied from middleware and repo-level push ABAC, impersonation variants deferred to task-108 per plan) hold on this candidate — confirmed by the 11 focused tests above, not by label.

Findings:

- [open] **F1 (moderate-major): `network.rs` `update_peer`/`delete_peer` Admin guards still reject unaudited — same defect class as round-8 F1, missed by the round-8 enumeration.** Non-JWT non-Admin callers (per-agent uuid token → `UserRole::Agent`, `jwt_claims: None`) updating/deleting another agent's peer get 403 ("Admin role required to … another agent's peer record") with **zero** `rbac_denied` events — only `auth_success` is recorded (ABAC allowed the request; the handler guard is the operative enforcement, so the rejection is invisible to the audit trail and SIEM forwarding). The sibling `register_peer` guard in the same file **was** instrumented by this task's own diff, making the file internally inconsistent. Probe (evidence: `task-104-r9-probe-residual-guards.log`): seeded uuid token + Admin-registered peer for `other-agent` → PUT/DELETE `…/network/peers/:id` → 403, `rbac_denied_count=0`. Contradicts the spec trigger "RbacDenied: Role check rejects request", coverage row 5's "all live per-handler role guards", and the shipped note's "network peers" claim. **Repair:** add `record_rbac_denied(&state, &auth, "Admin", "PUT/DELETE /api/v1/network/peers/:id")` at both sites (round-8 pattern), extend the regression test; or correct the coverage/shipped claims to scope them accurately.
- [open] **F2 (moderate): `meta_specs.rs` `put_rollout_policy` Admin guard (arrived via the task-157 base merge) rejects unaudited.** Developer JWT → `PUT /api/v1/workspaces/:id/meta-specs/rollout-policy` → 403 "only Admin role may update workspace rollout policy" with zero `rbac_denied` events (only `auth_success`). The guard is base code (task-157), but the candidate tree now carries it while coverage row 5 claims "all live per-handler role guards" and the shipped notes enumerate the meta-specs guards as covered. **Repair:** instrument the guard (round-8 pattern) or scope the claims accurately and assign the guard's instrumentation to task-157's scope explicitly.

Non-findings (checked, judged out of scope or acceptable):

- `messages.rs:146` broadcast guard ("Server origin or Admin role") — mixed origin/role disjunction, not a pure role guard; a Server-origin non-Admin passes. Borderline; noted for task-108/impersonation-era instrumentation pass, not a revision blocker here.
- `explorer_views.rs` `check_ownership`, `key_binding.rs` revoke, `messages.rs` inbox/ack guards — ownership checks with an Admin bypass; the rejection reason is ownership, not role. Out of the "pure role guard" class round 8 established.
- `cargo test --all` / full workspace suites / arch checks — owned by verification; the focused suites relevant to this task's diff are green.
- Round-7 verification repair (`8e6ed8ec`) confirmed emission-logic-neutral (diff inspection + focused tests).
