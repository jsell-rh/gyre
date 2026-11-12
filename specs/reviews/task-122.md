# Review — task-122 (Enhance team management with lead, slug, and integration points)

Spec: `specs/system/user-management.md` §Team Management.
Candidate: `c1520e19468e57ccbb55b10c1ba30d60f3ef6618` (base `199c1bb78f53f27088c54a0929b5605e7302e874`).
Product commits: `bbd72bc37` (implementation), `abaee5ea1` (unused-import cleanup); merge `f0b06a8cb` brought base task-203 in.
Verdict: **approved** — every acceptance criterion is backed by real production code and hard tests; the one open sub-bullet (default reviewers) is honestly documented with a verified spec-gap rationale and is not in the task's acceptance criteria.

## Round 1 (2026-11-13) — independent review

Evidence under `/tmp/stage/review-evidence/task-122/`. The working tree's `specs/tasks/task-122.md` modification is the executor's assigned-contract restoration (see durable finding) — all probes below ran against the candidate source, which is byte-identical to the working tree for every file except that task doc.

### Probes run (all in this sandbox, via the stage build wrapper)

- `cargo test -p gyre-domain` → **478 passed, 0 failed** (full suite; includes 5 team tests: slug validation/derivation incl. non-ASCII and `---`/`!!!` → None cases, membership `added_at`).
- `cargo test -p gyre-adapters` → **475 passed, 0 failed, 6 ignored** (full suite; includes 6 team tests: `find_by_slug` round-trip with workspace scoping, duplicate-slug create/update rejection, duplicate-membership rejection, `added_at` ordering, delete-removes-memberships, and `migration_000062_backfills_team_slug_and_lead` which builds a genuine pre-000062 DB, runs only 000062, and asserts slug←id / lead←Owner backfills, unique-index duplicate rejection, dropped `member_ids`, composite-PK duplicate rejection).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean, no Rust warnings (only build.rs web-asset notes). `--all-targets` and server test *execution* are rejected by the stage wrapper's 4 GiB gate — infrastructure, not a code defect; deferred commands below.
- Mechanical gates, all exit 0 at candidate state: check-arch, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-abac-route-registry, check-abac-exempt-handlers, check-inert-enforcement, check-in-memory-state-stores, check-scope-literal-defaults, check-fabricated-scope-defaults, check-forged-scope-fields, check-dead-message-kinds.
- check-task-commit-attribution: **fails only in the working tree** because the executor's contract restoration zeroed the `commits:` frontmatter. Verified via temporary stash (restored immediately): at the candidate commit both task-labeled product commits (`abaee5ea1`, `bbd72bc37`) are recorded and the gate passes — see `check-task-commit-attribution-candidate-state.txt`.

### Acceptance criteria — verified

- **Team entity fields** — `gyre-domain/src/team.rs` matches the spec struct exactly (id, workspace_id, name, slug, description, lead, created_at); `member_ids` removed from the entity in favor of the spec's membership model.
- **TeamMembership {team_id, user_id, added_at}** — own struct, own `team_memberships` table (composite PK), returned by the new member endpoints.
- **Slug unique within workspace, URL-safe** — enforced at three layers with a race-safe backstop: `validate_slug` (1–64 `[a-z0-9-]`, no edge/consecutive `-`; explicit invalid slugs → 400, never rewritten), API-level `reject_duplicate_slug` → 409, and unique index `idx_teams_workspace_slug` in the migration (adapter contract tests assert the failure both via create and update). Same slug in a different workspace is allowed (tested).
- **Lead references a valid workspace member** — `validate_team_lead` queries real `workspace_memberships`; non-member → 400 on create and update (tested with a tenant user lacking membership). Migration backfills lead ← earliest workspace Owner, `''` only for orphaned rows.
- **Notification routing (`team:slug`)** — `resolve_team_members` (notifications.rs) resolves `team:{slug}` → real member ids via `find_by_slug` + `list_members`; unresolvable refs log and skip, never fabricate recipients. Wired into the `SpecApprovalRequested` emitter in specs_assist save flow. The e2e test's distinguishing recipient is a **Viewer** team member — outside the role-based Admin/Developer/Owner fan-out — so the test fails if team routing is dropped, stubbed, or reverted; the Owner-receipt assertion guards against the whole notification block silently not running.
- **Existing team CRUD endpoints work with new fields** — all four handlers reworked: they now authenticate (`AuthenticatedAgent`; previously unauthenticated), tenant-scope via `load_workspace_scoped`, gate on Owner/Admin/TenantAdmin (`can_manage_membership`), validate name/slug/lead, and per-handler check the team belongs to the path's workspace (cross-workspace team id → 404 — this closes a pre-existing hole where `update`/`delete` ignored the workspace entirely). API tests cover the full flow, 400/409 negative arms, workspace scoping on all four mutating routes, and Developer → 403 with nothing written.
- **Migration adds missing columns** — `2026-11-13-000062_team_slug_lead_membership` is the next unused sequence number (000061 is the prior max); portable SQL for both SQLite and PostgreSQL (portability gate passes); real backfill test on a genuine pre-migration database (see probes).
- **`cargo test --all`** — cannot execute in this sandbox (4 GiB build wrapper). Compilable proxies all green (see probes); deferred to the verifier.

