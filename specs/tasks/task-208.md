---
title: "Remove My Tasks/MRs/Agents from profile; amend user-management 'My Stuff'"
spec_ref: "human-system-interface.md §12"
depends_on: []
progress: complete
coverage_sections:
  - "human-system-interface.md §12 What the Profile Is NOT"
commits: ["98fd096e3972e7a6d1df78b700b63ea1dee15fe5", "ad1121c5093cd2f1f2b3824ed88352ac4e181fd9"]
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

## Implementation Notes (2026-10-07)

- Part 1 + Part 2 landed in 98fd096; this round verified every acceptance criterion and added the coverage-matrix record: `human-system-interface.md` row 54 flipped `task-assigned` → `implemented` with landed evidence (was missed by the wip round).
- Backend: `my_stuff_endpoints_are_removed` regression test (users.rs test mod) asserts 404 on all three URIs — passes. `check-abac-route-registry.sh` and `check-abac-exempt-handlers.sh` pass.
- Frontend: UserProfile.test.js 23/23, including the exact-six-tab guard (`[role="tab"]` data-id list — fails if any removed tab is reintroduced). `ExplorerCanvas.test.js` / `ExplorerCanvas-performance.test.js` failed only in full-suite runs racing concurrent cargo builds (100ms timing budgets); 147/147 pass in isolation at HEAD and the same files pass at the base commit — pre-existing load flakes, unrelated to this task.

## Revision Round (2026-10-07, review round 1)

- **F1 (major) fixed:** the committed `web/dist/` bundle still shipped the removed surface (`spa.rs` RustEmbeds it; `SKIP_WEB_BUILD=1` serves it). `cd web && npm run build` regenerated the dist from current sources → commit `ad1121c` (bundle hash `index-KSqzVjd3.js`; deterministic — matches independent build). F1 acceptance grep `grep -o "my-agents\|my-tasks\|my-mrs\|users/me/agents\|users/me/tasks\|users/me/mrs" web/dist/assets/*.js` → no matches; kept-surface check (`users/me/tokens`, `notif-prefs`, `users/me/judgments` present in bundle) → pass. The earlier round's note claiming the stale dist was "out of scope" was wrong — the reviewer is right that the violation survived on the shipped Rust-only build path.
- **Minor (M22 milestone) addressed:** `specs/milestones/m22-platform-entities.md` M22.8 endpoint list now strikes the three `users/me/{agents,tasks,mrs}` rows with a removal annotation pointing at HSI §12 / task-208. Historical count ("12 REST endpoints") left intact as the M22-era record.
- Optional reviewer hardening (dist-freshness gate) deliberately not added: it requires a general source↔bundle staleness oracle the repo doesn't have; the F1 class is now closed for this surface, and the source-level guards (six-tab test, 404 route test) fail on reintroduction.
- Runtime serving check attempted (`SKIP_WEB_BUILD=1` build of `gyre-server`, launched): every TCP `accept` fails with `os error 95` (EOPNOTSUPP) in this sandbox — kernel socket-accept is blocked, so live HTTP probes are impossible here ([INFERENCE]: sandbox restriction, not a server defect; the in-process router test `my_stuff_endpoints_are_removed` still proves the 404 route table without sockets). Embed chain verified at artifact level instead: `spa.rs` has no `debug-embed` (debug reads `web/dist` from disk; release embeds the same committed folder), and the committed folder passes the F1 grep — so the shipped `SKIP_WEB_BUILD`/release artifact serves the new bundle.

## Shipped

- `/profile` renders only the six HSI §12-compliant tabs (info, tokens, memberships, ledger, notif-prefs, notifications); the My Agents / My Tasks / My MRs tabs, tab bodies, state, fetches, and CSS are deleted from `UserProfile.svelte`, and the exact-six-tab guard test fails on any reintroduction.
- `GET /api/v1/users/me/{agents,tasks,mrs}` deleted end-to-end (handlers, routes, ABAC mappings, `api.js` client methods); the in-process router regression test `my_stuff_endpoints_are_removed` asserts 404 on all three URIs.
- `specs/system/user-management.md` §"My Stuff" Views amended to record the supersession (ui-navigation.md §2 workspace home is the landing page; no per-user my-stuff surface anywhere); M22.8 completeness row struck; M22 milestone rows annotated; coverage matrices updated (user-management row 22 → `n/a`, HSI row 54 → `implemented`).
- Committed `web/dist/` regenerated (`ad1121c`) so the RustEmbed'd `SKIP_WEB_BUILD=1` build path serves the six-tab bundle — the shipped artifact no longer contains the forbidden surface.

## Review Round 2 (2026-10-07)

- **F1 verified fixed at artifact level.** `web/dist/index.html` points at `index-KSqzVjd3.js`; the F1 grep over `web/dist/assets/*.js` returns no matches for any forbidden identifier; the compiled tabs array is exactly `{info, tokens, memberships, ledger, notif-prefs, notifications}` with `Promise.allSettled` reduced to the four kept fetches; kept-surface identifiers (`users/me/tokens`, `users/me/judgments`, `notif-prefs`) present; stale `index-fzyK9GaC.js` / old CSS removed. No source drift since `98fd096` (`git diff 98fd096..HEAD --stat -- web/src/ crates/ specs/system/user-management.md` empty).
- **Scoping fix (this round):** `ad1121c` was absent from the task's `commits:` frontmatter — the attribution gate passed only because its product-surface regex (`^(crates/|web/src|web/tests)`) doesn't cover `web/dist/`, so a task-labeled dist-only commit is invisible to both the gate and review scoping. Appended the SHA to the frontmatter (the review round-1 repair path's explicit requirement; the implementer's `6bc4189` claim of "record dist regen commit" only annotated the M22 milestone). Exemption file untouched (still 3 frozen entries, no task-208 lines).
- M22 milestone annotation verified (three rows struck with HSI §12 / task-208 pointer, historical count intact).
- All round-1 probes still green at this HEAD: `my_stuff_endpoints_are_removed` ok, ABAC route-registry + exempt-handlers gates OK, UserProfile.test.js 23/23, attribution gate OK (now including `ad1121c` in frontmatter).
- Verdict: **complete** — every acceptance criterion holds; the only residual (attribution gate's `web/dist/` blind spot) is a process-script gap outside this task's product scope, recorded here for a process-task owner.
