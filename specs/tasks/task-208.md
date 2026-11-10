---
title: "Remove My Tasks/MRs/Agents from profile; amend user-management 'My Stuff'"
spec_ref: "human-system-interface.md §12"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §12 What the Profile Is NOT"
commits: ["e29a1b658732a8faca03070bf5fe0a59a390ea79", "aecd6847da18a86095e7688fe41bb92f4332a63b", "ea006d52bb42e96e6f176b02b5390b6cb56a3116", "ad1121c5093cd2f1f2b3824ed88352ac4e181fd9", "98fd096e3972e7a6d1df78b700b63ea1dee15fe5"]
---

## Spec Excerpt

From `specs/system/human-system-interface.md` §12 (lines 1423-1428):

> ### What the Profile Is NOT
>
> - **Not "My Tasks"** — tasks are agent work units, not human artifacts
> - **Not "My MRs"** — humans don't author MRs; they approve or reject them (that's in the judgment ledger)
> - **Not "My Specs"** — specs owned by the user are discoverable via the Specs view with an `?owner=me` filter (no separate surface needed)
> - **Not "My Agents"** — agents are system machinery; humans interrogate them (via Inbox) but don't manage them

Conflicting text in `specs/system/user-management.md` §"My Stuff" Views → "My Dashboard (Landing Page After Login)" (lines 456-460) specs My Tasks / My MRs / My Agents sections, and the implementation placed exactly those three views inside `/profile` — the surface HSI §12 governs.

**Why HSI §12 wins (evidence):**
- user-management.md is M22-era; HSI is the later spec (milestone "HSI", after M35).
- `ui-navigation.md` (the newest spec) preserves `/profile` explicitly citing "HSI §12" (§7 route table, line 507) and supersedes the landing-page concept entirely: "The workspace home is a **dashboard**, not a sidebar-driven view. It's the landing page after selecting a workspace" (§2, line 74). ui-navigation §10 supersedes `ui-layout.md` §1 "entrypoint flow".
- HSI's position matches the platform vision: humans direct via specs and exercise judgment; they don't author code, MRs, or tasks.

## Implementation Plan

Two parts: spec amendment first (spec lifecycle), then code cutover.

### Part 1 — Amend `specs/system/user-management.md`

1. **§"My Stuff" Views → "My Dashboard (Landing Page After Login)"** (lines 452-463): rewrite the section to record supersession:
   - The My Dashboard landing-page model is **superseded by `ui-navigation.md` §2** — the workspace home is the dashboard/landing page after selecting a workspace. No separate `/dashboard` page exists or will be built.
   - My Tasks / My MRs / My Agents get **no per-user surface anywhere**: per `human-system-interface.md` §12 "What the Profile Is NOT", they must not appear in `/profile`. Humans who need agent/task/MR state use the workspace home, repo tabs, and the judgment ledger. Remove those three rows from the dashboard table.
   - Pending Approvals / My Notifications / Recent Activity: these needs are served by the ui-navigation workspace home (Decisions section, notifications) — record that mapping and remove the standalone dashboard table.
2. **§"Completeness Assessment (M22.8 Baseline)"** (line 628): the row `` `GET /api/v1/users/me/{agents,tasks,mrs}` — "my stuff" | ✅ Implemented `` — strike it or mark it **Removed per HSI §12** (the endpoints are deleted in Part 2).
3. **§"UI Pages"** (line 575): check the table for references to the My Dashboard / my-stuff surfaces and align them with the amendment.
4. Keep §"User Profile Page (`/@{username}`)" as-is — it is a separate public tenant-scoped surface, not governed by HSI §12, and is covered by task-114.

Also update `specs/coverage/system/user-management.md` rows 22 ("My Dashboard (Landing Page After Login)") — reclassify to `n/a` with a note pointing at the amendment (superseded by `ui-navigation.md` §2 + HSI §12) — and row 23 stays `task-assigned` (task-114, /@{username} only).

### Part 2 — Remove the violating surfaces (clean cutover)

All three endpoints verified registered at `crates/gyre-server/src/api/mod.rs:772-774`.

1. **Backend handlers** — `crates/gyre-server/src/api/users.rs`:
   - Delete `get_my_agents` (line 164), `get_my_tasks` (line 186), `get_my_mrs` (line 215) and any private helpers used only by them.
   - Update the module doc comment (lines 4-7) that lists these endpoints.
2. **Routes** — `crates/gyre-server/src/api/mod.rs`:
   - Delete the three `.route(...)` registrations (lines 772-774) and the now-unused imports `get_my_agents, get_my_mrs, get_my_tasks` (line 79).
