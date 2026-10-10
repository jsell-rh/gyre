# Review — task-208 (Remove My Tasks/MRs/Agents from profile; amend user-management "My Stuff")

Spec: `specs/system/human-system-interface.md` §12 "What the Profile Is NOT" (lines 1423-1428).
Commits under review: `98fd096` (product, Part 1 + Part 2), `0d0b956`/`1eaa013`/`183945a`/`3c4afaa` (docs/process).
Verdict: **needs-revision** — one material finding (F1, stale shipped bundle); everything else verified clean.

## Round 1

Probes run (main checkout at HEAD `183945a`; `98fd096` is an ancestor):

- `cargo test -p gyre-server my_stuff_endpoints_are_removed` → **`api::users::tests::my_stuff_endpoints_are_removed ... ok`** (1 passed, lib target).
- `bash scripts/check-abac-route-registry.sh` → OK.
- `bash scripts/check-abac-exempt-handlers.sh` → OK (89 handlers).
- `bash scripts/check-task-commit-attribution.sh` → OK (all task-labeled product commits recorded).
- `cd web && npx vitest run src/__tests__/UserProfile.test.js` → **23/23 passed**.
- Diff inspection of `98fd096` for all ten touched files; grep sweep of `crates/` + `web/src/` for dead references.

Verified working (no findings):

- **Backend removal is complete and guarded.** Handlers `get_my_agents`/`get_my_tasks`/`get_my_mrs` deleted from `users.rs` (module doc header updated), route registrations deleted from `api/mod.rs` (import list pruned), ABAC `RouteResourceMapping` entries deleted from `abac_middleware.rs` (registry gate confirms no dangling routes). The regression test `my_stuff_endpoints_are_removed` (`users.rs` test mod, lines 944-969) drives the real router via `app().oneshot(...)` with a bearer token and asserts 404 on all three URIs — it exercises the actual route table, not a mock, and fails if any route is reintroduced.
- **Frontend source cutover is clean.** `UserProfile.svelte`: the three tabs, tab bodies, state vars, `Promise.allSettled` fetches, `nav()` helper, `entityName` import, and the now-orphaned `.entity-list*` CSS are all gone; six tabs remain (info, tokens, memberships, ledger, notif-prefs, notifications). `api.js` no longer exports `myAgents`/`myTasks`/`myMrs`. Grep of `web/src/` finds the removed identifiers only inside the six-tab guard test itself (intentional).
- **The tab test is a real guard, not self-confirming.** `UserProfile.test.js` "exposes exactly the six permitted tabs" asserts the exact `data-id` list of every `[role="tab"]` — `toEqual(['info','tokens','memberships','ledger','notif-prefs','notifications'])` — so reintroducing any removed tab (or dropping a kept one) fails it.
- **Spec amendment (Part 1) is faithful and precedes the removal in the same commit.** `user-management.md` §"My Stuff" Views now records the supersession (ui-navigation.md §2 landing page; HSI §12 forbids per-user My Tasks/MRs/Agents surfaces; remaining needs mapped to workspace-home Decisions/bell/Briefing), the M22.8 completeness row is struck as Removed, and §UI Pages drops the My Dashboard row with a supersession note. The `/@{username}` section is untouched, as required by task-114's dependency.
- **Coverage matrices updated on both sides**: `user-management.md` row 22 → `n/a` with supersession note; `human-system-interface.md` row 54 → `implemented` with landed evidence; counts (11 n/a / 25 task-assigned; 17/36 with 10 implemented) are internally consistent.
- **i18n is clean.** The removed tabs used hardcoded labels (`Agents${...}` etc.), not i18n keys; `web/src/locales/en.json` `user_profile.tabs.*` contains exactly the six kept tabs' keys, all still referenced.
- **`scripts/e2e-flow.sh`** correctly inverts its my-stuff checks from "fetch the lists" to "assert 404" with a `fail` on any other code.
- Remaining `users/me/*` endpoints (`/me`, `/me/tokens`, `/me/notifications`, `/me/judgments`) keep their per-handler-auth ABAC-exempt model — untouched, as instructed.

Findings:

- [-] **F1 (major): the committed `web/dist/` bundle still ships the removed surface — the HSI §12 violation survives in the Rust-only build path.** `crates/gyre-server/src/spa.rs` `RustEmbed`s `#[folder = "../../web/dist"]`, and `build.rs` skips the npm build when `SKIP_WEB_BUILD=1` — the documented Rust-only build (AGENTS.md quick start: "uses committed web/dist/"). The committed bundle `web/dist/assets/index-fzyK9GaC.js` predates this task (last dist regen: `44a8187`, task-097+095+102) and still contains, verbatim: the API client methods `myAgents:()=>Kt("/users/me/agents"),myTasks:()=>Kt("/users/me/tasks"),myMrs:()=>Kt("/users/me/mrs")`, the tab definitions `{id:"my-agents",label:`Agents${...}`}` / `my-tasks` / `my-mrs`, and the tab-body render branches `e(pe)==="my-agents"?Ye(mr,2):e(pe)==="my-tasks"?Ye(Hs,3):e(pe)==="my-mrs"?...`. So `SKIP_WEB_BUILD=1 cargo build -p gyre-server` produces a binary whose `/profile` still renders the three forbidden tabs (now permanently empty — the endpoints 404, so `Promise.allSettled` rejects and each tab shows its "No agents/tasks/MRs" empty state). The task's dead-code grep AC scoped `web/src/` only, which is why this passed; but the shipped artifact is the real surface, and repo convention is to commit regenerated dist when web sources change (`44a8187 build(web): regenerate dist from merged sources`). The acceptance criterion "no references to myAgents/myTasks/myMrs remain" is not met in the artifact a default `SKIP_WEB_BUILD=1` deployment serves.

  **Repair path (mechanical, owned by task-208's revision round):** `cd web && npm run build`, commit the regenerated `web/dist/` (conventional commit, e.g. `build(web): regenerate dist without my-stuff profile tabs (task-208)`), append that commit's SHA to task-208.md `commits:` frontmatter, and re-run the dist grep — `grep -o "my-agents\|my-tasks\|my-mrs\|users/me/agents\|users/me/tasks\|users/me/mrs" web/dist/assets/*.js` must return nothing. Optional hardening (reviewer's suggestion, not required): a `scripts/check-web-dist-freshness.sh`-style gate or a test asserting the committed dist contains no forbidden identifiers, so the next source cutover can't ship stale again.

Minor (non-blocking, recorded for completeness):

- `specs/milestones/m22-platform-entities.md` lines 72-74 still list the three endpoints. That file is a historical milestone record (M22 shipped them at the time), not a living spec; the governing spec (`user-management.md`) was amended. No change required, but a one-line annotation pointing at the removal would prevent future confusion if anyone mines M22 for the current API surface.
- `docs/api-reference.md` rows for the endpoints were correctly deleted in the same commit.

Test-inflation check: none found. The new backend test drives the real router; the frontend tab guard asserts an exact id list; both fail on reintroduction. The e2e inversion (`fail` unless 404) is a genuine behavioral check.

Scope check: no out-of-scope edits — `/@{username}`, `?owner=me`, judgment ledger, tokens, notification preferences, and memberships code paths are untouched by the diff.

## Round 2 (2026-10-07) — F1 repair verified, verdict complete

Revision commits: `ad1121c` (dist regen), `6bc4189` (M22 annotation + docs), `604e75b`/`0149588` (process). Probes at HEAD `0149588`:

- **F1 fixed.** `web/dist/index.html` now references `index-KSqzVjd3.js` (+ `index-DGYWpzy3.css`); the old `index-fzyK9GaC.js` and old CSS are deleted. F1 acceptance grep over `web/dist/assets/*.js` for `my-agents|my-tasks|my-mrs|myAgents|myTasks|myMrs|users/me/agents|users/me/tasks|users/me/mrs` → **zero matches**. The compiled bundle's tab array is exactly `{info, tokens, memberships, ledger, notif-prefs, notifications}` and its `Promise.allSettled` holds only the four kept fetches (`me`, `myNotifications`, `myJudgments`, `workspaces`) — the six-tab UI ships on the Rust-only `SKIP_WEB_BUILD=1` path. Kept-surface identifiers present in the bundle (`users/me/tokens` ×3, `users/me/judgments`, `notif-prefs` ×2). No source drift since `98fd096` (`git diff 98fd096..HEAD --stat -- web/src/ crates/ specs/system/user-management.md` empty).
- **M22 annotation landed** (`6bc4189`): the three endpoint rows in `specs/milestones/m22-platform-entities.md` are struck with a removal pointer at HSI §12 / task-208; the historical "12 REST endpoints" count preserved as an M22-era record — exactly the optional treatment round 1 suggested.
- **Scoping gap found and fixed this round (F2, process):** `ad1121c` was missing from the task's `commits:` frontmatter. The attribution gate passed only because its product-surface regex `^(crates/|web/src|web/tests)` does not cover `web/dist/` — a task-labeled dist-only commit escapes both the gate and review scoping, even though `web/dist` is the shipped artifact on the Rust-only build path. Round 1's repair path explicitly required recording the SHA; the implementer's `6bc4189` ("record dist regen commit") only touched the M22 milestone. Fixed here: `ad1121c5093cd2f1f2b3824ed88352ac4e181fd9` appended to the frontmatter; exemption file untouched (3 frozen entries, none referencing task-208). Residual: the gate's `web/dist/` blind spot is a process-script gap out of this task's product scope — flagged for a process-task owner (regex extension `web/dist/`, plus deciding whether dist-regen commits must always be task-attributed).
- Frontmatter `progress: complete`; `## Shipped` section added to the task file per the merge-description contract.

Verdict: **complete**. Every acceptance criterion holds; both rounds' findings are closed with code evidence.

## Round 14 (2026-10-10, repair round 13 successor) — attribution clobber 17, product green

Assigned candidate `d8b14cf2` (base `e77537fa`). The tip commit itself performed the 17th checkpoint-attribution clobber: it rewrote `commits:` from the fully-attributed three-SHA list (present at parent `bf03932f`, the clobber-16 repair) back to `["98fd096e..."]`, dropping `ad1121c5093cd2f1f2b3824ed88352ac4e181fd9` (build(web): regenerate dist without my-stuff profile tabs — the F1 fix; task-labeled, verified ancestor) and `ea006d52bb42e96e6f176b02b5390b6cb56a3116` (build(web): exempt generated dist bundles from whitespace gate; task-labeled, verified ancestor). Same documented class and gate blind spot (`scripts/check-task-commit-attribution.sh:114` product-surface regex excludes `web/dist/` and root `.gitattributes`; in-repo gate exit 0 at the candidate, re-verified). Repair is mechanical: restore the three-SHA list at the tip.

Product surfaces re-verified green at this exact candidate with fresh probes:

- **Rust:** `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib api::users::tests::my_stuff_endpoints_are_removed` → 1 passed (404 on all three URIs through the real router).
- **Web:** `npm ci` then `npx vitest run src/__tests__/UserProfile.test.js` → 23/23, including the exact-six-tab guard (`toEqual(['info','tokens','memberships','ledger','notif-prefs','notifications'])` — full-list equality, fails on any reintroduced tab).
- **Greps:** dead-code (`get_my_agents|get_my_tasks|get_my_mrs|myAgents|myTasks|myMrs` over `crates/` + `web/src/`) → zero; route/ABAC registrations → zero; residual `users/me/{agents,tasks,mrs}` strings in `crates/` are only the regression test's own URI array (`users.rs:950-952`); committed dist (`index-KSqzVjd3.js` via `index.html`) → zero forbidden identifiers, kept surfaces present (`users/me/tokens` ×3, `notif-prefs` ×2, `users/me/judgments` ×1). An independent rebuild (unintentionally triggered via `build.rs` during a `cargo test` without `SKIP_WEB_BUILD`) also produced a bundle with zero forbidden identifiers — recorded as evidence, working tree then restored to the committed dist (`git status` clean).
- **Gates:** `check-abac-route-registry.sh` exit 0, `check-abac-exempt-handlers.sh` exit 0 (89 handlers), `git diff --check` clean over the base→candidate range (with the `.gitattributes` `web/dist/** -whitespace` exemption), attribution gate exit 0.
- **No product drift since `ad1121c5`** on any task-208 surface (users.rs, api/mod.rs, abac_middleware.rs, api.js, UserProfile.svelte, UserProfile.test.js, web/dist); the only `web/src` drift on the branch is other tasks' merges (task-196/210, sidebar removal), none touching task-208 surfaces.
- **Spec/coverage:** user-management.md amendment intact (§"My Stuff" supersession, §UI Pages strike, M22.8 Removed row at line 652); coverage user-management row 22 `n/a`, HSI row 54 `implemented`, 0 `not-started` rows in both governed specs; M22 milestone rows annotated.
- **i18n:** `user_profile.tabs.*` holds exactly the kept tabs' keys; removed tabs used hardcoded labels, not i18n keys. No orphans.

Sandbox note: TCP accept is blocked (`EOPNOTSUPP`), so live HTTP probes are impossible here — same as every prior round; the in-process router test covers the 404 contract without sockets.

Verdict: **needs-revision** (attribution only). All acceptance criteria hold on product code; the finding is the frontmatter clobber, mechanical repair as in rounds 3–13.
