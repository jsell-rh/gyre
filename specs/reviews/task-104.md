# Review — task-104 (Add access control and source control audit event types)

Spec: `observability.md` §Audit Event Types (4 source control + 6 access control event types, detail fields, CEF severity mapping).
Candidate under review: `90ee7e745` (merge base `c5deca801a`); commits `ac5f88365`, `ca905fe43`, `8e6ed8ecc` plus process/merge commits.
Verdict: **needs-revision** (one finding).

## Probe evidence (this round, on candidate `90ee7e745`)

- `cargo test -p gyre-domain --lib audit` → **9/9 ok** (incl. `source_control_and_access_control_event_types_roundtrip`).
- `cargo test -p gyre-server --lib -- audit_event` → **20 passed, 0 failed** (real smart-HTTP pushes with configured gates, a git-declined non-fast-forward push, a real `git clone` over a local listener, spec-file push, invalid/revoked token, per-request AuthSuccess dedupe, ABAC denial, both RbacDenied paths).
- `cargo test -p gyre-server --lib cef_severity_source_control` → **1/1 ok**; `jwt_missing_tenant_id_defaults_to_default` → **1/1 ok**.
- `cargo test -p gyre-server --lib` (full suite) → **1443 passed, 0 failed**.
- Mechanical checks on the candidate: `check-arch.sh`, `check-inert-enforcement.sh`, `check-scope-literal-defaults.sh`, `check-dead-message-kinds.sh`, `check-lossy-secret-conversion.sh`, `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh` → all exit 0 (exemption freeze honestly lowered 3→2 with the entry deleted, matching the task-103 precedent fix).
- Mutation probes (source restored and re-verified clean after each):
  - `verify_applied_ref_updates` forced to pre-fix `is_applied = true` → `push_declined_by_git_records_no_git_push_accepted_audit_event` FAILS (test is real).
  - AuthSuccess dedupe marker disabled → `auth_success_audit_event_recorded_once_per_request` FAILS (test is real).
- Gap probe (temporary test, restored): Developer JWT → `GET /api/v1/meta-specs-registry` returns 403 via the handler's Admin role guard with **zero** `rbac_denied` audit events recorded.

## Verified working (no findings)

- **All 10 event types** in `AuditEventType` with as_str/from_str roundtrip; ImpersonationStarted/Ended exist with CEF severity 7, emission deferred to task-108 per task plan.
- **CEF severity mapping** matches spec field-by-field (AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, Impersonation 7, SpecChanged 4).
- **GitPushAccepted** — per *verified-applied* ref update (round-5 repair holds; regression test mutation-verified this round), carrying repo_id/branch/commit_sha/agent_id and structured per-gate `push_gate_results` from `check_pre_accept_gates`.
- **GitPushRejected** — at every push-gate rejection path (pre-accept gates, constraints, manifest, cycles) with gate_name/reason, outcome Blocked; no accepted event for rejected pushes; refs rolled back.
- **GitClone** — emitted by `git_upload_pack` after upload-pack succeeds (not on failure), tenant-scoped via `resolve_repo_by_slug`.
- **SpecChanged** — spec-lifecycle hook on default-branch applied updates with spec_path/change_kind/task_id; workspace scope resolved by lookup and skipped (not fabricated) on lookup failure.
- **AuthSuccess/AuthFailure** — extractor, global middleware, and WS path; token_kind distinguishes global/agent_jwt/uuid_token/api_key/user_jwt/federated_jwt; rejection_reason distinguishes invalid/expired/revoked on the extractor path; per-request dedupe via request-extension marker (mutation-verified).
- **AbacDenied** — ABAC middleware deny path (policy_id/resource/action/claims) and the repo-level push ABAC check.
- **Round-7 verification repair** — the JWT tenant-claim "absent = default" contract moved to a serde schema default (`#[serde(default = "default_tenant_claim")]`), behavior test-pinned by `jwt_missing_tenant_id_defaults_to_default`; exemption entry removed and FROZEN_EXEMPTION_COUNT lowered 3→2 (correct direction; no gaming). Remaining `unwrap_or("default")` sites in auth.rs are JWK `kid` values, not scope identities.

## Findings

- [ ] **F1 (moderate-major, code): `RbacDenied` is not emitted on all live role-check rejections — five pure role guards reject with 403 while recording no audit event, contradicting both the spec trigger ("Role check rejects request") and the shipped/coverage claim "every live per-handler role guard".**
  1. `crates/gyre-server/src/api/meta_specs.rs:378` — `POST /api/v1/workspaces/:id/meta-specs/preview` requires Admin or Developer; rejection has no `record_rbac_denied`.
  2. `crates/gyre-server/src/api/meta_specs.rs:1033` — `GET /api/v1/meta-specs-registry` requires Admin (ABAC-exempt route: the handler is the only authorization surface); no emission. **Empirically confirmed**: Developer JWT → 403, zero `rbac_denied` events (probe log: `/tmp/stage/review-evidence/probe-uninstrumented-guards.log`).
  3. `crates/gyre-server/src/api/specs.rs:445` — `POST /api/v1/specs/:path/approve` rejects ReadOnly callers; no emission.
  4. `crates/gyre-server/src/api/specs.rs:1032` — `POST /api/v1/specs/:path/reject` requires Admin or Developer; no emission.
  5. `crates/gyre-server/src/api/merge_requests.rs:855` — MR status→Approved rejects the Agent role (the code comment itself says "Role-based check"); no emission.

  Ownership/membership guards that merely use an Admin bypass (workspaces tenant-scoping 404s, saved_views/explorer_views/key_binding/messages ownership, network peer ownership) are correctly *not* counted — those are not role rejections. The five sites above are pure role rejections. Fix: add `record_rbac_denied` calls at the five sites (for the merge_requests site, `required_role` semantics need a value such as "non-Agent"/"human"), or amend the task contract/coverage note to scope the claim accurately. Everything else in this task is verified working, including both mutation probes.
