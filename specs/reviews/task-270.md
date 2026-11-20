# Review — task-270 (Agent-principal default-branch push protection + continuous PR hygiene)

Spec: `specs/development/agent-workflow.md` §PRs as the Collaboration Primitive ("No direct pushes to main"; "A PR hygiene agent (or background job) continuously monitors for stale PRs … nudge reviewers, rebase, escalate, or close with explanation"), reconciled with `specs/system/spec-lifecycle.md` §Automatic Task Creation / §What This Does NOT Do (human default-branch pushes must stay accepted).

Candidate under review: `88077483be7f0afcb9c0efa340b4d9978a19ae3e` (branch `pipeline/task-270/91c1240c7a114b8fa0a4cdc2f8e50aa2-1`, base `00669baa`; product commits `c2690229`, `d5888ad5`, `c9faf6a8`, `041bddcd`, `34f7878e`, `f48a62f6`, `1b0cbd6c`; head commit is process/docs only).
Verdict: **approved** — no findings. Full diff read; independent probes run; server test execution deferred to the verifier per the 4 GiB budget policy (list below).

## Probes run (evidence: /tmp/stage/review-evidence/, see REVIEW-SUMMARY.md)

- `cargo check -p gyre-server` → exit 0 (7m43s).
- `cargo check -p gyre-server --tests` → exit 0 (3m55s — every new test compiles).
- `cargo test -p gyre-common` → 129 passed (NotificationType additions: string round-trip, priority bands, `all()` consistency).
- `cd web && npm ci && npm test` → 127 files / 2566 passed (includes the UserProfile test that extracts the `NotificationType` variants from the gyre-common Rust source and asserts `NOTIF_TYPE_IDS` matches and every toggle renders with a locale label — not mirrored logic).
- Mechanical invariants: `check-arch.sh`, `check-dead-message-kinds.sh` (the new `PushRejected` kind has an emitter), `check-abac-route-registry.sh` (no new routes, no exemption growth), `check-in-memory-state-stores.sh`, `check-mem-port-contracts.sh` → all exit 0.
- `python3 scripts/check-rustfmt-diff.py 00669baa` → changed lines clean (9 Rust files).
- `1b0cbd6c` verified formatting-only (single-line assert collapse in a test helper); head `88077483` touches only `specs/tasks/task-270.md`.

## Verified working

**Agent-principal default-branch push protection** (`git_http.rs:841`):

- Direct, unconditional check in the push handler (not an opt-in pre-accept gate), placed after repo resolution / mirror / archive / Write authorization / repo ABAC and BEFORE `run_git_stateless("receive-pack", …)` — a rejected push never updates the ref server-side (tests assert `git rev-parse --verify` absence in the real store).
- Caller-class discrimination is exactly the task contract: `auth.user_id.is_none() && !auth.is_system_principal()`. Verified against the auth extractor: the agent-token path mints `user_id: None` (auth.rs:1689), API-key and user-JWT paths set `user_id: Some(...)` (auth.rs:1865), and `is_system_principal()` (auth.rs:1266) requires `agent_id == "system" && user_id.is_none()` — not a spoofable bare string match.
- Target ref resolved from the Repository record's `default_branch` (never a hardcoded "main"); the trunk-repo test asserts both directions (protected on `refs/heads/trunk`, a literal `main` branch accepted).
- Deletion bypass closed: the guard matches the RAW pkt-line ref-update commands (`parse_ref_update_commands`, including zero-SHA deletions that `parse_ref_updates` filters out); covered by `agent_deletion_of_default_branch_rejected` with a real delete command body.
- Real-git-client robustness: the parser strips everything after the NUL capability separator (pre-existing, unchanged from base, shared with the production spec-lifecycle path that demonstrably works with real `git` clients in e2e), so capability-carrying first command lines still match the protected refname. Multi-ref pushes containing a default-branch update are rejected atomically (correct conservative behavior).
- Rejection is observable: 403 with an explicit reason directing to the MR flow (`gyre_agent_complete`), a `PushRejected` message-bus event, and a `GitPushRejected` audit event (gate_name `default-branch-protection`, outcome Blocked).

**Human/system path preserved (spec-lifecycle reconciliation):**

