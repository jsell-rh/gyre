# Review — task-104 (Observability §Audit Event Types — access control & source control events)

Spec: `specs/system/observability.md` §Audit Event Types (source control: GitPushAccepted, GitPushRejected, GitClone, SpecChanged; access control: AuthSuccess, AuthFailure, AbacDenied, RbacDenied, ImpersonationStarted, ImpersonationEnded).
Candidate under review: `4d202bb624736c78310f347df99c0da6c8266b2f` (head of the assigned branch; task commits `ac5f8836`, `ca905fe4`, `8e6ed8ec`, `47190ed9`, `0e487df7`, `48edaaf9`, `07794b48`, `2d4faab3`, `738e4fed` + merge/process lineage from base `402142b1c`).
Verdict: **approved** (round 16).

## What was verified independently

All ten event types exist in `AuditEventType` (`gyre-domain/src/audit.rs`) with `as_str`/`from_str` round-trips and CEF severities exactly per spec: GitPushAccepted 3, GitPushRejected 5, GitClone 3, SpecChanged 4, AuthSuccess 3, AuthFailure 6, AbacDenied 5, RbacDenied 5, ImpersonationStarted/Ended 7 (`siem.rs format_cef`).

Emission points, each carrying the spec'd detail fields and persisting through the shared `api::audit::emit_server_audit_event` (AuditRepository port `state.audit.record` + SSE broadcast — real storage, not log-only):

- **GitPushAccepted** — `git_http.rs` post-receive, one event per ref update that `verify_applied_ref_updates` confirms via `git rev-parse` actually landed (git-declined updates emit nothing), with branch, commit_sha, agent_id, and structured per-gate `push_gate_results`.
- **GitPushRejected** — all four push-gate rejection paths, with gate_name/reason, outcome Blocked.
- **GitClone** — `git_upload_pack` after a served smart-HTTP clone/fetch.
- **SpecChanged** — `process_spec_lifecycle`, all three arms (dedup-skip, create-Err, create-Ok), scoped from the once-per-push `ws_id` resolution; never a fabricated scope.
- **AuthSuccess / AuthFailure** — extractor paths for global/agent_jwt/uuid_token/api_key/user_jwt, deduped exactly once per request via the `AuthAuditMark` request extension; failures carry `rejection_reason` on invalid, expired, revoked, and (round 12) missing-tenant-binding paths, including the pre-extractor global middleware path (`lib.rs` → `record_auth_failure_for_middleware`). Forensic source_ip comes from `ConnectInfo`, not forwarded headers.
- **AbacDenied** — middleware deny path plus the shared `record_abac_denied` helper at the git-push repo-level check and the nine round-11 per-handler guards, with claims attached.
- **RbacDenied** — `RequireAgent` extractor, the per-handler role-guard sweep (rounds 8–9), and the MCP tools/call write-role gate (round 10), with required/actual role and endpoint.
- **ImpersonationStarted/Ended** — variants + severity 7 only; emission deferred to task-108 exactly as the task plan and acceptance criteria specify.

## Probes (logs in /tmp/stage/review-evidence, task-104-r16-*)

- `cargo test -p gyre-server --lib` → **1591 passed, 0 failed** at the candidate head.
- Focused subsets: `audit` filter 50/50, `git_http::tests` 54/54, `siem` 11/11, `auth::` 51/51, `abac_middleware::tests` 14/14; `--test git_integration` 17/17; `cargo test -p gyre-domain` 451 passed across targets.
- `cargo fmt --all --check` clean.

**Mutation verification** (temporary edits, restored byte-identically via `git checkout --`; tree confirmed clean at `4d202bb62` afterwards):

1. GitClone emission disabled → `clone_records_git_clone_audit_event` FAILS.
2. GitPushAccepted loop neutered → `push_records_git_push_accepted_audit_event` FAILS.
3. `record_auth_success` early-returns → both `auth_success_audit_event_recorded_*` tests FAIL.
4. `record_abac_denied` early-returns → both `abac_denial_records_abac_denied_audit_event` and `uninstrumented_abac_guards_record_abac_denied_audit_event` FAIL.

Every asserted emission is load-bearing: the tests fail when the production behavior is removed.

## Notes (non-blocking)

- `GitClone` fires on any served upload-pack (clone and fetch are indistinguishable server-side) and after a successful git invocation even if the client-side negotiation reported an error in the pkt-line stream — the code documents the clone/fetch reading, and the spec's trigger is adequately served.
- `scripts/fabricated-scope-defaults-exemptions.txt` diff vs base is comment-only with the entry count lowered, not grown.
- Full workspace suites, workspace-wide clippy, arch checks, and commit-attribution mechanics are owned by the verification stage.

## Verdict

Approved. All twelve acceptance criteria are satisfied by production code verified with independent, mutation-checked evidence.