3. **ABAC mappings** — `crates/gyre-server/src/abac_middleware.rs`:
   - Delete `RouteResourceMapping::api("/api/v1/users/me/agents", "agent", None)`, `.../tasks`, `.../mrs` (lines 452-454).
4. **Frontend API client** — `web/src/lib/api.js`:
   - Delete `myAgents`, `myTasks`, `myMrs` (lines 513-515).
5. **Profile UI** — `web/src/components/UserProfile.svelte`:
   - Remove the three tabs from the `tabs` array (lines 131-133), the three tab bodies (`my-agents` 342-361, `my-tasks` 363-385, `my-mrs` 387-412), the state vars (lines 27-29), the three `api.*` fetches in `Promise.allSettled` (lines 158-160) and their result assignments (lines 170-178).
   - Preserve the remaining tabs: info, tokens, memberships, ledger, notif-prefs, notifications.
6. **Tests** — `web/src/__tests__/UserProfile.test.js`:
   - Remove the `myAgents/myTasks/myMrs` mocks (lines 10-12, 63-65) and any test cases asserting those tabs render. Grep the file first; only remove what exercises the deleted tabs.
   - Grep `crates/` for tests hitting `/users/me/agents|tasks|mrs` and remove those cases too.
7. **i18n** — grep `web/src/lib/i18n*` (or wherever `user_profile.tabs.*` lives) for keys used only by the removed tabs; delete orphaned keys.

## Acceptance Criteria

- [ ] `specs/system/user-management.md` §"My Stuff" Views records the supersession (ui-navigation.md §2 + HSI §12); the My Dashboard landing-page table with My Tasks/MRs/Agents rows is gone; the M22.8 completeness row for `/users/me/{agents,tasks,mrs}` marks the endpoints removed.
- [ ] `specs/coverage/system/user-management.md` row 22 reclassified `n/a` with supersession note.
- [ ] `GET /api/v1/users/me/agents`, `/tasks`, `/mrs` return 404 (routes, handlers, ABAC mappings all deleted — verified by a test or by route-table inspection in the test suite if a route-registry test exists).
- [ ] `UserProfile.svelte` renders only: info, tokens, memberships, ledger, notif-prefs, notifications tabs; no references to `myAgents`/`myTasks`/`myMrs` remain in `web/src`.
- [ ] No dead code: `grep -rn "get_my_agents\|get_my_tasks\|get_my_mrs\|myAgents\|myTasks\|myMrs" crates/ web/src/` returns nothing (excluding unrelated matches).
- [ ] `cargo test --all` passes.
- [ ] `cd web && npm test` passes.

## Agent Instructions

Read `specs/system/human-system-interface.md` §12 ("What the Profile Is NOT", lines 1423-1428) and `specs/system/ui-navigation.md` §2 + §10 for the supersession rationale. The conflict is documented in `specs/coverage/system/human-system-interface.md` row 54.

Do the spec amendment (Part 1) BEFORE the code removal (Part 2) — the spec is the contract; the amendment is what makes the removal legitimate.

Do NOT touch: the `/@{username}` public profile page scope (task-114), the `?owner=me` specs filter (HSI §12 "My Specs" row — separate concern), the judgment ledger, tokens, notification preferences, or memberships functionality.

The `users/me/*` endpoints that remain (`/me`, `/me/tokens`, `/me/notifications`, `/me/judgments`, `/me/notification-preferences` if present) are per-handler-auth ABAC-exempt per HSI §2 amendments — do not change their auth model.

After both parts, run the acceptance-criteria greps yourself and fix any stragglers (docs comments in `users.rs` header, i18n keys, test mocks).

## Shipped

*(Consolidated record — replaces the per-round diagnostic ledger; full round history lives in the pipeline durable findings for this task.)*

### Product surface (verified at HEAD)

- `/profile` renders only the six HSI §12-compliant tabs (info, tokens, memberships, ledger, notif-prefs, notifications); the My Agents / My Tasks / My MRs tabs, tab bodies, state, fetches, and CSS are deleted from `UserProfile.svelte`, and the exact-six-tab guard test (`[role="tab"]` data-id list) fails on any reintroduction.
- `GET /api/v1/users/me/{agents,tasks,mrs}` deleted end-to-end (handlers, routes, ABAC mappings, `api.js` client methods); the in-process router regression test `my_stuff_endpoints_are_removed` asserts 404 on all three URIs. The remaining `users/me/*` endpoints keep their per-handler-auth ABAC-exempt model unchanged.
- `specs/system/user-management.md` §"My Stuff" Views amended to record the supersession (ui-navigation.md §2 workspace home is the landing page; no per-user my-stuff surface anywhere per HSI §12); M22.8 completeness row struck to **Removed**; coverage matrices updated (user-management row 22 → `n/a`, HSI row 54 → `implemented`).
- Committed `web/dist/` regenerated (`ad1121c5`) without the forbidden surface; `.gitattributes` exempts generated dist bundles from the whitespace gate (`ea006d52`).

