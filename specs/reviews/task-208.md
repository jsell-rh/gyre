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

## Round 54 (repair round 54 — candidate `df877ec7`, clobber 48)

Assignment: candidate `df877ec718fb64a23193a253559c9ce9e5bde2c2` on base `a884329d71182b59aba3868a9291f2ee2f4282a0` (task-076, already merged at `5a3c2916f`). All probes at the candidate tip (= HEAD), evidence in `/tmp/stage/review-evidence/r54/`.

Product verification (all green, zero drift since the round-53 verified tip `f6d243dad` — `git diff f6d243dad..HEAD` over every task-208 surface is empty):

- Dead-code grep (`get_my_agents|get_my_tasks|get_my_mrs|myAgents|myTasks|myMrs` over `crates/` + `web/src/`) → zero matches; route and ABAC registrations for `/api/v1/users/me/{agents,tasks,mrs}` absent (both exit 1).
- Router 404 regression test present (`users.rs:954`); `SKIP_WEB_BUILD=1 cargo test -p gyre-server my_stuff_endpoints_are_removed` → **1/1 passed** at this exact source.
- `UserProfile.svelte` tabs array is exactly the six compliant tabs (info, tokens, memberships, ledger, notif-prefs, notifications).
- Spec amendment + coverage matrices in place (`user-management.md` supersession at lines 456/611; coverage row 22 `n/a` with supersession note).
- Fresh `npm ci` + `npx vitest run src/__tests__/UserProfile.test.js` → **23/23 passed**; `scripts/test_dev_attribution.py` → 2/2 OK.

Finding (reproduced on the assigned candidate — 48th occurrence of the rounds 2–53 clobber class):

- [-] The candidate's **only** change vs parent `cfab5570f` rewrites `commits:` from the restored six-SHA list back to five SHAs, dropping `ea006d52bb42e96e6f176b02b5390b6cb56a3116` (`build(web): exempt generated dist bundles from whitespace gate (task-208)` — root `.gitattributes` `web/dist/** -whitespace`, live at `.gitattributes`). Verified: `ea006d52` is an ancestor of HEAD (merge-base --is-ancestor exit 0) and NOT an ancestor of the assigned base (exit 1), so it is branch product surface that becomes invisible to review scoping per the AGENTS.md invariant. The fixed in-repo `scripts/dev-attribution.py` (line 26, `path == '.gitattributes'`) computes exactly the six-SHA list on this branch; the executor-staged `/tmp/stage/dev-attribution.py` still lacks that line (grep exit 1), so each checkpoint pass re-drops the SHA until the staged script advances. The assignment body's `commits:` list carried the same clobbered five-SHA list because it was generated from the clobbered task file; the canonical invariant is AGENTS.md plus the fixed in-repo gate logic.

Verdict: **needs-revision** (process repair only — restore the six-SHA `commits:` list via the fixed in-repo pass; product is clean and needs no change). The verifier must still run `cargo test --all` and `cd web && npm test`. The branch-tip committed dist bundle remains the assignment base's stale pre-task-208 output per the executor restore-to-base flow; final integration rebuilds and publishes dist remotely from the merged `web/src` — not a code defect.
