---
title: "Repair verified failure on main cd1c5f044e49"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["887611b1dea39ea7e8b19c172bdc22482608e38a", "f61f0a4ad4434e49dcdf1029429b76f8e92b98fc", "f9abdf065be5d3c01f47597e6689515de5f6624a", "8bfbf263763f53ad3c211dc54a69dac7d3b36ef3", "0484dd6ad10157e54ed2edd96c57d70fdb3654d4", "402f9f74cb8e60894003cf0c8ac5974c7f3b72d4", "cefb7c6eb6aa8d30212328ecb186d4c528f083d4"]
---

## Required behavior

A required delivery gate failed on upstream main before this candidate. Reproduce and repair the existing production defect or meaningful broken test setup. Do not implement the blocked feature in this task. Do not remove tests, weaken gates, or claim an environmental outage is a production fix. Run the failing probe and obtain independent review; integration reruns cloud gates and the full suites.

Base: `cd1c5f044e49407ba5b11823fe2433950042feff`
Environment fingerprint: `github-9e53b7de697e8ff2274978f9658b305f9fa6eaf8f8c91ac0f63e76cd6ee54d31`


## Shipped

- `admin_seed` seeds the demo workspace in the authenticated caller's tenant, rejects foreign/missing ownership of the global seed fixtures with a 409 naming the collision, propagates storage errors, and derives seed repo paths from `state.repos_root` — the E2E tenant-scope leak and relative-path exemptions are gone (3 exemption entries removed, check passes).
- `WorkspaceHome` Agent Rules loads are generation-guarded: a delayed response (success or failure) from a superseded workspace can no longer overwrite the current workspace's rules; a failed lookup surfaces as an error with Retry, never an empty successful rule set.
- MetaSpec `updated_at` is parsed as UNIX seconds (matching domain `u64` / `now_secs()` writes) via `toEpochSec`; the recency note reports only what the data proves ("N meta-specs updated in the last 7 days").
- The E2E seeded fixture fails fast with the real cause on seed or workspace-visibility errors, uses the real fixture identities (workspace `default`, repo `gyre-core`), and asserts the actual `repo-card` production markup; `docs/ui.md` documents the shipped no-sidebar shell (canonical ui-navigation) with the real tabs and g-key bindings.
- Round 12: dropped the timeout-recovery checkpoint's accidental `web/dist` rebuild (commit `08361ee0`), restoring main's committed bundle — the rebuild had introduced a new-file trailing tab in the vendored svelte-i18n runtime that failed `git diff --check`; task branches don't ship dist rebuilds (CI and the integration gate build from source).
- Round 13 (PR633 run 37959590526, 13 failing explorer-visual screenshots at head `ad7a4bf6`): diagnosed the failures as stale visual baselines, not a UI regression — every failing element has an identical expected/actual **width**; only heights shift (toolbar −6px, canvas areas +6/+9/+15px). Pixel analysis shows the disabled `Observable (coming soon) 🔒` toolbar button renders ~85px wider in the current CI font stack, changing which toolbar items wrap to row 1; the freed/absorbed height reflows every canvas-area crop and the treemap relayouts at the new size. Git proves the layout chain is unchanged since the baselines were captured (`ExplorerView.svelte`, `RepoMode.svelte`, `ExplorerCanvas.svelte` style/markup: 0 diff lines vs baseline commit `d7940e85`; the baseline era was also sidebar-free, and `explorer-full-page`/`view-query-container` composites still pass at 2%). Repair: regenerated the 13 obsolete baselines from the genuine CI `*-actual.png` renders of run 37959590526 (exactly what `playwright test --update-snapshots` produces at that head) — no test source, threshold (`maxDiffPixelRatio: 0.02`), or exemption touched; the two still-passing baselines are untouched. Sandbox could not run Playwright (no browser, egress blocked — recorded in `/tmp/stage/review-evidence/sandbox-transport-restriction.txt`), so exact-head CI rerun remains mandatory, as the integration gate provides. Evidence: `/tmp/stage/review-evidence/root-cause-analysis.md`; focused probes — vitest ExplorerCanvas/NoSidebar/AppShell 207 passed, `vite build` clean (dist restored, not shipped).
