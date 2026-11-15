# Review — task-114 (Public user profile page `/@{username}`)

Spec: `specs/system/user-management.md` §User Profile Page (`/@{username}`) (rescoped 2026-09-30: public profile only; no My Dashboard scope).
Candidate: `6b55ce614065889af9a856ad50601dd15c951ef8` (base `706bc7bd2250aac1768a39687a468355ff8afda7`).
Product commits: `fc06e371` (feat), `978faf1a` / `d813d79b` (tests), `b25f1060` (gate repair) — all recorded in the task frontmatter; verified ancestors of HEAD.
Verdict: **approved** (Round 1). Evidence: `/tmp/stage/review-evidence/`.

## Round 1 — 2026-11-15

Probes run (all exit-coded, logs in `/tmp/stage/review-evidence/`):

- `bash scripts/check-abac-route-registry.sh` → OK (both new routes resolved in the ABAC registry, "user" resource, in-handler containment).
- `bash scripts/check-arch.sh` → OK. `bash scripts/check-mem-port-contracts.sh` → OK.
- `cd web && npm ci` → OK; `npx vitest run src/__tests__/PublicUserProfile.test.js src/__tests__/AppShell.test.js` → **85/85 passed**.
- **Mutation probe (test-inflation check):** disabling `notFound = true` and the team-rendering branch in `PublicUserProfile.svelte` → 2/8 component tests fail (`mutation-probe.log`); component restored. The tests kill real bugs, they are not self-confirming.
- `cargo test -p gyre-adapters --lib list_by` → **27 passed**, including the three new adapter tests (filter + newest-first ordering for `list_by_reviewer` / `list_by_approver` / `list_by_spawner`).
- `cargo check -p gyre-server --lib --tests` → exit 0 (also proves build.rs builds the current web source).

Verified working:

- **Backend is real, not stubbed.** Three port methods (`ReviewRepository::list_by_reviewer`, `SpecApprovalRepository::list_by_approver`, `AgentRepository::list_by_spawner`) implemented in SQLite, PostgreSQL, mem, and the search-sync indexing wrappers with real SQL/queries; agents queries are tenant-filtered at the adapter (the `agents` table carries `tenant_id`); SQLite adapter tests prove filtering and ordering. Routes registered in `api/mod.rs`; ABAC resolver mappings added (registry gate passes).
- **Tenant containment is shared and enforced.** All three handlers resolve the subject through `resolve_public_user` (username lookup + tenant match, cross-tenant = 404 indistinguishable from nonexistent; system principal exempt as provisioner). The mem-backed handler test drives the real router and asserts cross-tenant 404 on both `/activity` and `/stats`, plus stats exclusions (another reviewer's rows excluded, rejected approval not counted), newest-first ordering, pagination with `total`, and a same-tenant peer reading the feed. Deferred to the verifier per the 4 GiB budget (list in `/tmp/stage/review-evidence/verifier-tests.md`); code-level inspection confirms the assertions bind to the production paths.
- **Reviews attribute exactly** (reviewer identity = globally-unique user id, enforced at submission in `merge_requests.rs`); spec approvals and spawns match the identity forms the write paths record (`user:{display_name}`/`user:{username}`, spawned principal id), deduped by row id — the same identity-form model the base already uses in `specs.rs` `expand_team_approvers`.
- **Teams do not leak rosters:** drawn only from **accepted** workspace memberships via `TeamRepository::list_for_user`; `ProfileTeamResponse` carries no member list.
- **Frontend is complete:** `/@{username}` route in `parseUrl`/`urlFor` (URL-encoded handles, history state, popstate restore, title, breadcrumb with back arrow), `PublicUserProfile.svelte` with identity header (avatar/initial, display name, handle, timezone, joined), stats bar, memberships with role badges, teams, activity feed with entity navigation via `goToEntityDetail`, 404 empty state that does not probe stats/activity, responsive breakpoints (720px/420px). Avatar-menu "Public profile" link to own handle; `goToUserProfile` context exposed for @mention/activity-feed integration (no @mention surface exists yet — context + route are the integration point, as the task file states). HSI §12 NOT-list guarded by a test ("does not render private /profile surfaces"). e2e locator tightening (`/^Profile$/`) is correct — the menu now also offers "Public profile".
- **Docs/coverage updated faithfully:** api-reference rows for the two new endpoints, ui.md component row, coverage row 23 → implemented with accurate evidence, SUMMARY recount consistent.
- **Attribution hygiene:** all four task-labeled product/test commits recorded in `commits:` frontmatter; no unattributed task-114 commits in the base..candidate range; no scripts/exemption files touched.

Non-blocking observations (recorded for future hardening, none violate the task contract):

- **Identity-form attribution inherits the platform's legacy ledger model.** `spec_approvals` has no tenant/user-id column and records `user:{display_name}`; `display_name` is user-editable and not unique. Colliding display names (within or across tenants) can therefore mis-attribute ledger rows on the public feed/stats — e.g. a tenant-1 user who sets their display name equal to a tenant-2 user's can surface that user's `spec_approved`/`spec_rejected`/`spec_revoked` events (spec path, sha, reasons). This ambiguity is pre-existing and systemic (the base's `expand_team_approvers` matches the same forms for gate satisfaction, with the same property), and subject containment itself is enforced; exact attribution is impossible without ledger schema evolution (record user id/tenant on approval). Suggested follow-up: ledger records the resolving user id, or display-name uniqueness is enforced — a platform-model change, out of this task's scope.
- **`mem.rs` `list_by_approver` reverses `HashMap` iteration order** under a comment claiming "insertion order" (which `HashMap` does not provide); the SQLite equivalent orders by UUIDv4 `id desc`, which is not chronological either. No behavioral impact — the handler sorts merged events by timestamp and stats only count — but the port doc's "newest first" is loose for this store in every adapter. A one-line `sort` by a real key (or a doc fix) would tidy this.
- **AppShell routing tests mirror `parseUrl`/`urlFor`** rather than importing them (the file's pre-existing convention), so the new App.svelte integration branches (breadcrumb, avatar-menu item, mode render) have no direct component-level test; they are covered indirectly by the component test, the e2e locator update, and the deferred server tests.
- `cargo check -p gyre-server` triggers build.rs, which regenerated `web/dist` as a side effect; review restored the tree to the committed bundle (dist is verifier-owned in this checkpoint).

Test-inflation check: none found. Component tests proven mutation-killing; adapter tests assert filter + ordering against real SQLite; handler tests drive the real router with cross-tenant 404, exclusion, ordering, and pagination assertions.

Scope check: no out-of-scope edits — no `/profile` (private surface) changes, no scripts/verifier/exemption files, no spec text changes beyond the task's own coverage rows.
