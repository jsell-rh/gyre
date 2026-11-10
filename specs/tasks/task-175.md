---
title: "Specs and Inbox view layouts — spec list, inbox cards, briefing narrative"
spec_ref: "ui-layout.md §6-§8"
depends_on: [task-214]
progress: ready-for-review
coverage_sections:
  - "ui-layout.md §6. Specs View Layout"
  - "ui-layout.md §7. Inbox Layout"
  - "ui-layout.md §Item Structure"
  - "ui-layout.md §Action Buttons per Item Type"
  - "ui-layout.md §8. Briefing Layout"
commits: ["98f6c234564283c3f5bebce30145b49e3e965eca", "f5d3446bb5bfbb680caa429b748ac881ad791024", "3a17bff5e5e0940306f67f13cfed844fdd38ca15", "c424ae307c4d282c095c4af28596acf34fc6cdab", "4797d0f456c06b1090d624ee98ede31a8847cc95", "a9aca60e1ff6dc564bdfc6a53d0f1572f40ceed7", "351bd57dcc59872bafe7f238dc9bb64c9f662cf3", "d6dfb7da45bbd862f5a7249c214c75e6ad475c48", "56395200e6d791faefa6eb588b2a6fc96d89f734", "722c9c29c5114fcc5bd1963f94ee054e4a42add9", "1f31ed07d12007bf3609e9a7c16378223096c0ab", "3c3d16a72e9ce48a87682cfd6a6f557b12f605db", "fd3c74d138cf11a94d2ee015b1f3c973e68d26ed", "7ce04d6903b99a04934528e856778d59e0712901", "db5b4f8222cacbf7e1531a7b1725a6866380fdfa", "956897081069ec8199ba0bf6e706fbfb2dd2af72"]
---

## Spec Excerpt

**Specs View Layout (§6)**: Full-Width layout. Tenant/workspace scope: sortable table (Path, Status, Kind, Owner, Last Updated). Repo scope: spec list with implementation progress (path, status, task progress bar). Click spec → detail panel with tabs: Content, Edit, Progress, Links, History. New spec creation via `[+ New Spec]` → Editor Split. Data from `GET /api/v1/specs?workspace_id=` and per-spec endpoints.

**Inbox Layout (§7)**: Cards with priority badge, description, workspace attribution, timestamp. Accordion expand shows full context (uncertainty text, related spec, agent info) with action buttons per item type (10 priority levels). Expanded cards include inline action buttons: Respond, View Spec, Approve, Reject, Retry, Override, etc.

**Briefing Layout (§8)**: Full-width narrative with sections: COMPLETED, IN PROGRESS, CROSS-WORKSPACE, EXCEPTIONS, METRICS. Action buttons inline in narrative. Entity references (spec, agent, MR names) are clickable → open detail panel. "Ask about this briefing" chat input at bottom.

## Implementation Plan

1. **Specs View compliance** (`SpecDashboard.svelte`):
   - Verify spec list matches spec: Path, Status, Progress bar, Last Activity columns
   - Detail panel tabs: Content, Edit, Progress, Links, History
   - `[+ New Spec]` button opens Editor Split (depends on task-172)
   - `?owner=me` toggle and status filters

2. **Inbox Layout compliance** (`Inbox.svelte`):
   - Verify card structure: priority badge, description, workspace/repo attribution, timestamp
   - Accordion expand with full context
   - Action buttons per all 10 priority levels (per HSI §8 priority table)
   - Workspace attribution badges at tenant scope

3. **Briefing Layout compliance** (`Briefing.svelte`):
   - Verify section structure: COMPLETED, IN PROGRESS, CROSS-WORKSPACE, EXCEPTIONS, METRICS
   - Entity references as clickable links → detail panel
   - "Ask about this briefing" chat input
   - Time range selector

4. **Tests**:
   - Spec list renders with progress bars
   - Inbox accordion expands with correct action buttons
   - Briefing narrative renders all sections

## Acceptance Criteria

- [ ] Specs view renders sortable table with implementation progress
- [ ] Spec detail panel has Content/Edit/Progress/Links/History tabs
- [ ] Inbox cards show priority badge, description, attribution, timestamp
- [ ] Inbox accordion shows action buttons for all 10 priority levels
- [ ] Briefing renders COMPLETED/IN PROGRESS/CROSS-WORKSPACE/EXCEPTIONS/METRICS sections
- [ ] Entity references in briefing are clickable (open detail panel)
- [ ] Tests pass

## Agent Instructions

Read `ui-layout.md` §6-§8 for the exact layout specifications. Also reference `human-system-interface.md` §8 for the full inbox priority table (10 item types with their action buttons). The existing `SpecDashboard.svelte`, `Inbox.svelte`, and `Briefing.svelte` are the starting points — this task is about aligning them with the spec's layout requirements, not building from scratch. Check the data source endpoints mentioned in the spec: `GET /api/v1/specs`, `GET /api/v1/users/me/notifications`, `GET /api/v1/workspaces/:id/briefing`.

