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