- `human_push_to_default_branch_accepted_and_spec_lifecycle_runs` drives the full path with a real API-key user + Developer membership: 200, ref lands, and the `spec_changed` audit event carries an auto-created `task_id` — exactly the §Automatic Task Creation contract. System-token pushes to `refs/heads/main` remain covered by pre-existing in-file tests that assert receive-pack actually moved the ref.
- e2e suites (`git_integration.rs`, `e2e_ralph_loop.rs`) updated so only initial default-branch pushes switched to the system token; agents keep pushing feature branches with their own tokens — authentication now consistent with the intended caller class.

**Server-side landing unaffected:** the merge processor lands via `git_ops.merge_branches` (merge_processor.rs:1476/2666), never through the HTTP receive-pack handler; the e2e `merge_queue_auto_merges_mr_and_commit_on_main` test covers merge-queue landing on the default branch end-to-end.

**Continuous PR hygiene job** (`pr_hygiene.rs`, 1675 lines):

- Registered in `jobs.rs` with `interval_secs` (default 300 s, `GYRE_PR_HYGIENE_INTERVAL_SECS`) and `run_at_utc_hour: None`, and spawned in `main.rs` via `spawn_job` (interval branch) — minutes-scale continuous cadence, not the daily `abandoned_branch_check`; the daily job is untouched. Registration test asserts interval-not-daily and that a manual trigger runs a real evaluation.
- All four stale classes act on real persisted state: `MergeRequest.updated_at`, reviewer assignments, review records (`list_reviews`), `has_conflicts` plus a real merge-base conflict check through the production `GitOpsPort::can_merge` (an in-memory `git merge` via git2, spawn_blocking — not a structural check). Thresholds configurable via `GYRE_PR_HYGIENE_{NUDGE,ESCALATE,CLOSE}_SECS` with sane non-zero defaults (2d/7d/30d).
- Unreviewed → reviewer nudge (user reviewers via deduplicated notification rows; no-reviewer MRs escalate to Developers/Admins, Viewers excluded); conflicted → real jj rebase attempt in the author agent's live worktree (same safeguards + shared 60 s KV backoff window as the merge processor's §4 rebase) or a `MrNeedsRebase` escalation when not applicable — every path records something; long-open → higher-priority (rank 3) `MrStaleEscalation` to Admins/Owners; abandoned → close with persisted `Open|Approved → Closed` transition (valid per the domain state machine), trace deletion mirroring the REST close endpoint, an explanatory MR comment, author delivery (agent bus message / user notification), Admin notification, and a `pr_hygiene` audit event — never a silent close.
- Idempotence is real and tested across consecutive evaluations: user notifications dedupe through `has_active_notification` (implemented in the mem, SQLite, and PostgreSQL adapters — verified), agent-reviewer bus nudges dedupe through a persisted KV backoff keyed (MR, agent), and closed MRs leave the open set.
- Observability surfaces are complete: five new `NotificationType` variants wired through `as_str`/`from_str`/`all()`/priority ranks, email headings (en/de), `NOTIF_TYPE_IDS`, and `en.json` labels — the frontend consistency test passes.

**Test quality (no inflation found):** the push tests assert real git-store ref state (`rev-parse --verify`) with packfile-carrying receive-pack bodies (the earlier missing-packfile defect was repaired in `f48a62f6` — bodies now actually land refs); hygiene tests assert exact notification counts, persisted status, jj call arguments, audit events, and no-duplicate behavior on second runs — each fails if its production check is deleted. The fresh-MR and reviewed-MR tests pin the negative space.

## Minor (non-blocking, recorded for completeness)

- The hygiene auto-close does not re-index the MR search document (the REST close endpoint calls `search_indexer::index`), so search shows a stale status until the next re-index trigger. Outside the task contract (status transition + explanation delivery), no spec text requires it.
- `is_conflicted` performs an in-memory merge per open MR per evaluation (default every 5 min) — bounded and off the async executor thread; acceptable.

## Verifier must run (server test compilation exceeds this sandbox's budget)

- `cargo test -p gyre-server agent_push_to_default_branch_rejected_directing_to_mr_flow agent_push_to_feature_branch_succeeds agent_deletion_of_default_branch_rejected protection_resolves_real_default_branch_not_literal_main human_push_to_default_branch_accepted_and_spec_lifecycle_runs`
- `cargo test -p gyre-server pr_hygiene` (full module suite, incl. both consecutive-evaluation idempotence tests)
- `cargo test -p gyre-server pr_hygiene_check_registered_on_interval_not_daily`
- `cargo test -p gyre-server --test git_integration --test e2e_ralph_loop`
- Standard full workspace suite + Clippy.
