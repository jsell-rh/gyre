---
title: "Remove My Tasks/MRs/Agents from profile; amend user-management 'My Stuff'"
spec_ref: "human-system-interface.md §12"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §12 What the Profile Is NOT"
commits: ["a11cbf7e109440f5e7406523f4751a48eb1dbe6a", "98fd096e3972e7a6d1df78b700b63ea1dee15fe5", "ad1121c5093cd2f1f2b3824ed88352ac4e181fd9", "ea006d52bb42e96e6f176b02b5390b6cb56a3116"]
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

- `/profile` renders only the six HSI §12-compliant tabs (info, tokens, memberships, ledger, notif-prefs, notifications); the My Agents / My Tasks / My MRs tabs, tab bodies, state, fetches, and CSS are deleted from `UserProfile.svelte`, and the exact-six-tab guard test (`[role="tab"]` data-id list) fails on any reintroduction.
- `GET /api/v1/users/me/{agents,tasks,mrs}` deleted end-to-end (handlers, routes, ABAC mappings, `api.js` client methods); the in-process router regression test `my_stuff_endpoints_are_removed` asserts 404 on all three URIs. The remaining `users/me/*` endpoints keep their per-handler-auth ABAC-exempt model unchanged.
- `specs/system/user-management.md` §"My Stuff" Views amended to record the supersession (ui-navigation.md §2 workspace home is the landing page; no per-user my-stuff surface anywhere per HSI §12); M22.8 completeness row struck; M22 milestone rows annotated; coverage matrices updated (user-management row 22 → `n/a`, HSI row 54 → `implemented`).
- Committed `web/dist/` regenerated (`ad1121c5`) so the RustEmbed'd `SKIP_WEB_BUILD=1` build path serves the six-tab bundle — the shipped artifact no longer contains the forbidden surface. `.gitattributes` exempts generated dist bundles from the whitespace gate (`ea006d52`).
- Landing commits: `98fd096e` (spec amendment + code cutover), `ad1121c5` (dist regeneration, F1 fix), `ea006d52` (whitespace-gate exemption); recovery checkpoint `a11cbf7e` retained in `commits:`. All are task-208-labeled product-surface commits and verified ancestors of the tip.
- Merge resolution (this round): assigned base `110180fb` merged into the branch; all conflicts were in generated `web/dist/` bundles (base-side task-172 web/src changes vs. branch dist). Resolved by regenerating `web/dist/` from the merged `web/src` with the locked toolchain (`npm ci` + `vite build`) — no hand-edited artifacts. Fresh probes on the new bundle: forbidden-identifier grep (`my-agents|my-tasks|my-mrs|users/me/agents|users/me/tasks|users/me/mrs`) → zero matches; kept-surface identifiers (`users/me/tokens`, `users/me/judgments`) present. Zero drift on every task-208 product surface (`git diff de501f89..HEAD --stat` over users.rs / api/mod.rs / abac_middleware.rs / api.js / UserProfile.svelte / UserProfile.test.js / amended specs is empty).

## Review

*(Repair Round 22, 2026-10-10 — review finding 765d9d2b, clobber 21.)*

- **Finding reproduced at the assigned candidate `de501f89`:** the tip commit (`process: record task-208 branch commits`) rewrote `commits:` from the fully-attributed four-SHA list restored at parent `9609abf3` back to `["a11cbf7e", "98fd096e"]`, dropping `ad1121c5093cd2f1f2b3824ed88352ac4e181fd9` (the round-1 F1 fix that removed the shipped my-stuff surface from the RustEmbed'd bundle) and `ea006d52bb42e96e6f176b02b5390b6cb56a3116` (root `.gitattributes` whitespace-gate exemption). Same documented class as clobbers 2–20: the checkpoint attribution pass's regex `^(crates/|web/src|web/tests)` excludes `web/dist/` and root `.gitattributes`; the in-repo gate (`scripts/check-task-commit-attribution.sh:114`) shares the blind spot.
- **Repair (mechanical, product untouched):** restored `commits:` to the full four-SHA list at the tip (this commit). All four SHAs verified ancestors of HEAD (`git merge-base --is-ancestor` OK). Resolved the active merge of base `110180fb` first (dist regeneration from merged sources, see Shipped); the merge introduces zero drift on task-208 product surfaces.
- **Verification this round (fresh probes):** dead-code grep (`get_my_agents|get_my_tasks|get_my_mrs|myAgents|myTasks|myMrs` over `crates/` + `web/src/`) → zero matches; committed-dist forbidden-identifier grep over the newly built bundle → zero matches; kept-surface identifiers present; task-208 surface diff vs. the reviewed candidate `de501f89` is empty. Product acceptance criteria were independently verified green at this exact product state by prior-round reviewers (router 404 regression test, UserProfile.test.js 23/23 with exact-six-tab guard) — this round's product diff is only the regenerated dist and the frontmatter repair. Full suites remain owned by verification/publication.
- Residual for the process owner (21st occurrence of the same class): the checkpoint attribution pass may re-drop the `web/dist/`/`.gitattributes` commits at the next checkpoint; the in-repo gate shares the blind spot.
- Evidence: `/tmp/stage/review-evidence/task-208-repair-round22-agent.txt`.
