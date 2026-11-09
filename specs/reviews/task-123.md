# Review — task-123 (Implement notification entity with full type taxonomy and priority model)

Spec: `user-management.md` §Notification Entity.
Candidate: `393543670c5a69d625f7af636ad0c80c9890530a` (branch `pipeline/task-123/0b69938bd38d497e92cfbd2aff696e1c-1`), diffed against base `fefde2946f3a0af1f212588f54591516fee0f7b2`.
Product commits under review: `27bfb2a69`, `deec7953a0`, `f92a48916` (+ merge `730073ad1` carrying the rebase onto the base); the remaining branch commits are process/attribution only (attribution check passes).
Verdict: **approved** (round 1).

## Probes run (evidence in /tmp/stage/review-evidence/)

All runs on the exact candidate tree; working tree restored clean afterwards (`git status --porcelain` → 0 entries; the `web/dist` churn seen mid-review was build.rs output from my own cargo invocations, reverted).

- `cargo test -p gyre-common notification` → **8 passed, 0 failed** (18-variant taxonomy round-trip, legacy-name parse, band mapping, rank bands, HSI table).
- `cargo test -p gyre-adapters --lib sqlite::notification` → **4 passed, 0 failed** (create persists source/action_url/read against the real migration 000059 — `SqliteStorage` runs pending migrations on construction, so the migration executed; mark_read stamps/clears and is user-scoped; has_active_notification type/entity/since matching + resolve removes from active set; retention read/unread split incl. marked-read).
- `cargo test -p gyre-server --lib notification` → **32 passed, 0 failed**; plus `create_pending_persona_requests_approval_from_workspace_admins`, `pause_merge_queue_notifies_admins_and_owners`, `stale_transition_notifies_workspace_members_once` → all pass.
- Mutation probes (temporary, restored immediately; both killed by the tests, proving they are not self-confirming):
  - Disabled `maybe_notify_budget_threshold` (early return) → `budget_threshold_notifications_fire_once_per_period` **FAILED** (empty recipient list).
  - Disabled `deep_link` (return None) → 5 notifications tests **FAILED** (mr_merged/gate_failure/gate_passed/agent deep links + workspace-home fallback).
- `cargo check -p gyre-cli` → clean (CLI reads notifications as JSON via the API, which still exposes `entity_ref`; no struct-field breakage).
- Mechanical checks (19 scripts: arch, migration versions + SQL portability, ABAC route registry, ABAC exempt handlers, MCP write tools, dead message kinds, mem port contracts, fabricated-scope/scope-literal/inert-enforcement/lossy-secret/relative-path/byte-slice/forwarded-header/in-memory-state/unbounded-HTTP/fail-open-ref/forged-scope, task-commit attribution) → **all exit 0**.
- Frontend mapping audit (script `check-inbox-mapping.py`): every one of the 18 user-management taxonomy types is present in both `Inbox.svelte` maps (`typeNormMap`, `TYPE_VARIANTS`), and every newly added PascalCase key corresponds to a real `NotificationType::as_str()` value. The only non-matching keys are pre-existing base entries or deliberate legacy-row support (`SpecPendingApproval`, `BudgetWarning` — both marked `// legacy rows`), which `NotificationType::parse` accepts.

## Verified against the task contract