## Shipped

The full §6-§8 surface was built across earlier rounds (spec table + detail tabs + editor split,
all-10-priority Inbox cards with per-type actions, briefing narrative with entity links and chat),
and review rounds repaired the MR-list enrichment, the P3 Override gate, the empty-`spec_sha`
fail-open, the P5 task-creation mechanism, and the stale-`spec_sha`/dead-P5-lookup/dead-wrapper
findings (commit `98f6c234`). This round repaired the three verification-gate failures from the
failed `checks.sh` run (commit `db5b4f82`); no product behavior changed.

### §6-§8 surface (unchanged)

- **§6 Specs View** (`SpecDashboard.svelte` + `DetailPanel.svelte`): sortable table (Path, Status,
  Kind, Owner, Last Updated) at tenant/workspace scope with status/kind filters and the `owner=me`
  toggle; repo-scope progress table (path, status badge, `N/M tasks` progressbar, last updated);
  detail panel tabs Content/Edit/Progress/Links/History, each lazily backed by its real endpoint;
  `[+ New Spec]` opens the editor modal calling `POST /repos/:repo_id/specs/save`.
- **§7 Inbox** (`Inbox.svelte`): cards with priority badge, title/subtitle, type badge,
  workspace-name badge at tenant scope, relative timestamp; accordion expand with full context and
  per-type action rows for all 10 HSI §8 priority types; P2 fan-out server-side to every eligible
  workspace member with a structured body. Iterative saves refresh the unresolved cards'
  `spec_sha` in place via `NotificationRepository::update_body_by_entity_ref` (SQLite, PostgreSQL,
  and mem adapters, unresolved/undismissed guard).
- **§8 Briefing** (`Briefing.svelte`): five narrative sections, inline action buttons, clickable
  entity references, time-range selector, "Ask about this briefing" chat.

### Verification-gate repairs (this round, commits `db5b4f82` + `956897081`)

- **E0063 compile error (lib test)**: main's task-108 added the `impersonation` field to
  `auth::AuthenticatedAgent`; the branch merge fixed one test initializer in `specs_assist.rs`
  but missed the P2 fan-out test's initializer at line 1177, breaking `cargo clippy --all-targets`
  (and the lib-test build). Added `impersonation: None` there — the only remaining stale
  initializer in the task's changed files.
- **rustfmt (changed lines)**: the new SQLite `update_body_by_entity_ref` test used six over-long
  single-line `assert_eq!`s; `check-rustfmt-diff.py` flagged lines 457-463. Reformatted to
  rustfmt's multi-line form; `cargo fmt --check` now clean for all three touched crates.
- **clippy::double_must_use and clippy::too_many_arguments (changed lines)**: async_trait's
  macro-generated warnings carry primary spans covering the whole trait (line 9 through each
  method), so adding `update_body_by_entity_ref` made the file's pre-existing macro-noise
  warnings overlap the diff and fail `check-clippy-diff.py`. `NotificationRepository` was the one
  port trait missing the repo-standard `#[allow(clippy::double_must_use)] // async_trait generates
  a must_use future for each method.` annotation carried by every other port trait (workspace,
  repository, tenant, audit, git_ops, …); added it, plus the same method-level
  `#[allow(clippy::too_many_arguments)]` used for that lint elsewhere (auth.rs,
  graph_extraction.rs) on the pre-existing 7-parameter `list_for_user` filter signature. Zero
  clippy warnings remain for the file.

### Test evidence (focused probes; full gates owned by verification)

- `cargo test -p gyre-server --lib api::specs_assist` — 23 passed, 0 failed (includes the
  stale-SHA refresh test and the repaired fan-out initializer).
- `cargo test -p gyre-adapters --lib sqlite::notification` — 2 passed, 0 failed (reformatted
  `update_body_by_entity_ref` SQL test).
- `python3 scripts/check-rustfmt-diff.py 89025d728` — "changed lines clean (7 Rust files
  checked)".
- `python3 scripts/check-clippy-diff.py 89025d728` — re-run after the fixes: no failures
  (the 14 double_must_use + 2 too_many_arguments lines and the E0063 are gone; full
  `--all-targets` clippy compiles). Verified 0 flagged warnings against both the assignment
  base `89025d728` and the failed run's base `b5a872087`.
- Prior rounds' evidence (Inbox 74, Accordion/Briefing 43 vitest passes, notification-consumer
  regressions 26, i18n key audit, architecture scripts) is unchanged by this round — no web or
  product source was touched, only the three Rust gate repairs above.

Sandbox notes: builds serialized via /tmp/stage/bin/cargo with SKIP_WEB_BUILD=1 (web/dist
untouched, per assignment rules). TCP listener probe unsupported, so no local server/browser
end-to-end run; component tests from prior rounds stand in.
