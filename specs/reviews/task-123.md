# Review — task-123 (user-management.md §Notification Entity — notification entity with full type taxonomy and priority model)

Candidate: `96caef60d1419a9577d25b36eeb87b176a2d5bbb` on base `6cc3df14b02292c30b0925e42810937737a28ea3`
(diff: +2813/−245 over 44 files; all four attributed commits `27bfb2a6`, `deec7953a`, `f92a4891`, `12f5495f` verified present in history; attribution gate OK).
Prior verification attempt `31ed4a0f` failed the changed-line rustfmt/clippy gates; repair commit `12f5495f` re-checked this round — see lint section. Verdict: **approved**.

## What was verified (independent evidence, /tmp/stage/review-evidence/)

### Entity model matches the spec contract

- `gyre-common/src/notification.rs`: all 18 spec `NotificationType` variants defined (plus the HSI §2 inbox variants and base-merged impersonation/persona variants, unified into one taxonomy); `NotificationPriority` (Low/Medium/High/Urgent) with `parse`/`as_str`/`from_rank`; `NotificationSource` {entity_type, entity_id, workspace_id, repo_id}. `Notification` carries every spec field (id, user_id, notification_type, title, body, source, priority, action_url, read, read_at, created_at) plus the HSI inbox fields the base contracts require (workspace_id, priority_rank, resolved_at, dismissed_at, tenant_id).
- Priority-band consistency is structural, not duplicated mapping: `default_priority()` = `from_rank(default_priority_rank())`, bands 1–2 Urgent / 3–4 High / 5–7 Medium / 8–10 Low. Checked every type against the spec's level examples: MrMerged/GatePassed → Low (rank 9), DependencyStale/SpecDriftDetected → Medium (7/6), AgentBudgetWarning/GateFailure → High (4/3), SecurityFinding/AgentEscalation/MrReverted → Urgent (1/2/2). Remaining assignments (AgentBudgetExhausted 1→Urgent, AgentFailed 3→High, MergeQueuePaused 3→High, BreakingChangeDetected 4→High, PersonaApprovalRequested/SpecApprovalRequested 2→Urgent, InvitationReceived/MembershipChanged 10→Low, SystemAlert 3→High) are all band-consistent and sensible.
- Renames (`SpecPendingApproval`→`SpecApprovalRequested`, `BudgetWarning`→`AgentBudgetWarning`) keep legacy DB strings parsing via `parse()` aliases, so pre-migration rows load; supersession note added to `specs/system/human-system-interface.md` §2. No lingering `NotificationType::SpecPendingApproval`/`::BudgetWarning` references anywhere (grep clean).

### Persistence is real (both backends + mem)

- Migration `2026-11-08-000059_notification_entity_source` adds `entity_type`, `action_url`, `read` (NOT NULL DEFAULT 0), `read_at`; backfills read state from resolved/dismissed timestamps; `down.sql` reverts. Version prefix `20261108000059` is unique (`check-migration-versions.sh` OK — the date prefix disambiguates from `20261008000059`); `check-migration-sql-portability.sh` OK (plain ADD COLUMN/UPDATE, no dialect-only SQL).
- SQLite and PostgreSQL adapters implement identical new row-mapper/create paths and the two new port methods: `mark_read(id, user_id, read)` (user-scoped update; stamps/clears `read_at`; cross-user is a no-op) and `has_active_notification(ws, user, type, entity_ref, since)` (filters resolved/dismissed and `created_at >= since`). Mem adapter implements both in code (user-scoped, same semantics) — `check-mem-port-contracts.sh` OK. Retention `delete_older_than` now counts marked-read as read on all three adapters.
- `schema.rs` updated for the new columns. Ran the real migration through the adapter tests (SQLite): create persists source/action_url/read; mark_read stamps/clears/user-scopes; dedup query matches type/entity/since and respects resolve; retention split includes the marked-read flag — 4/4 passed.

### Creation paths are real and correctly typed

Diff-audited every notification construction site in the server:

- Mis-typings fixed: `notify_mr_merged` now emits `MrMerged` (was the `SuggestedSpecLink` stand-in with the in-code TODO); agent failure path emits `AgentFailed` (was mis-typed `AgentEscalation`); breaking-change dependents get `BreakingChangeDetected` (was `CrossWorkspaceSpecChange`); spec-patrol drift findings get `SpecDriftDetected` (was mis-typed `GateFailure`, rank-3 High override correctly kept on both priority fields); `invite_member` emits `InvitationReceived` (was mis-typed `TrustSuggestion`).
- New emitters, each with a discriminating test asserting recipients, priority band, dedup, and/or deep link: `PersonaApprovalRequested` (create_persona → owner or workspace Admins/Owners; Viewer excluded), `AgentBudgetWarning` (≥80% → spawner+Admins, High) / `AgentBudgetExhausted` (≥100% → spawner+Owner, Urgent), both deduped once per budget period via `has_active_notification` keyed on workspace+period_start, and no notification below 80%; `GatePassed` (fires only when the finishing gate makes all gates green, deduped per user+MR); `MergeQueuePaused` (pause_merge_queue → Admins/Owners only, High, repo deep link); `DependencyStale` (staleness job, on stale transition only — second cycle produces no duplicate); `MembershipChanged` (remove_member).
- All other construction sites (specs_assist save_spec/spec-conflict, git_http persona-push/cross-workspace, impersonation, secret_rotation, prompt_assembly, reconciliation, trust_suggestion, spec_link_staleness, constraint_check, graph_extraction, mcp uncertainty, abandoned_branch, merge_processor cascade/circuit-breaker/escalation paths) migrated to the structured `source` model; activity/graph/briefing readers updated to read `source.*`.
- `notify_rich` derives `action_url` through workspace slug + repo name to the SPA entity routes; repo-less sources fall back to the workspace home; unresolvable workspace → `None` (never fabricated) — all three behaviors asserted in tests. No production code writes `priority`/`priority_rank` inconsistently (only the spec_patrol override, which sets both together).

