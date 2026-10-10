# Review: task-065 — ExplorerCanvas: Unified Canvas with Semantic Zoom Treemap

Spec: `explorer-canvas.md` §1–3, §8; `explorer-implementation.md` §1–2, §16, §17, §25.
Candidate: `0d11891b` (assigned base `770785f7`). Task commits: `b5fd081f`,
`d4bbe576`, `db9f944b`, `240aab15`, `3fb05db2`, `6110fe43`, `58dddc24`,
`d82a5a56` (all recorded in `commits:` frontmatter; attribution gate OK at head).
Verdict: **approved**.

## Round 2 (this review — font-truth CI-repair candidate)

Delta since the approved round-1 review (`6effe94c`, candidate `3690d7a3`):
the visual-baseline font-truth repair (b5fd081f lineage — committed woff2
fixtures + forced-fonts.css + spec wiring + regenerated baselines + probe
configs), an en.json `explorer_chat.*` key repair, and lifecycle bookkeeping.
The round-1 product surface (unified cutover, §17 props, lenses, presets,
treemap, view queries, interactions, minimap) was re-audited at this head.

### Contract and baseline

- Task-file contract region intact; only `depends_on`/`progress`/`commits:`
  and appended `## Shipped` history changed — sanctioned bookkeeping.
- `bash scripts/check-task-commit-attribution.sh` at candidate HEAD → OK,
  exit 0. `check-arch.sh`, `check-mem-port-contracts.sh` → pass.

### Diff audit (base..candidate: 55 files, +1308/−3967)

- Unified cutover unchanged from round 1: MoldableView/FlowCanvas/
  FlowRenderer/ExplorerFilterPanel/duplicate NodeBadge deleted with their
  orphaned tests; no live imports remain (git grep at candidate: only one
  comment mention). ExplorerView hosts a single ExplorerCanvas with
  `bind:filter`/`bind:lens` (dead `filters={null}` prop dropped).
- §17 props verified at `ExplorerCanvas.svelte:19-44`; three lenses at
  `:4969-4972` (Observable `aria-disabled`, spec-exact label); five presets
  (`FILTER_PRESETS :1536-1542`) with node dimming + edge gating in
  `canvas-filters.js`; drill/breadcrumb/minimap/interaction handlers all
  present at the previously-cited lines.
- `en.json` `explorer_chat.*` additions repair a pre-existing i18n gap:
  `ExplorerChat.svelte` (unchanged since d7940e85, outside this task's
  surface) referenced `status_*`/`suggestion_*`/`save_this_view` keys that
  base `en.json` never defined. Key rename + additions, no behavior change;
  pinned by dom-probe test 6 ("chat rail renders localized strings").
- Font fixtures authentic: all 11 committed woff2 files are byte-identical
  (`cmp`) to `@fontsource/red-hat-{text,mono,display}@5.3.0` published npm
  tarballs — real binaries, same typefaces Google Fonts serves. The forced
  font environment (empty fonts.googleapis.com stylesheet, /test-assets/
  fonts route, document_start injection, ctx.font patch) is coherent and
  applies to both CSS text and canvas labels.
- Committed `web/dist` is current: `npm run build` at the candidate produces
  a byte-identical dist (git status clean after rebuild).

### Independent probes (evidence: /tmp/stage/review-evidence/)

- `npm ci` → 169 packages (locked); `npm test` → 56 files, **1513 passed |
  41 skipped, 0 failed**; focused `canvas-filters` + `ExplorerCanvas` → 157
  passed.
- Static node http server on 127.0.0.1:2222 serving `web/dist` (with the
  seeded fixture's bootstrap APIs answered) + sandbox chromium:
  - `gyre-dom-probe.spec.js` → **7 passed** (toolbar contents, evaluative
    overlay + playback controls, annotation bar, blast-radius count, filter
    active state, chat-rail i18n, ws status) — structural verification
    against the real SPA.
  - `gyre-playwright-visual.config.js` clean compare → **15 passed**, no
    baseline writes (git status clean). Baselines deterministic under the
    forced-font environment.
  - Two initial probe failures were review-harness bugs in my own server
    (octet-stream SPA fallback; empty workspaces list), fixed in the probe
    server, not candidate code — recorded in the evidence log.
- Mutation re-proof against pristine tree (restored + git-diff-verified
  after each): dependencies→unconditional 0.1 → 2 fail; depends_on
  participants dropped → 4 fail; dim 0.1→0.5 → 8 fail. All killed.
- Sandbox transport: python TCP-listener probe unsupported (errno 95,
  /tmp/stage/capabilities.json) but node http listeners bind fine; live
  gyre-server browser check deferred to host verification. Authoritative
  gate: exact-head GitHub CI e2e (npm ci, npm run build, playwright vs real
  release server) — CI loads the same committed woff2 binaries through the
  route interception, so the regenerated baselines should compare equal
  there. If CI still differs, inspect its artifacts for font-truth
  divergence before touching baselines.

All acceptance criteria verified with file:line evidence; no structural
defects, fake implementations, missing enforcement, unsafe scope handling,
or weak tests found.

**Verdict: approved** (`/tmp/stage/verdict.json`, empty findings).
