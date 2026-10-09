# Review: TASK-210 — Repair verified failure on main cd1c5f044e49

**Reviewer:** Independent reviewer
**Date:** 2026-10-09
**Candidate:** `296e07411ebd3de30c754f78c87dae94a31c09ed` (base `053bd318526b3eec0b515e14e9f58e28fa3091a2`, ancestry confirmed)
**Verdict:** approved

---

## What was reviewed

Diff base..candidate (31 files): admin_seed tenant-scoping repair, WorkspaceHome
Agent Rules generation guard + failure surfacing, MetaSpec `updated_at`
seconds-parsing, E2E seeded-fixture fail-fast + real fixture identities,
docs/ui.md no-sidebar documentation, and the round-13 regeneration of 13
explorer-visual baselines from CI run 37959590526.

## Independent verification

- `cargo test -p gyre-server --lib api::admin` → **32 passed / 0 failed**,
  including foreign-tenant 409, foreign-owned workspace-id 409 (nothing
  written), orphan-repo 409, idempotency, workspace-visible-to-caller.
- `scripts/check-relative-path-defaults.sh` → OK; the 3 seed exemption entries
  are genuinely removed, not bypassed.
- vitest (locked `npm ci`): WorkspaceHomeRulesFailure + Sections **40 passed**
  (the delayed cross-workspace response test fails if the generation guard is
  removed); ExplorerCanvas/NoSidebar/AppShell **207 passed**;
  WorkspaceDrawerSectionNav **7 passed**. Full suite 1533 passed with one
  pre-existing load-flake (ExplorerCanvas-performance 10k-nodes timing,
  untouched by this candidate, passes in isolation).
- `npm run build` clean; dist not shipped (restored; tree clean at candidate).
- Baseline regeneration traceability: run 37959590526 confirmed via GitHub API
  as head `ad7a4bf6…`, PR 633, base `053bd318` — the exact failing head. PNG
  dimension deltas old→new match the CI failure report exactly (toolbar
  74→68, canvas crops +6/+9/+15, widths identical). Structural pixel analysis
  shows the same UI content with different text rasterization (font-metric
  drift), not different content.
- Root-cause audit: d7940e85..ad7a4bf6 ExplorerCanvas.svelte diff is entirely
  inside `<script>` (helper extraction refactor); toolbar markup/styles,
  ExplorerView, RepoMode, package.json/lock, playwright.config all unchanged;
  App.svelte deltas are mobile-drawer-only (hidden at 1280px). No diff line
  plausibly shifts explorer geometry — the stale-baseline diagnosis holds.
- Real-browser probe (chromium-1208 @ 1280x720): toolbar renders and
  screenshots successfully; geometry is font-stack-sensitive, consistent with
  the diagnosis. In-sandbox E2E run is blocked by a socket-accept transport
  restriction (errno 95), **not** by browser availability — see below.

## Findings

- [x] [noted-not-blocking] **F1: Shipped note misstates the reproduction
  blocker.** The round-13 note says local Playwright reproduction was
  impossible due to "no browser, egress blocked". The current sandbox has
  Playwright 1.58.2 with chromium_headless_shell-1208 and full chromium-1208
  installed (`PLAYWRIGHT_BROWSERS_PATH=/usr/local/share/gyre-playwright`); the
  actual blocker is that the sandbox blocks socket accept() (gyre-server
  listens but every connection fails with errno 95; curl gets an empty
  reply). The operative conclusion — the exact-head E2E rerun must happen in
  CI, which the integration gate provides — is unaffected, but future notes
  should name the real restriction. Evidence:
  `/tmp/stage/review-evidence/sandbox-transport-restriction.txt`.

## Verifier instruction (decisive check, needs unrestricted transport)

```
cd web && npm ci
SKIP_WEB_BUILD=1 cargo build --release -p gyre-server
cd web && CI=1 GYRE_AUTH_TOKEN=e2e-test-token GYRE_PORT=2222 \
  npx playwright test tests/e2e/explorer-visual.spec.js
```
Expected: 15/15 pass against the regenerated baselines at the candidate head.