### API + UI

- `POST /api/v1/notifications/:id/read|unread` registered in `api_router`, added to the ABAC resolver as exempt with per-handler auth identical to the pre-existing dismiss/resolve handlers (user-scoped `get` + tenant check — diff-compared line by line). `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` (75 handlers) both OK. Endpoint test drives the real router: read/unread cycle stamps/clears `read_at`; another user's notification is 404 with no mutation.
- List response keeps numeric `priority` (rank) and `entity_ref`/`repo_id` for the existing web Inbox and CLI consumers (gyre-cli verified: JSON-only consumer, compiles clean) and adds `priority_level`, `entity_type`, `action_url`, `read`, `read_at`. `docs/api-reference.md` updated (and the never-existed `PUT .../read` row fixed).
- Web Inbox (`Inbox.svelte`) maps renamed + all new taxonomy types (legacy names kept for pre-migration rows) with en.json labels; `npm ci` + vitest: Inbox 38/38, adjacent suites (WorkspaceHomeQueuePause, WorkspaceHome, AppShell, UserProfile) 101/101.

### Lint gates (the prior verification failure)

- `check-rustfmt-diff.py 6cc3df14` → clean (35 Rust files).
- `check-clippy-diff.py` invokes workspace-wide cargo, which this sandbox's executor wrapper rejects; ran the equivalent focused probes: `cargo clippy -p gyre-common -p gyre-ports` (zero warnings), `-p gyre-server --lib` (214 warnings, all pre-existing debt) and `-p gyre-server --tests` (238, pre-existing). Intersected every warning location with the task's changed-line set (2,505 lines from `--unified=0` diff): **zero warnings on changed lines**. No `[features]` sections in the touched crates, so `--all-features` is equivalent to default. The prior failures (`double_must_use` on the ports trait, `useless_format` in merge_processor test) are fixed with the documented allows/literal and no longer fire.

### Test results (focused; full command output in evidence dir)

| Probe | Result |
|---|---|
| `cargo test -p gyre-common` (full) | 124 passed, 0 failed (8 notification tests incl. 18-variant round-trip, legacy parse, band mapping) |
| `cargo test -p gyre-adapters --lib` (full) | 428 passed, 0 failed (runs the real migration) |
| `cargo test -p gyre-server --lib -- notification budget persona staleness spec_patrol gate_passed merge_queue impersonation membership` | 132 passed, 0 failed |
| `cargo test -p gyre-server --test graph_integration divergence` | 5 passed, 0 failed |
| `cargo check -p gyre-cli` | clean |
| vitest Inbox + adjacent | 38 + 101 passed |
| 20 mechanical gate scripts (arch, abac ×3, mem-port-contracts, dead-message-kinds, byte-slice, in-memory-state, fabricated-scope, lossy-secret, scope-literal, inert-enforcement, forwarded-header, forged-scope, unbounded-http, relative-path, fail-open, task-commit-attribution, migration-versions, migration-sql-portability) | all OK |

## Deferred to verification (sandbox restrictions, not code defects)

- `cargo test --all` — the executor wrapper rejects workspace-wide invocations; every touched package was run focused instead, and all test targets compiled clean via `cargo clippy -p gyre-server --tests` (covers common/ports/adapters/server test code).
- Full `web` vitest suite and full `gyre-server` lib suite (1,548 additional unfiltered tests) — diff-focused subsets all green.

## Minor observations (non-blocking)

- Spec sketch shows `body: String`; implementation keeps `body: Option<String>` — the pre-existing HSI §2 JSON-payload shape the Inbox quick-link buttons parse. Every emitter that promises a body populates it. Defensible unification, not hollow.
- Deep-link auto-derivation lives on the `notify_rich` path; direct `Notification::new` construction sites (impersonation, secret_rotation, prompt_assembly, etc.) leave `action_url` None. The spec makes the field optional, those notifications carry body-JSON quick links, and no link is fabricated — acceptable.
- `SecurityFinding` and `SystemAlert` are defined with priority mapping but have no emitting feature (no security scanner / system-alert source exists in the platform). Honestly documented in the coverage row rather than wired to a fake event — the correct call under the no-fakery rules; ownership stays with future tasks.
