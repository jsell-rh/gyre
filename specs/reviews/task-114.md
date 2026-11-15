# Review — task-114 (Public user profile page `/@{username}`)

Spec: `specs/system/user-management.md` §User Profile Page (`/@{username}`).
Candidate: `8174c6bb` (branch `pipeline/task-114/4cacdd869dbd40118b7a19e616fc238c-1`), base `81c24bf0`.
Commits under review: `23ebbb9c`, `b25f1060`, `d813d79b`, `978faf1a`, `fc06e37` (+ rebase/merge `6509b2c1`/`d05ad4c9`/`8174c6bb`, process-only).
Verdict: **needs-revision** — one blocking finding (F1, spec-approval attribution broken in production); structure, tenant containment, routing, and the other stats are real and verified.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci` → exit 0; `npx vitest run src/__tests__/PublicUserProfile.test.js src/__tests__/AppShell.test.js` → **85/85 passed** (component tests mock `api.js` — they prove rendering/routing, not backend attribution).
- `cargo test -p gyre-adapters --lib list_by` → **27 passed** (includes the three new adapter tests proving filter + newest-first ordering on SQLite).
- `cargo check -p gyre-server --lib --tests` → exit 0 (6m cold).
- Static writer/reader analysis for spec-approval attribution (decisive; details below).
- gyre-server test execution deferred to the independent verifier per the 4 GiB build-budget rule; required probe list in `/tmp/stage/review-evidence/probes.md` §7.

## Verified working (no findings)

- **Route + tenant containment is real.** `/api/v1/users/:username/activity` and `/stats` are registered GET-only in `api/mod.rs`, mapped to the "user" ABAC resource in `abac_middleware.rs`, and share `resolve_public_user` with the pre-existing profile endpoint: username lookup + explicit tenant comparison, 404 for cross-tenant handles indistinguishable from nonexistent. The cross-tenant 404 case is asserted for both new endpoints.
- **MR-review attribution is correct.** Human reviews are recorded under the caller's canonical user id (merge_requests.rs:1100-1146 rejects other identities), and the reader queries `list_by_reviewer(user.id.as_str())` — exact match with the writer. The test's control row (another reviewer) is correctly excluded.
- **Agent-spawn attribution matches its writer.** Orchestrator spawn records `spawned_by = auth.agent_id`, which is the display name for user principals (auth.rs:1625/1868/2034); `spawner_identity_forms` covers display name + username (rename tolerance).
- **Adapters are real queries.** SQLite and PostgreSQL implement `list_by_reviewer`/`list_by_approver`/`list_by_spawner` with actual filters and ordering; mem and search-sync indexing wrappers implement them too. SQLite adapter tests prove filter + newest-first ordering (27 passed).
- **Teams do not leak pending invitations.** The profile's `teams` are drawn only from accepted workspace memberships via `TeamRepository::list_for_user`, with no member roster in the public response.
- **Frontend route is complete.** `parseUrl`/`urlFor` handle `/@{username}` (URL-decoded handle), history state + popstate restore the username, title/breadcrumb set, avatar-menu "Public profile" link gated on the caller's own username, `goToUserProfile` context exposed for @mention surfaces, 404 renders an empty state without probing stats/activity, responsive breakpoints present. AppShell routing tests (77) and the HSI §12 NOT-list guard pass.
- **HSI §12 scope respected.** Nothing was added to the private `/profile`; `my_stuff_endpoints_are_removed` still passes as part of the suite.

## Findings

- [F1] **(major, blocking) Spec-approval attribution identity-key mismatch — `specs_approved` and spec feed events are dead in production.** `crates/gyre-server/src/api/users.rs:379` `approval_identity_forms` queries only `user:{display_name}` and `user:{username}`. The sole production writer of durable `SpecApproval` rows for human approvals — `approve_spec`, `crates/gyre-server/src/api/specs.rs:647-656` — records the **raw canonical user Id** (`auth.user_id.as_str()`), with an explicit comment that the `user:<display name>` label is kept off the ledger precisely because the mismatch "made every human approval invisible to the judgment ledger". The write form is pinned by the existing regression at `specs.rs:3337` ("ledger approver_id must be the canonical user Id"). The reader never queries `user.id.to_string()`, so the key sets are disjoint: for every real human spec approval, `GET /users/{username}/stats` reports `specs_approved: 0` and `/activity` omits the `spec_approved`/`spec_rejected`/`spec_revoked` events. The specced "Stats: ... specs approved" and the spec entries of the "Recent activity feed (public actions only)" are therefore not implemented in effect.

  The candidate's test `public_profile_activity_and_stats` (`users.rs:6331-6343`) seeds the ledger with `format!("user:{}", owner.display_name)` — a form no production path writes for humans — so it is mirrored-logic: it passes while the production behavior is broken. Reproduce: change the seed at users.rs:6335 to `owner.id.to_string()` (the production form) and the `specs_approved == 1` / feed assertions fail.

  **Repair:** include `user.id.to_string()` in `approval_identity_forms` (the reader already holds the resolved `User`), keeping the name forms only as legacy fallback for rows predating the task-193 writer fix; re-seed the test via the production identity (ideally driving the real `approve_spec` path as `specs.rs:3290-3338` does). Full evidence: `/tmp/stage/review-evidence/f1-spec-approval-identity-mismatch.md`.

Minor (non-blocking, recorded for completeness):

- `MemSpecApprovalRepository::list_by_approver` (`mem.rs:3813`) claims "insertion order: newest first" but the store is a `HashMap` — iteration order is arbitrary, so the mem adapter does not satisfy the port doc's "newest first" contract. Mitigated: the handler re-sorts by timestamp, and SQLite/PG order in SQL. Fix by sorting on `approved_at`/insertion sequence like the sibling mem methods.
- Display-name/username-form matching (`spawner_identity_forms`, legacy approval forms) can cross-attribute between users who share a display name. Within-tenant only; prefer canonical-id attribution wherever the writer records it.
- The activity/stats handlers load all of a user's reviews/approvals/spawns and paginate in memory. Acceptable at current scale; fine to leave.

## Generated-output observation (verifier-owned, not a finding)

`web/dist` at the candidate is the assignment-base bundle retained through the rebase merge (no dist committed this round). Per stage rules the deterministic verifier rebuilds and binds publication to the tested tree; the authored frontend source and tests were reviewed instead.

## Verifier must run (4 GiB wrapper)

1. `cargo test -p gyre-server --lib api::users::tests` (after F1 repair; re-run with the production ledger identity seeded).
2. Workspace `cargo clippy --all-targets` diff gate vs base `81c24bf0`.
3. Repo-wide `cargo test --all` and `npm test` (task DoD).

Test-inflation check: one instance found (F1 — the spec-approval seed mirrors the reader's wrong assumption instead of the writer's recorded key). The MR-review, ordering, pagination, containment, and frontend assertions are genuine and fail on the corresponding regressions.
