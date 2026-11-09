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
commits: ["98f6c234564283c3f5bebce30145b49e3e965eca", "f5d3446bb5bfbb680caa429b748ac881ad791024", "3a17bff5e5e0940306f67f13cfed844fdd38ca15", "c424ae307c4d282c095c4af28596acf34fc6cdab", "4797d0f456c06b1090d624ee98ede31a8847cc95", "a9aca60e1ff6dc564bdfc6a53d0f1572f40ceed7", "351bd57dcc59872bafe7f238dc9bb64c9f662cf3", "d6dfb7da45bbd862f5a7249c214c75e6ad475c48", "56395200e6d791faefa6eb588b2a6fc96d89f734", "722c9c29c5114fcc5bd1963f94ee054e4a42add9", "1f31ed07d12007bf3609e9a7c16378223096c0ab", "3c3d16a72e9ce48a87682cfd6a6f557b12f605db", "fd3c74d138cf11a94d2ee015b1f3c973e68d26ed", "7ce04d6903b99a04934528e856778d59e0712901"]
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
and prior review rounds repaired the MR-list enrichment, the P3 Override gate, the empty-`spec_sha`
fail-open, and the P5 task-creation mechanism. This round repaired the three findings from the
latest review round (commit `98f6c2345a4a5e21a9b95c88a2be60ac4b9fa3ab`).

### §6-§8 surface (unchanged from earlier rounds)

- **§6 Specs View** (`SpecDashboard.svelte` + `DetailPanel.svelte`): sortable table (Path, Status,
  Kind, Owner, Last Updated) at tenant/workspace scope with status/kind filters and the `owner=me`
  toggle; repo-scope progress table (path, status badge, `N/M tasks` progressbar, last updated);
  detail panel tabs Content/Edit/Progress/Links/History, each lazily backed by its real endpoint;
  `[+ New Spec]` opens the editor modal calling `POST /repos/:repo_id/specs/save`.
- **§7 Inbox** (`Inbox.svelte`): cards with priority badge, title/subtitle, type badge,
  workspace-name badge at tenant scope, relative timestamp; accordion expand with full context and
  per-type action rows for all 10 HSI §8 priority types; P2 fan-out server-side to every eligible
  workspace member with a structured body.
- **§8 Briefing** (`Briefing.svelte`): five narrative sections, inline action buttons, clickable
  entity references, time-range selector, "Ask about this briefing" chat.

### Review repairs (this round)

- **F1 (moderate) — P2 card `spec_sha` went stale on iterative saves**: `save_spec`'s
   append-to-existing-MR path wrote the new content to the MR branch and returned early (HTTP 200)
  without touching the notification, so the SpecPendingApproval card kept the FIRST save's blob
  SHA — every Approve click then recorded an approval for a SHA that never becomes the ledger's
  `current_sha` after merge (card shows "Approved", spec stays Pending, permanently). Fix: the
  append path now recomputes the blob SHA for the appended content (shared `spec_blob_sha` helper,
  extracted from the create path: `git rev-parse` primary, content-derived SHA-1 fallback) and
  refreshes the unresolved cards' body in place via a new
  `NotificationRepository::update_body_by_entity_ref(entity_ref, type, body)` port method,
  implemented by the SQLite, PostgreSQL, and mem adapters with the same unresolved/undismissed
  guard (resolved cards keep their historical body — the audit record of what was acted on).
  Anchored by two new tests: `save_spec_existing_mr_refreshes_notification_spec_sha`
  (gyre-server; two saves → still exactly one card, carrying the v2 `git hash-object` constant,
  not the stale v1 one) and
  `update_body_by_entity_ref_updates_only_unresolved_matches` (gyre-adapters; real SQLite —
  updates both fan-out recipients, skips resolved/dismissed/other-entity/other-type rows, returns
  the matched count, and updates nothing for an unknown entity).
- **F2 (minor) — P5 `resolveConflictMr` could never match in production**: the divergence body's
  `commit_sha_a/b` are default-branch push tips (graph extraction records them from git-http push
  ref updates), while `merge_commit_sha` is always a true merge commit created by the merge
  processor (never a fast-forward) — the two can never be equal in any emitter flow, so the
  losing-MR lookup was dead code and every Pick A/Pick B description fell back to "unresolved".
  Fix: dropped the resolution step entirely; the revert task description now identifies the
  losing side by agent + commit SHA (concretely actionable via `git show`/`git revert <sha>`),
  and the tests assert the description carries both sides' agents/SHAs and that no dead MR fetch
  occurs. The server-side list enrichment from the prior round remains (anchored by its Rust
  test; the single-MR `get_mr` consumer still relies on it).
- **F3 (minor) — dead API wrappers**: removed `api.revertMr` (its only caller was removed by the
  prior round's F4 repair; it survived only in negative test assertions and a comment falsely
  claiming Inbox P5 still used it) and `api.stopAgent` (zero references; `DetailPanel`'s stop
  action calls `api.adminKillAgent` through its own local function). The recovery endpoint
  `POST /repos/:id/revert/:mr_id` remains for its other consumers.

### Test evidence (focused probes; full gates owned by verification)

- `cargo test -p gyre-server --lib api::specs_assist` — 23 passed, 0 failed (includes the new
  stale-SHA refresh test).
- `cargo test -p gyre-adapters --lib sqlite::notification` — 2 passed, 0 failed (includes the new
  `update_body_by_entity_ref` SQL test).
- `cargo test -p gyre-server --lib notification` — 26 passed, 0 failed (no regressions in
  notification consumers: trust suggestions, retention, graph extraction).
- `cd web && npx vitest run src/__tests__/Inbox.test.js` — 74 passed, 0 failed (rewritten P5
  tests: SHA+agent description assertions, no-MR-fetch assertion, failure path; one test whose
  premise was the removed MR resolution was deleted with the dead code).
- `cd web && npx vitest run src/__tests__/AccordionItem.test.js src/__tests__/AccordionScoping.test.js
  src/__tests__/Briefing.test.js` — 43 passed, 0 failed.
- i18n key audit: every `$t('...')` key in Inbox/Briefing/SpecDashboard/Inbox.test resolves in
  `en.json`; the removed key `revert_task_no_mr` and the `{lose_mr}` placeholder have zero
  remaining references.
- `cargo fmt -p gyre-server -p gyre-adapters -p gyre-ports --check` clean; `scripts/check-arch.sh`,
  `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-fail-open-ref-resolution.sh`, `check-lossy-secret-conversion.sh`,
  `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh`,
  `check-relative-path-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-fabricated-scope-defaults.sh`, `check-forwarded-header-trust.sh`,
  `check-forged-scope-fields.sh`, `check-dead-message-kinds.sh`,
  `check-task-commit-attribution.sh` all pass.
- `cargo clippy -p gyre-server -p gyre-adapters -p gyre-ports --lib` — no errors; the new port
  method adds one instance of the pre-existing async-trait `#[must_use]` lint class that already
  fires for every method of every port trait (338 → 339 warnings in gyre-ports).
- The `cargo build` of gyre-server ran the web production build (`npm run build`) successfully —
  all changed Svelte/JS sources compile; `web/dist` output was restored, not committed.

Sandbox notes: frontend deps installed via `npm ci` (locked versions); `sha1`/`hex` were already
direct gyre-server dependencies from the prior round. TCP listener probe unsupported, so no local
server/browser end-to-end run; component tests + production build stand in.
