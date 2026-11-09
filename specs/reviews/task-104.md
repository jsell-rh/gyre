# Review — task-104 (Add access control and source control audit event types)

Spec: `observability.md` §Audit Event Types.
Candidate under review: `ba29036ef` (process commit recording round-9 repair) against base `60cf8fb647`. Product head: `0e487df7f` (round-9 repair) on merged base `71a02247c`.
Verdict: **needs-revision** (one finding, same class as rounds 8–9).

## Round 10 (2026-11-09)

### Probes run (evidence in `/tmp/stage/review-evidence/`)

- `task-104-round10-rbac-denied-tests.log` — `cargo test -p gyre-server --lib rbac_denied` (via `/tmp/stage/bin/cargo`): **4 passed, 0 failed** (extractor, handler guard, round-8, round-9 regression tests).
- `task-104-round10-git-event-tests.log` — git_http audit tests (push accepted with gate results, git-declined push records nothing, gate-rejected push, spec-change push, clone): **5 passed, 0 failed**.
- `task-104-round10-auth-abac-siem-tests.log` — AuthSuccess (global token + per-request dedupe), AuthFailure (invalid + revoked JWT), AbacDenied middleware denial, CEF severity field-by-field: **6 passed, 0 failed**.
- `task-104-round10-full-lib-suite.log` — `cargo test -p gyre-server --lib`: **1481 passed, 0 failed**.
- `task-104-round10-mutation-secrets.log` — mutation check: deleting the secrets `require_admin` `record_rbac_denied` emission makes `round9_uninstrumented_role_guards_record_rbac_denied_audit_event` FAIL (restored afterward; tree verified clean).
- `task-104-round10-mcp-gate-probe.log` — live probe of the MCP write-role gate (see F1): role rejection delivered, rbac_denied event count **0**.
- Mechanical checks on this tree: `check-fabricated-scope-defaults.sh` OK (exemption file *reduced* 4→2 — two legacy sites fixed, frozen count lowered, the sanctioned direction, not new entries), `check-arch.sh` OK, `check-inert-enforcement.sh` OK, `check-task-commit-attribution.sh` OK.

### Verified working (no findings)

- **All 10 event types** in `AuditEventType` with `as_str`/`from_str` (`gyre-domain/src/audit.rs:19-80`); roundtrip covered by tests.
- **CEF severity mapping** (`siem.rs` `format_cef`): AuthFailure 6, AbacDenied/RbacDenied/GitPushRejected 5, AuthSuccess/GitPushAccepted/GitClone 3, ImpersonationStarted/Ended 7, SpecChanged 4 — matches the spec table exactly, asserted field-by-field.
- **Source control emission points**: GitPushAccepted only for verified-applied ref updates (round-5 `verify_applied_ref_updates`; the git-declined non-fast-forward test asserts no accepted/rejected events), GitPushRejected on every gate path, GitClone after successful upload-pack, SpecChanged via the spec-lifecycle hook with task_id. All tests green on this head.
- **Access control emission points**: AuthSuccess/AuthFailure from the extractor + global middleware + WebSocket path with token_kind and rejection_reason, deduped once per request (test asserts exactly one); AbacDenied from the ABAC middleware deny path and repo-level push ABAC; RbacDenied from the `RequireAgent` extractor and all round-8/round-9 guard sites (network update_peer/delete_peer, meta-specs rollout-policy/preview/registry, secrets require_admin ×4, message broadcast, spec approve/reject, MR approve). The round-9 regression test drives the full `api_router` with real tokens and asserts exactly one rbac_denied event with `required_role`/`endpoint`/Blocked outcome against the real `AuditRepository` port; mutation-verified this round.
- **Hybrid owner-or-Admin guards** (messages inbox poll/ack, saved views, explorer views, key bindings, spec approval revocation, workspaces tenant-mismatch guards returning 404, ws.rs tenant filters) reject on ownership/tenancy, not role — correctly classified out of the RbacDenied class.
- **ImpersonationStarted/Ended**: variants + CEF 7 exist; emission deferred to task-108 per the task plan (impersonation is not implemented).
- Test harness quality: assertions query the real `AuditRepository` (not mirrored state), each sub-case uses a fresh state so event counts are exact.

### Findings

- [open] **F1 (moderate-major, same class as round-8 `a42d6a7254` F1 and round-9 `301748df18` F1/F2): the MCP `tools/call` write-role gate rejects with no rbac_denied audit event.** `crates/gyre-server/src/mcp.rs:3305` — `needs_write && !has_role_at_least(&auth.roles, UserRole::Agent)` returns `PERMISSION_DENIED` ("insufficient permissions: this tool requires Agent or higher role") for the 12 write tools; `mcp.rs` contains no audit instrumentation at all, so role-based denials on the MCP surface are invisible to the audit trail and SIEM forwarding (severity-5 RbacDenied events never fire). Live-probed: ReadOnly JWT → `POST /mcp` `tools/call` `gyre_create_task` → role rejection, rbac_denied count 0 (evidence: `task-104-round10-mcp-gate-probe.log`). This contradicts the spec trigger "RbacDenied: Role check rejects request" (observability.md §Audit Event Types) and the coverage row 5 claim "RbacDenied from the rbac extractor and all live pure per-handler role guards" (specs/coverage/system/observability.md) — the round-9 same-class sweep covered only `api/` handlers, but `mcp_handler` in `src/mcp.rs` is also a live per-handler role guard (AGENTS.md itself names the needs_write gate as a mechanically-checked RBAC invariant). The gate predates the task (d7940e857, present in base `60cf8fb647`), but per the round-9 precedent (put_rollout_policy, arrived via a base merge) a live uninstrumented role guard in the candidate tree makes the "all live" claim false regardless of origin. **Repair:** record rbac_denied at the gate via the shared `rbac::record_rbac_denied` helper before returning the error (endpoint e.g. `POST /mcp tools/call`), or amend the coverage row and shipped claim to scope the claim accurately (api/ handlers only; MCP gate explicitly deferred to a named owner).

### Non-findings (checked, judged acceptable)

- `scripts/fabricated-scope-defaults-exemptions.txt` changes in the candidate diff are a *reduction* (4→2: git_http ws_id fallback fixed in task-168's repair; auth.rs JWT tenant fallback fixed by this task's round-7 serde-schema-default repair) with `FROZEN_EXEMPTION_COUNT` lowered accordingly — the sanctioned direction; the check passes.
- `record_rbac_denied` reports `actual_role` as `auth.roles.first()` — for multi-role callers this may not be the highest role; cosmetic, detail fields all present per spec.
- WS/explorer_ws role references are tenant-delivery filters, ownership guards, or non-rejecting fallthroughs (SDK explorer path falls back to native LLM rather than rejecting) — out of class.

### Summary

The ten event types, their emission points, and the CEF mapping are real and well-tested; the round-8 and round-9 repairs are genuine (mutation-verified this round) and the full gyre-server lib suite is green at this head. But the recurring failure class of this task — live pure role guards rejecting without an rbac_denied event — still has one instance outside the `api/` directory the previous sweeps covered: the MCP tools/call write gate. The "all live per-handler role guards" coverage claim is therefore still false. Revision required (instrument the gate or scope the claim).

— Reviewer, 2026-11-09 (round 10)