### Integration points

- **Spec ownership (`team:slug` approvers)** — `expand_team_approvers` (api/specs.rs) expands `team:{slug}` manifest entries into the member identifier forms human approvals record, before the pure §9 resolver. The e2e test uses a real git manifest repo, real ledger, team + membership created through the real team API, and real API-key human approvals: a team member's approval approves the spec, a **non-member's approval leaves it Pending**, and an **unresolvable team reference fails closed** (Pending) — both fail-open classes are killed. `spec-registry.md` manifest schema documents the entry form (spec amendment is a one-row documentation of the new entry form, consistent with the implemented behavior).
- **Budget reporting by team** — `GET /api/v1/workspaces/:id/budget/by-team` aggregates the real `BudgetCallRecord` audit trail: agent-caused records attribute to the spawning user (via the agents store), then to every team that user belongs to; no-attribution records land in an explicit `unattributed` bucket rather than vanishing. Handler tenant-scopes (stricter than the sibling `GET .../budget`, which relies on ABAC alone). Test asserts aggregation and the unattributed bucket over real records.
- **Default reviewers — NOT implemented, honestly recorded.** Verified the rationale: no spec models a repo→team ownership association (the `Repository` entity has no team field; grep across `specs/system/` finds no defined association — only the usage bullet itself and escalation-table mentions that equally presuppose such a link). Implementing it would require inventing the ownership model, contradicting goal rule 4. The coverage row documents the open bullet explicitly rather than claiming it — no fake completion. Not in the task's acceptance criteria.

### Test-inflation check

None found. The new tests drive the real router with real auth paths (bearer, API keys), real git repos, the real ledger, and real storage; every negative arm (invalid slug 400, duplicate slug 409, non-member lead/member 400, duplicate membership 409, cross-workspace 404, Developer 403, non-member approval stays Pending, unresolvable team fails closed) asserts on observable state, and the migration test exercises a genuine pre-migration database.

### Scope check

No out-of-scope production edits. Spec changes are limited to the coverage matrix (row 10), the one-row `spec-registry.md` manifest-schema documentation, `docs/api-reference.md` rows for the new/changed endpoints, and the task doc. Frontend has no dependency on the Team shape (the workspace-settings teams tab renders workspace members, not team objects) — no UI change required by this task. `web/dist/` untouched (verifier-owned).

### Non-blocking observations (recorded, not findings)

- `budget_by_team` lists the audit trail through the port's `limit` parameter (100_000 rows). Beyond 100k records in one workspace the report silently truncates — a scale boundary inherent to the port's limit-based listing, not a spec violation at any tested scale.
- `expand_team_approvers` contributes display-name/username/uid identifier forms per member, relying on the pre-existing resolver's identity-form matching; a collision would require two users sharing an identifier the system already treats as the same identity.

## Required verifier commands (deferred from this sandbox)

Server test execution exceeds the stage wrapper's 4 GiB budget. The verifier must run:

- `cargo test --all` (the open acceptance criterion), or at minimum:
- `cargo test -p gyre-server --lib api::users::tests::team` — API flow (slug derivation, lead validation, member add/remove with `added_at`, duplicate slug 409, invalid slug 400, non-member lead/member 400, workspace scoping 404, Developer 403).
- `cargo test -p gyre-server --lib api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e` — team-member approves, non-member and unresolvable-team stay Pending.
- `cargo test -p gyre-server --lib api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers` — Viewer team member receives `SpecApprovalRequested` via `team:platform` routing.
- `cargo test -p gyre-server --lib api::budget::tests::budget_by_team_aggregates_usage_through_membership` — aggregation + unattributed bucket.
- `cargo test -p gyre-server --lib mem::team_contract_tests` — mem adapter duplicate guards.
