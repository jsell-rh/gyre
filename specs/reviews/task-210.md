# Review — task-210 (Repair verified failure on main cd1c5f044e49)

Candidate: `a915fcbc1519e1828613c45bdbe61a21f8070f98` against assigned base `66422bd4b99de70536cce8422ec23db5eaec082d` (= `origin/main` at review time; the `cd1c5f04` Base in the task file is diagnostic provenance per prior review rounds). Verdict: **approved**.

Task contract: a required delivery gate (GitHub E2E, 37 failures) failed on upstream main. The candidate repairs the production defects and broken test setup behind those failures without implementing the blocked feature, removing tests, or weakening gates. All findings below were probed independently with fresh, isolated build artifacts.

## Scope of the diff (18 files, `66422bd4..a915fcbc`)

1. **`crates/gyre-server/src/api/admin.rs`** — `admin_seed` derives tenant scope from the authenticated caller (`auth.tenant_id`, JWT-validated; static token resolves "default"), rejects foreign ownership of the global seed fixtures with 409 naming the collision (repo→workspace path and workspace-id path), rejects inconsistent seed data (orphan repo without workspace) as 409 instead of reporting success, propagates storage errors (`?` instead of `let _ =`), and derives seed repo paths from `state.repos_root` (removing 3 relative-path exemptions — gate tightened, `check-relative-path-defaults.sh` OK).
2. **`web/src/components/WorkspaceHome.svelte`** — Agent Rules loads are generation-guarded (`rulesRequestSeq`): a delayed response (success or failure) from a superseded workspace cannot overwrite the current workspace's rules; failures surface as an error with Retry. Recency uses `toEpochSec` (MetaSpec `updated_at` is `u64` seconds, `meta_spec.rs:141`, written via `now_secs()`), replacing a ms-misparse and unsupported "Reconciling" copy.
3. **`web/src/App.svelte`** — removes the permanent sidebar (HSI §1.3, superseded by `ui-navigation.md` Principle 5) and its ⌘1-6 shortcuts; drawer navigation maps to workspace-home sections preserving scope; g-key tab map reordered to the spec's `g 1`-`g 4` (Specs/Architecture/Decisions/Code) with additive `g 5`-`g 8`.
4. **E2E fixture/spec** — `seeded.js` fails fast on seed or workspace-visibility errors and exports the real fixture identities (workspace slug `default`, repo `gyre-core`); `app.spec.js` targets real production markup (`repo-card`/`repo-card-name` in `RepoCard.svelte`, verified present).
5. **`docs/ui.md`** — documents the shipped no-sidebar shell, real tab list (verified against `RepoMode.svelte` TABS), and real g-key bindings.

## Independent evidence (all under `/tmp/stage/review-evidence/`)

Every Rust run used a target dir created empty in this session (`/tmp/review-task210-target` for the assigned checkout; separate dirs per probe worktree; all probe worktrees and targets removed after use). The shared `/tmp/gyre-target` contains no `gyre_server` binaries (only `.d` files and proc-macro `.so`s), so no cross-checkout replay was possible; isolated targets were used regardless.

- **Assigned checkout, fresh compile (363s):** `cargo test -p gyre-server --lib admin::tests` → **32 passed, 0 failed**, including all four tenant-scope tests (`rust-admin-tests-review.txt`).
- **Mutation probe 1 — guard removed** (`admin.rs:443` → `if false {`, isolated worktree + isolated target): same suite → **31 passed, 1 FAILED**, `admin_seed_rejects_caller_from_foreign_tenant` panicked 200 ≠ 409, exit 101 (`mutation-probe-review.txt`).
- **Mutation probe 2 — production reverted, tests kept** (`admin_seed` function replaced with its base `66422bd4` version, tests untouched): **28 passed, 4 FAILED** — all four new regression tests fail on the pre-fix code (`prefix-revert-probe-review.txt`). The tests defend the actual repair.
- **Mutation probe 3 — JS generation guards deleted** (`WorkspaceHome.svelte:338,342`): `keeps workspace B rules when workspace A finishes loading after navigation` FAILS; passes on the assigned checkout (`js-mutation-probe-review.txt`).
- **Mutation probe 4 — epoch-seconds misparse** (`toEpochSec(m.updated_at)` → `m.updated_at / 1000`): `treats meta-spec updated_at as epoch seconds, not milliseconds` FAILS; passes on the assigned checkout (`epoch-mutation-probe-review.txt`).
- **Frontend (assigned checkout, `npm ci` fresh):** WorkspaceHome suites (Home + Sections + RulesFailure) → 44 passed / 41 pre-existing skipped; shell suites (AppShell, NoSidebar, WorkspaceDrawerSectionNav) → **78/78 passed** (`vitest-workspacehome-review.txt`, `vitest-shell-review.txt`).
- **Gates on `66422bd4..a915fcbc`:** `git diff --check` clean; `check-rustfmt-diff.py` changed lines clean; `check-clippy-diff.py` changed lines clean (1145 pre-existing warnings outside changes); all 20 named invariant scripts OK (`gates-review.txt`, `invariant-scripts-review.txt`). Other `check-*.sh` scripts with non-zero exits fail identically at the base commit (pre-existing, advisory — not introduced by this candidate).

## Scope and integrity checks

- **Commit attribution:** exactly 7 commits in range touch the product surface (`crates/`, `web/src/`, `web/tests/`); the frontmatter lists all 7 and `check-task-commit-attribution.sh` passes. The `de90a0a2`/`08361ee0` and `be45c5d9` commits touch only `web/dist` and deleted Python bytecode — outside the product-surface prefixes.
- **Root scope restorations:** `git diff origin/main a915fcbc -- scripts/check-task-commit-attribution.sh specs/tasks/task-{072,077,087,092,106,107}.md` is empty; no `__pycache__` files tracked at the candidate.
- **`web/dist`:** identical to `origin/main` at the candidate — the checkpoint's accidental rebuild was dropped as claimed.
- **No test weakening:** `Sidebar.test.js` (17 cases) tested the deleted `Sidebar.svelte` component — superseded by `ui-navigation.md` (explicitly supersedes HSI §1) with replacement `NoSidebar.test.js` (4 cases, including negative assertions that the sidebar is absent). `WorkspaceSidebarSectionNav.test.js` → `WorkspaceDrawerSectionNav.test.js` rename preserves the scroll-target coverage for Briefing/Specs; the dropped Explorer-expand case tested removed sidebar chrome. `AppShell.test.js`: 23 removed / 10 added — removed cases assert the removed sidebar highlight model; `ws-selector` coverage remains (3 assertions kept); the decisions-badge remains covered by E2E `topbar_renders_with_workspace_selector_search_decisions_avatar` (app.spec.js:42), though the unit-level `99+` cap assertions are now only production code — minor, non-blocking (badge display is unchanged from base; not part of this task's repair).
- **Working tree:** clean at review end (a transient `web/dist` rebuild caused by this review's own `npm ci` was restored; the committed candidate was never modified).

## Residual risk

Full Playwright E2E, full vitest, and full Rust suites cannot run in this sandbox (no loopback listeners, no browser download) — the controller's host/GitHub gates re-run them on the exact merge SHA. Within sandbox scope: unresolved, none.
