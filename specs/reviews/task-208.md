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

## Round 3 (2026-10-10) — candidate 9a29df8d, base 73a31e0b; verdict needs-revision (one bookkeeping finding)

Independent re-review of the exact candidate (`9a29df8d`, tip after the whitespace-repair round). Probes (full command/output: `/tmp/stage/review-evidence/task-208-candidate-9a29df8d.txt`):

- `cargo test -p gyre-server --lib api::users::tests::my_stuff_endpoints_are_removed` → **ok** (1 passed, 1196 filtered). The test drives the real `api_router()` via `app().oneshot()` with a bearer token and asserts 404 on all three URIs.
- `cd web && npm ci` (locked) then `npx vitest run src/__tests__/UserProfile.test.js` → **23/23 passed**, including the exact-six-tab guard (`toEqual(['info','tokens','memberships','ledger','notif-prefs','notifications'])` over `[role="tab"]` data-ids).
- Acceptance greps: `get_my_agents|get_my_tasks|get_my_mrs|myAgents|myTasks|myMrs` over `crates/`+`web/src/` → zero (identifiers survive only as the 404-test URIs and the guard test's forbidden strings); forbidden-surface strings in the shipped bundle `index-KSqzVjd3.js` → zero, with kept surfaces present (`users/me/tokens` ×3, `users/me/judgments`, `notif-prefs` ×2).
- `bash scripts/check-abac-route-registry.sh` / `check-abac-exempt-handlers.sh` / `check-task-commit-attribution.sh` → all OK; ABAC registry no longer maps the three routes (base mapped them at `abac_middleware.rs:468-470`).
- `git diff --check 73a31e0b 9a29df8d` → exit 0. The `.gitattributes` `web/dist/** -whitespace` exemption is legitimate: the flagged byte is a literal TAB inside a minified string literal (od dump: backtick, space, TAB, LF — a Svelte class-list separator constant), and base main's own bundles carry the identical byte class (`elk.bundled-BcuvlO8W.js` 3 lines, `index-fzyK9GaC.js` 1 line) — they pass only because they predate the diff range. No hand-written path is exempted.
- Task-surface drift since the dist regen `ad1121c5`: zero on `UserProfile.svelte`, `api.js`, `UserProfile.test.js`; only an unrelated `en.json` `rules_reconciling` string from the upstream merge. The bundle's sidebar-era staleness on other surfaces is base-inherited (task-210's source merge postdates both base and the regen; both base and candidate bundles ship identical sidebar-era bytes) — not a task-208 regression.
- Spec amendment, coverage matrices (user-management row 22 → `n/a`; HSI row 54 → `implemented`), M22 strike-through annotation, `docs/api-reference.md` deletions, and the `e2e-flow.sh` 404 inversion all verified intact at the candidate.

Findings:

- [-] **F3 (process/bookkeeping): the candidate tip re-dropped the dist-regen and gitattributes commits from the task's `commits:` frontmatter — 13th clobber of the documented automation class.** `9b8f922c` correctly recorded `["98fd096e", "ad1121c5", "ea006d52"]`, but the final commit `9a29df8d` ("process: record task-208 branch commits") reset the list to `["98fd096e"]` only. `ad1121c5` is the F1 fix (task-labeled, product surface: `web/dist/`), `ea006d52` is the whitespace repair (task-labeled, root `.gitattributes`) — both are now invisible to review scoping. The in-repo attribution gate passes only because its product-surface regex `^(crates/|web/src|web/tests)` excludes `web/dist/` and root files; per the AGENTS.md attribution invariant an unlisted commit is silently outside every future review round's scope. This is the exact residual the implementer's own history records (checkpoint pass rebuilds `commits:` from a regex that excludes `web/dist/`). Repair is mechanical: restore the three-entry list at the tip; no product files involved.

Test-inflation check: none. The router 404 test exercises the real route table; the tab guard asserts an exact id list; the e2e 404 check `fail`s on any non-404. Scope check: clean — no out-of-surface edits (`/​@{username}`, judgment ledger, tokens, notification preferences, memberships, `?owner=me` untouched; canonical HSI/ui-navigation specs untouched).

Verdict: **needs-revision** — all product/behavioral acceptance criteria hold at the candidate, but the tip's frontmatter state fails the attribution contract; F3 is the only finding.
