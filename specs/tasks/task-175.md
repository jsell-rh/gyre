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
commits: ["f5d3446bb5bfbb680caa429b748ac881ad791024", "3a17bff5e5e0940306f67f13cfed844fdd38ca15", "c424ae307c4d282c095c4af28596acf34fc6cdab", "4797d0f456c06b1090d624ee98ede31a8847cc95", "a9aca60e1ff6dc564bdfc6a53d0f1572f40ceed7", "351bd57dcc59872bafe7f238dc9bb64c9f662cf3", "d6dfb7da45bbd862f5a7249c214c75e6ad475c48", "56395200e6d791faefa6eb588b2a6fc96d89f734", "722c9c29c5114fcc5bd1963f94ee054e4a42add9", "1f31ed07d12007bf3609e9a7c16378223096c0ab", "3c3d16a72e9ce48a87682cfd6a6f557b12f605db", "fd3c74d138cf11a94d2ee015b1f3c973e68d26ed", "7ce04d6903b99a04934528e856778d59e0712901"]
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
all-10-priority Inbox cards with per-type actions, briefing narrative with entity links and chat).
This round repaired the four review findings from the prior verification round
(commit `f5d3446bb5bfbb680caa429b748ac881ad791024`).

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

- **F1 (major) — P5 arbitration could not resolve MRs**: `GET /merge-requests` (list) never
  serialized `merge_commit_sha` (`MrResponse::from` hardcoded `None`; only the single-MR `get_mr`
  enriched it from the attestation store), so Pick A/Pick B always failed with "Could not find the
  merged MR". Fix: extracted `enrich_merged_attestation()` (the same attestation lookup `get_mr`
  performs) and applied it in `list_mrs`, `list_workspace_mrs`, and `get_mr`
  (`crates/gyre-server/src/api/merge_requests.rs`). Anchored server-side by the new Rust test
  `list_mrs_enriches_merged_rows_with_merge_commit_sha` (open→approved→merged transition + stored
  attestation, then asserts the list row carries `merge_commit_sha`/`merged_at`) — the web test
  mock now mirrors that real wire format instead of fabricating it.
- **F2 (moderate) — P3 Override gated nothing**: `handleOverrideGate` only recorded an approval
  review, but the merge processor's human-approval escape is `mr.status == Approved` set via
  `PUT /merge-requests/:id/status` (`supervised_trust_hold` returns `None` only for
  `MrStatus::Approved`). Fix: Override now submits the review AND sets the MR status to `approved`;
  the test asserts both calls.
- **F3 (minor) — silent pre-edit approval**: the server wrote `spec_sha: ""` into the
  spec-pending-approval notification when git was unavailable (`get_blob_sha(...).unwrap_or_default()`
  fail-open), and the Inbox ledger fallback then approved the PRE-edit version for a still-open
  spec-edit MR (invalidated on merge). Fixes: (a) server derives the blob SHA from the exact saved
  content (`sha1("blob <len>\0<content>")`, the same value `git rev-parse`/`git hash-object`
  returns for those bytes) instead of `""`; (b) the client ledger fallback now requires the
  spec-edit MR to be merged (ledger then holds the approved content) and otherwise fails with
  `missing_spec_sha`. The Rust test asserts the exact git-produced SHA constant
  (`8035a516…79d4`, produced by `git hash-object`); web tests cover both the open-MR failure and
  merged-MR success paths.
- **F4 (minor) — spec divergence in P5 mechanism**: HSI §8 P5 speccs Pick A/Pick B as creating a
  task via the existing task domain model; the code performed a direct revert
  (`POST /repos/:id/revert/:mr_id`), contradicting the spec. Fix: Pick A/Pick B now create a
  `spec-implementation` revert task (`api.createTask`, labels `['spec-implementation', 'revert']`)
  with the selected implementation as reference; the losing MR is resolved best-effort by
  merge-commit SHA (exercising the F1 list enrichment) so the task description is concretely
  actionable, and the commit SHA alone carries the task when the MR cannot be resolved. The direct
  `revertMr` call is removed from the Inbox; the recovery endpoint remains for its other consumers.
  No spec amendment needed — the code now implements the specced mechanism.

### Test evidence (focused probes; full gates owned by verification)

- `cargo test -p gyre-server --lib api::merge_requests` — 34 passed, 0 failed (includes the new
  list-enrichment test).
- `cargo test -p gyre-server --lib api::specs_assist` — 22 passed, 0 failed (includes the
  strengthened exact-SHA assertion).
- `cd web && npx vitest run src/__tests__/Inbox.test.js` — 75 passed, 0 failed (rewritten P5
  task-creation tests with wire-format mock, F2 dual-call override assertion, F3 open/merged
  fallback tests, task-creation failure path).
- `cd web && npx vitest run src/__tests__/AccordionItem.test.js src/__tests__/AccordionScoping.test.js
  src/__tests__/Briefing.test.js` — 43 passed, 0 failed.
- i18n key audit: every `$t('...')` key in Inbox/Briefing/SpecDashboard/Inbox.test resolves in
  `en.json`; removed keys (`side_picked`, `no_conflict_mr`) have zero remaining references.
- `cargo fmt -p gyre-server --check` clean; `scripts/check-arch.sh`,
  `check-fail-open-ref-resolution.sh`, `check-inert-enforcement.sh`,
  `check-lossy-secret-conversion.sh`, `check-mem-port-contracts.sh`,
  `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh`,
  `check-task-commit-attribution.sh` all pass.
- `cd web && npx vite build` clean (dist not committed, per task-210 convention).

Sandbox notes: frontend deps installed via `npm ci` (locked versions); `sha1 = "0.10"` was already
in Cargo.lock (transitive) so the new gyre-server dependency adds no new crate version. Clippy on
this toolchain flags a pre-existing `explicit_counter_loop` in untouched `gyre-common` code — not
introduced by this round. TCP listener probe unsupported, so no local server/browser end-to-end
run; component tests + production build stand in.
