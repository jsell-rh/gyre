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


## Candidate round (2026-10-10) — candidate `f1323df6` vs base `6bf777a6`, verdict approved

Independent review of the assigned candidate. Probes (evidence: `/tmp/stage/review-evidence/task-208-candidate-*.txt`):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib my_stuff_endpoints_are_removed` → **ok (1 passed, 0 failed; 1196 filtered)** against the real router at `f1323df6` — asserts 404 on all three URIs.
- `cd web && npm ci` → **added 169 packages, exit 0** (locked versions); `npx vitest run src/__tests__/UserProfile.test.js` → **23/23 passed** including the exact-six-tab guard (`toEqual(['info','tokens','memberships','ledger','notif-prefs','notifications'])`).
- Dead-code grep (`get_my_agents|get_my_tasks|get_my_mrs|myAgents|myTasks|myMrs`) over `crates/` + `web/src/` → **zero matches** (tool grep; the only URI strings in `crates/` are the regression test's own assertion list at `users.rs:950-952`).
- Forbidden-identifier grep (`my-agents|my-tasks|my-mrs|myAgents|myTasks|myMrs|users/me/agents|users/me/tasks|users/me/mrs`) over the shipped `web/dist/` → **zero matches**; `dist/index.html` references `index-KSqzVjd3.js`/`index-DGYWpzy3.css`; kept-surface identifiers present in the bundle (`users/me/tokens` ×3, `notif-prefs` ×2, `users/me/judgments` ×1). The F1 fix (regenerated dist) survives at the candidate.
- `bash scripts/check-abac-route-registry.sh` → OK; `bash scripts/check-abac-exempt-handlers.sh` → OK (89 handlers).
- `bash scripts/check-task-commit-attribution.sh` → **OK (exit 0)** at the candidate — the final bookkeeping commit `f1323df6` dropped the dist-regen entry `ad1121c5` from `commits:` (the twelfth occurrence of the documented `dev-attribution.py` `web/dist/` blind spot), but `98fd096e` is still recorded, so the gate passes and review scoping is complete for product surface. The dist-regen commit is reachable from the assigned base→candidate range and was verified directly this round. Residual process-owner item unchanged from prior rounds; not a product defect.
- Product-surface drift check: `git diff ad1121c5..f1323df6` on all task-208 surfaces (users.rs, api/mod.rs, abac_middleware.rs, api.js, UserProfile.svelte, UserProfile.test.js, web/dist/, user-management spec+coverage, api-reference.md, e2e-flow.sh) is empty except `docs/api-reference.md` (task-200 message-bus rows, out of scope) and `specs/coverage/system/human-system-interface.md` (2026-10-09 navigation-binding correction, out of scope). Zero drift on task-208's own surfaces.
- Spec amendment intact at the candidate: §"My Stuff" Views records the supersession (ui-navigation §2 landing page; HSI §12 forbids per-user My Tasks/MRs/Agents; remaining needs mapped to Decisions/bell/Briefing); the M22.8 completeness row is struck **Removed**; §UI Pages drops the My Dashboard row with a supersession note; `/@{username}` untouched. Coverage: user-management row 22 → `n/a` with note; HSI row 54 → `implemented` with landed evidence.
- Remaining `users/me/*` routes at the candidate are exactly the kept set (`/me`, `/me/tokens(+/…:id)`, `/me/notification-preferences`, `/me/judgments`, `/me/notifications(+/count)`) with their per-handler-auth model untouched — do-not-touch constraints honored.
- `bash -n scripts/e2e-flow.sh` → syntax OK; its my-stuff checks invert to assert HTTP 404 with `fail` on any other code.

Sandbox limitation (recorded, not a code defect): TCP `accept` is blocked (EOPNOTSUPP, errno 95; `/tmp/stage/capabilities.json`), so no live HTTP 404 probe against a running server is possible here. Host verification / GitHub CI should run the live checks: start `gyre-server` (SKIP_WEB_BUILD=1 or full build) and assert `curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer <token>" http://localhost:3000/api/v1/users/me/{agents,tasks,mrs}` returns 404 for each, and `scripts/e2e-flow.sh` passes its three "My … surface removed: HTTP 404" assertions. The in-process router test exercises the identical route table the live server would use.

Test-inflation check: none. The backend test drives the real router via `app().oneshot` with a bearer token; the frontend guard asserts the exact tab-id list; both fail on reintroduction of any removed surface.

Verdict: **approved**. Every acceptance criterion holds at `f1323df6` with fresh independent evidence; the frontmatter `ad1121c5` drop is the known automation blind spot, recorded above, and does not affect product correctness or review scoping of the product commit (`98fd096e`, recorded).