- **Entity fields: PASS.** `Notification` (gyre-common/src/notification.rs) carries id, user_id, notification_type, title, body, source, priority, action_url, read, read_at, created_at (plus the HSI §2/§8 inbox fields the existing contracts require: workspace_id, priority_rank, resolved_at, dismissed_at, tenant_id). Spec deltas (i64 timestamps, Option<String> body) follow the pre-existing repo-wide wire convention and are documented in the coverage row.
- **18 variants: PASS.** All defined, round-trip through `as_str`/`parse` (tested); unified with the 17 HSI inbox types (35 total). Renames (`SpecPendingApproval`→`SpecApprovalRequested`, `BudgetWarning`→`AgentBudgetWarning`) keep parsing legacy DB strings, with a supersession note added to `specs/system/human-system-interface.md` §2 — spec amendment rather than contradiction.
- **NotificationPriority: PASS.** Low/Medium/High/Urgent; `default_priority()` is always the band of the HSI 1–10 rank, so the two models cannot disagree; band mapping matches the spec's explicit level examples (tested).
- **NotificationSource: PASS.** `{entity_type, entity_id, workspace_id, repo_id}` replaces the flat `entity_ref`/`repo_id`; every creation site in the repo migrated (compile proves no stragglers); pre-migration rows map NULL entity_type to the generic "system" kind rather than fabricating one (documented in the migration).
- **Migration 000059: PASS.** Next unused version (base max was 000058); portable SQL only (portability script + real SQLite execution in adapter tests); read state backfilled from resolved/dismissed timestamps; down.sql provided.
- **Default priority mapping: PASS** (see above).
- **Action URL: PASS.** `notify_rich` derives deep links through workspace slug + repo name to the SPA entity routes; no link fabricated when the workspace is unresolvable (tested negative case); sources without a repo-scoped page fall back to the workspace home.
- **Creation paths updated: PASS.** Mis-typed stand-ins replaced (`SuggestedSpecLink`→`MrMerged`, `AgentEscalation`→`AgentFailed` on the failure path, `CrossWorkspaceSpecChange`→`BreakingChangeDetected`, `GateFailure`→`SpecDriftDetected` for patrol findings, `TrustSuggestion`→`InvitationReceived`). New emitters are real, routed, deduped, and tested: PersonaApprovalRequested (owner or Admins/Owners), AgentBudgetWarning/Exhausted (≥80%/≥100% threshold crossing, once per period, spawner + Admins/Owner), GatePassed (all-gates-green, deduped per user+MR), MergeQueuePaused (Admins/Owners), DependencyStale (stale transition only), MembershipChanged. Recipient routing verified by recipient-list assertions in tests.
- **Read state end-to-end: PASS.** `mark_read` port method implemented in SQLite, PostgreSQL, and mem adapters (user-scoped, stamps/clears read_at); `POST /api/v1/notifications/:id/read|/unread` registered, ABAC-exempt with per-handler auth identical to dismiss/resolve (user-scoped get + tenant check; cross-user 404 with no mutation tested); retention counts marked-read as read.
- **API/docs/UI: PASS.** List response adds priority_level/entity_type/action_url/read/read_at while keeping the numeric rank and entity_ref/repo_id the web Inbox and CLI read today; api-reference.md updated (including fixing a stale doc row for a never-existent PUT endpoint); Inbox.svelte + en.json map all renamed/new types with legacy names retained for pre-migration rows.
- **`cargo test --all`:** focused suites green everywhere probed (common/adapters/server new + notification paths); full workspace suite left to verification per review-scope rules. No observed cross-crate breakage (cli checked clean; domain/ports/adapters compiled as dependents of the server test run).

## Non-findings (checked, judged out of scope or acceptable)

- **SecurityFinding / SystemAlert have no emitting feature.** Defined with priority mapping, honestly recorded as no-emitter in the coverage row rather than wired to a fake event. The §Notification Entity spec defines the entity model, not emitters; the task's acceptance criteria do not require emitters for these two; the repo's dead-emitter invariant covers `MessageKind`, not `NotificationType`.
- **Pre-existing `"default"` tenant literal** on `notify_mr_reverted` and the `fail_agent` path (the latter's call was rewritten by this task but preserved the base's literal). Legacy debt predating the branch; every *newly added* emitter resolves the tenant from the workspace record and skips on unresolvable scope. tenant_id is an HSI field, not part of §Notification Entity — out of this task's contract; candidate for a follow-up task.
- **No UI test feeds the new PascalCase types** through the Inbox normalization (fixtures use post-normalization snake_case). The pipeline itself is tested (SpecConflict/SpecAssertionFailure cases), and I mechanically verified the mapping keys match real type strings — a typo would degrade to raw-type display only, not a failure. Minor test-coverage note, not a defect.
- **entity_type vocabulary** extends the spec's exemplar comment ("agent", "mr", "spec", "gate", "budget") with "workspace", "system", "repo", "persona", "task", "secret". The spec field is a `String` with an exemplar comment, not a closed enum; extension is documented in the type docs and consistent with the spec's own structural definition.
- **`mark_read` is a silent no-op when the id/user pair doesn't match** — same contract as the pre-existing `dismiss`/`resolve`; the API layer returns 404 via the user-scoped `get` before mutating, and the cross-user no-op is tested.

## Summary

The entity model, taxonomy, dual priority model, structured source, portable migration, adapter parity (SQLite/PG/mem), read-state endpoints with per-handler auth, deep links, and all owned creation paths are real production implementations with meaningful tests (mutation probes confirm the tests kill disabled behavior). The coverage row's claims match the code, including the honest no-emitter disclosure. No structural defects, missing enforcement, unsafe scope handling, fake implementations, or weak tests found.

— Reviewer, 2026-11-08
