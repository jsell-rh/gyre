# Review — task-122 (Enhance team management with lead, slug, and integration points)

Spec: `specs/system/user-management.md` §Team Management.
Candidate under review: `747819d0` (base `44a01390`; product commits `bbd72bc`, `abaee5ea`, `a2b86d8`, `3056a7e`, `9d520866` — all recorded in the task frontmatter; remaining branch commits touch only task notes).
Verdict: **approved** — no blocking findings. Non-blocking observations recorded below.

## Round 1

### Probes run (independent, in this sandbox via `/tmp/stage/bin/cargo`; logs under `/tmp/stage/review-evidence/`)

- `cargo check -p gyre-server` → clean (7m21s, EXIT=0).
- `cargo check -p gyre-server --tests` → clean (3m09s, EXIT=0) — server test code compiles, including the merge-repaired `prompt_template_sha: None` init in the by-team budget test (the E0063 from verification round `2681105b`).
- `cargo test -p gyre-domain` → 478 passed + 8 doctests, EXIT=0.
- `cargo test -p gyre-adapters team` → 6 passed (team repository contract tests + migration 000062 backfill regression against a real pre-000062 SQLite database), EXIT=0.
- `scripts/check-arch.sh` → OK (domain boundaries intact).
- `scripts/check-abac-route-registry.sh` → OK (new team member routes + budget/by-team registered; no duplicates).
- `scripts/check-abac-exempt-handlers.sh` → OK (75 handlers).
- `scripts/check-migration-versions.sh` → OK (000062 is the next unused sequence).
- `scripts/check-migration-sql-portability.sh` → OK (no dialect-only SQL in 000062).
- `scripts/check-mem-port-contracts.sh` → OK (mem adapter enforces the port's duplicate contracts in code).
- `scripts/check-in-memory-state-stores.sh`, `scripts/check-forged-scope-fields.sh`, `scripts/check-conditional-test-guards.sh`, `scripts/check-aspirational-test-names.sh` → OK.
- `scripts/check-assertionless-tests.sh` → EXIT=2, **environmental**: the sandbox has `mawk` 1.3.4, the script requires gawk's 3-arg `match()` (probe saved: `awk-probe.sh`). Not a candidate defect; the new tests' assertions were verified by direct reading (all are assertion-dense).
- Full diff inspection `44a01390..747819d0` (21 files, +2861/−137); grep sweep for stale `member_ids` references.

### Verified working (no findings)

- **Entity matches the spec exactly.** `Team` (gyre-domain/src/team.rs): id, workspace_id, name, slug, description, lead, created_at. `TeamMembership { team_id, user_id, added_at }` persisted in a `team_memberships` table (composite PK) — the superseded never-writable `member_ids` JSON column is dropped. Domain tests cover slug validation/derivation and membership fields.
- **Slug: unique within workspace, URL-safe.** `validate_slug` (1–64 chars `[a-z0-9-]`, no edge/consecutive `-`) rejects explicit bad slugs with 400 — never silently rewritten; `sanitize_slug` derives from the name and returns `None` (→ 400, require explicit slug) when nothing URL-safe remains. Uniqueness is enforced three ways: handler pre-check (409), `idx_teams_workspace_slug` unique index (SQLite + PG, race-safe), and mem-adapter code guard — with stable error labels across adapters. SQLite tests prove create/update collisions fail and cross-workspace reuse is allowed.
- **Lead references a valid workspace member.** `validate_team_lead` checks the real `workspace_memberships` store; non-member lead → 400 on create and update (tested, including a tenant user with no membership).
- **CRUD endpoints work with the new fields — and gained real authorization.** The pre-task handlers had **no auth extractor at all** and ignored the path workspace on update/delete; now all team handlers authenticate, tenant-scope via `load_workspace_scoped`, gate on Owner/Admin/TenantAdmin (`can_manage_membership`, stored-role authoritative), and 404 a team id from another workspace (cross-workspace write fixed; tested for PUT/DELETE/members routes). New `POST .../teams/:team_id/members` (workspace-membership required — "teams never expand access" — duplicate → 409, returns `added_at`) and `DELETE .../members/:user_id` (idempotent 204). ABAC resolver entries added for all new routes.
- **Notification routing (`team:slug`) is real.** `resolve_team_members` (notifications.rs) resolves `team:{slug}` through `find_by_slug` + `list_members` against real storage; unresolvable references log and skip — no fabricated recipients. Wired into the `SpecApprovalRequested` emitter (specs_assist save flow). The e2e test's distinguishing recipient is a **Viewer** team member — outside the Admin/Developer/Owner role fan-out — so the test fails if team routing is removed, reverted, or stubbed.
- **Spec ownership (`team:slug` approvers) is real and fail-closed.** `expand_team_approvers` (api/specs.rs) expands manifest `human_approvers` `team:{slug}` entries into the member identifier forms (`user:{display_name}`, `user:{username}`, `user:{uid}`) before the pure §9 resolver runs. An unresolvable team (or memberless team) is kept **verbatim** — correct and subtle: dropping it would leave an empty `human_approvers` list, which the resolver treats as "any human may approve" (fail-open). The e2e test drives a real git manifest repo, real ledger, real team API, real API-key human approvals, with negative arms that kill the fail-open classes: a non-member's approval leaves the spec Pending; an unresolvable team reference stays Pending even for the lead.
- **Budget reporting by team is real.** `GET /workspaces/:id/budget/by-team` aggregates real `BudgetCallRecord` audit rows: agent-caused records attribute to the agent's `spawned_by` user, then to every team that user belongs to; records with no attributable user land in an explicit `unattributed` bucket (never dropped). Handler authorization matches `load_workspace_scoped` semantics exactly (tenant match + system-principal carve-out), consistent with sibling budget endpoints. Test distinguishes team-attributed, teamless-user, and agentless records.
- **Migration 000062 is correct and proven.** Next unused sequence number; portable SQL (portability script passes); backfills slug ← team id (stable, unique, URL-safe uuid hex) and lead ← workspace Owner; unique index; `team_memberships` table. The regression test builds a genuine pre-000062 database (all prior migrations applied, legacy rows inserted), applies 000062 alone, and asserts the backfill, the unique index, the dropped column, and the composite PK — it passed in this sandbox.
- **All three adapters enforce the port contracts** (mem in code with matching error labels; SQLite/PG via indexes) — `check-mem-port-contracts.sh` passes and the mem contract test mirrors the SQLite suite.
- **No test inflation found.** Tests drive the real router with real git repositories, real ledgers, and real team storage; negative arms (403 Developer, 400 non-member lead/member, 409 duplicate slug/membership, 404 cross-workspace, Pending fail-closed approvals) kill the failure classes they name.
- **Commit attribution** is complete: exactly the five product commits are in the frontmatter; every other task-122-labeled commit touches only `specs/tasks/task-122.md`.
- **Spec handling is faithful.** `spec-registry.md` documents the `team:{slug}` approvers semantics (elaboration consistent with user-management.md's spec-ownership bullet); `docs/api-reference.md` rows updated; coverage row 10 marks the section implemented with an explicit in-row disclosure of what is NOT implemented.

### Non-blocking observations (recorded, not findings)

- **Default reviewers is not implemented — transparently documented, and correctly so.** The spec bullet ("MRs in repos owned by team X auto-assign team X for review") presupposes a repo→team ownership association that no spec models: the `Repository` entity has no team field and the concept appears nowhere else in the spec corpus (grep-verified). Implementing it would require inventing an unspecced data model (rule 4 forbids implementing around a spec by reinterpretation). It is not in this task's acceptance criteria, and the omission is disclosed in the coverage row and task notes rather than faked. A future spec must model repo→team ownership before this bullet can close.
- **`budget_by_team` scans the audit trail with `limit = 100_000`** (newest first). A workspace with more than 100k budget call records would silently exclude the oldest records from team aggregation. Documented in a comment; consistent with the port's limit-based API and far beyond typical volumes, but a future aggregate-in-SQL port method would remove the bound.
- **The team lead is not automatically a team member.** Notification routing and spec-ownership expansion cover members only; a lead who is never added as a member receives no team-routed notifications. The spec defines lead and membership as independent, so this is a defensible reading, but operators should add leads as members if they expect them to be routed.
- **Server test execution is deferred to the verifier** (sandbox wrapper blocks gyre-server codegen; by design). Type-checked here via `cargo check --tests`.

### Verifier must execute

- `cargo test -p gyre-server team` — team CRUD/membership/scoping suites (`api::users::tests::{team_crud_slug_lead_and_membership_flow, team_slug_validation_and_uniqueness, team_lead_and_members_must_be_workspace_members, team_routes_are_workspace_scoped, team_update_validates_slug_conflict_and_lead, team_management_requires_owner_admin_or_tenant_admin}`, `mem::team_contract_tests::mem_team_enforces_duplicate_contracts`).
- `cargo test -p gyre-server specs` — including `api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e`.
- `cargo test -p gyre-server specs_assist` — including `api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers`.
- `cargo test -p gyre-server budget` — including `api::budget::tests::budget_by_team_aggregates_usage_through_membership`.
- `cargo test --all` (final gate) and the standard pre-commit/CI script set.