### Attribution state (round 124)

- **The entire task-208 branch has been absorbed by main.** The assigned base `078ec12a` contains every task-208 product commit (candidate `b476398f` is an ancestor of the base; `git diff 078ec12a..HEAD` is empty), so the retained source checkpoint is fully present at HEAD.
- `commits:` now lists exactly the five task-labeled product-surface commits: `e29a1b65` (verified assets), `aecd6847` (pipeline checkpoint), `ea006d52` (whitespace-gate exemption), `ad1121c5` (dist regeneration), `98fd096e` (spec amendment + code cutover). The two unlabeled `checkpoint: recover interrupted pipeline source` commits (`e7c21825`, `a11cbf7e`) are no longer listed: they were scoped only as branch-only commits, and with the branch merged into main `origin/main..HEAD` is empty — they are part of main's history and were never task-labeled. Computed by the fixed in-repo pass and committed via its `--commit` flow as `d10f09233` (frontmatter-only 1+/1−).
- **Root-cause loop closed:** fix commit `67430e1bb` ("count root .gitattributes as attribution product surface") has landed on origin/main — `origin/main:scripts/dev-attribution.py` now carries the fix (sha256 `c23f2505…`, byte-identical to the in-repo script). Once the controller refreshes its ROOT checkout, the checkpoint pass stops producing the rounds 2–123 clobber.

## Review

*(Repair Round 124 — assignment on branch `pipeline/task-208/2334ae4317974964bc09dc8243936180-1`, category `requirements_changed`: "Implement the current contract using the retained source checkpoint." Assignment base `078ec12a` (task-123 notification entity) was merged by the executor at `5a45c9f41` before assignment start.)*

- **Retained source checkpoint fully present, zero drift.** Candidate `b476398f` is an ancestor of the assigned base and `git diff 078ec12a..HEAD` is empty (evidence `r124/base-tree-diff.log`, 0 lines) — the merge is trivial and every task-208 surface is exactly the merged-main state.
- **Finding reproduced:** the carried `commits:` frontmatter was the recurring clobber variant — six SHAs listing `e29a1b65` but dropping `ea006d52` (task-labeled `.gitattributes` product commit), leaving task-labeled product surface invisible to review scoping while unlisted. Repaired mechanically via `python3 scripts/dev-attribution.py task-208 --commit` → `d10f09233`; the pass is idempotent at the new HEAD (dry-run exit 0, clean tree).
- **Verification this round (fresh probes at the post-repair HEAD, all exit-coded, evidence `/tmp/stage/review-evidence/r124/`):** dead-code grep (`get_my_agents|get_my_tasks|get_my_mrs|myAgents|myTasks|myMrs` over `crates/` + `web/src/`) → zero matches (grep exit 1); the only `/api/v1/users/me/{agents,tasks,mrs}` URI references in `crates/` are inside the 404 regression test itself (`users.rs:1145-1147`); remaining `users/me/*` routes (`/me` get+put, `/me/tokens`, `/me/tokens/:id`, `/me/notification-preferences`, `/me/judgments`, `/me/notifications`, `/me/notifications/count`) keep the per-handler-auth ABAC-exempt model unchanged; exactly six compliant profile tabs in `UserProfile.svelte`; spec amendment and coverage rows in place (user-management row 22 `n/a` with supersession note, row 23 `task-assigned` task-114, HSI row 54 `implemented`); `scripts/test_dev_attribution.py` → 2/2 OK (exit 0); in-repo attribution gate `scripts/check-task-commit-attribution.sh task-208` → exit 0; `npm ci` (locked versions) + `npx vitest run src/__tests__/UserProfile.test.js` from `web/` → 23/23 passed; `SKIP_WEB_BUILD=1 cargo test -p gyre-server my_stuff_endpoints_are_removed` → `api::users::tests::my_stuff_endpoints_are_removed` ok, 1 passed. The verifier still owns `cargo test --all`, `cd web && npm test`, and the full workspace gates.
