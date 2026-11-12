# Review — task-122 (Enhance team management with lead, slug, and integration points)

Spec: `specs/system/user-management.md` §Team Management.
Base: `199c1bb78` — candidate: `fea1a77e9` (production history: `bbd72bc37`, `abaee5ea1` cleanup, `a2b86d834` rustfmt repair; remaining commits are pipeline bookkeeping plus the base merge `f0b06a8cb`).
Verdict: **approved** — every acceptance criterion is backed by real production code; all evidence gatherable in this sandbox passed. The `cargo test --all` criterion is deferred to the independent verifier by the stage build wrapper (gyre-server codegen exceeds the 4 GiB sandbox budget); the exact commands are recorded below and in `specs/tasks/task-122.md`.

## Probes run (independent, this sandbox)

- `cargo test -p gyre-domain team` → **5/5 passed** (slug validation accept/reject, slug derivation, construction carries slug+lead, membership carries `added_at`).
- `cargo test -p gyre-adapters team` → **6/6 passed**, including `sqlite::tests::migration_000062_backfills_team_slug_and_lead` (real pre-000062 SQLite DB → 000062 alone: slug←id and lead←workspace-Owner backfills verified, unique index rejects duplicate (workspace, slug), `member_ids` dropped, composite PK rejects duplicate membership) and the SQLite team repository contract tests (slug round-trip, duplicate-slug create/update rejection, duplicate-membership rejection, `added_at` ordering, delete-removes-memberships).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → clean (lib). `--all-targets` is wrapper-banned in review stage; the implementer's prior `--all-targets` check is on record.
- Mechanical gates, all exit 0: check-arch, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-abac-route-registry, check-abac-exempt-handlers, check-task-commit-attribution, check-forged-scope-fields, check-scope-literal-defaults, check-fabricated-scope-defaults, check-relative-path-defaults, check-byte-slice-truncation, check-lossy-secret-conversion, check-inert-enforcement, check-forwarded-header-trust, check-in-memory-state-stores, check-dead-message-kinds, check-unbounded-external-http, check-fail-open-ref-resolution, and `check-rustfmt-diff.py 199c1bb78` ("changed lines clean, 15 Rust files").
- Evidence: `/tmp/stage/review-evidence/task-122-review/`.

## Verified against the acceptance criteria

- **Entity fields**: `Team` (gyre-domain/src/team.rs) carries id, workspace_id, name, slug, description, lead, created_at — exactly the spec struct. `TeamMembership { team_id, user_id, added_at }` replaces the old `member_ids` JSON id list, persisted in a new `team_memberships` table (composite PK, `added_at` per row).
- **Slug unique within workspace, URL-safe**: `validate_slug` (1–64 chars `[a-z0-9-]`, no edge/consecutive `-`; explicit violations → 400, never silently rewritten), `sanitize_slug` derivation (400 when nothing URL-safe remains), API-level `reject_duplicate_slug` → 409, and a race-safe backstop: unique index `idx_teams_workspace_slug` in SQLite/PG, in-code guards in the mem adapter (mem-contract gate passes; I ran the SQLite arms and the migration's duplicate-index arm myself).
- **Lead references a valid workspace member**: `validate_team_lead` does a real `workspace_memberships.find_by_user_and_workspace` lookup on create and update; non-member → 400 (tested both arms).
- **team:slug notification routing**: `resolve_team_members` (notifications.rs) resolves `team:{slug}` via `find_by_slug` + `list_members`; unresolvable refs log-and-skip, never fabricate recipients. Wired into the `SpecApprovalRequested` emitter in the specs_assist save flow (manifest-named team members are appended to the role-based fan-out, deduped). The test's distinguishing recipient is a **Viewer** team member — outside the role fan-out — so it fails if team routing is dropped or stubbed.
- **Spec ownership (`team:slug` approvers)**: `expand_team_approvers` runs inside `resolve_new_approval_status` before the pure §9 resolver (both production call sites are real approval/revoke handlers, not dead code). The E2E test uses a real git manifest repo, the real team API, and real API-key human approvals, with fail-closed negative arms (non-member approval stays Pending; unresolvable team reference stays Pending even for the lead). `spec-registry.md` manifest schema documents the entry form (a spec amendment, per goal rule 4).
- **Budget reporting by team**: `GET /api/v1/workspaces/:id/budget/by-team` (ABAC-registered, tenant-scoped) aggregates real `BudgetCallRecord` rows: agent-caused records attribute to the spawning user, then to every team that user belongs to; records with no attributable user/team land in an explicit `unattributed` bucket rather than vanishing (tested with exact token/cost assertions).
- **CRUD + membership endpoints**: team routes now authenticate, tenant-scope via `load_workspace_scoped`, gate on Owner/Admin/TenantAdmin (`can_manage_membership`), and enforce workspace ownership of the path's team id (cross-workspace team id → 404 — a real fix; `update`/`delete` previously ignored the workspace entirely). New `POST .../teams/:team_id/members` (workspace-member requirement → 400, duplicate → 409, returns `added_at`) and `DELETE .../members/:user_id` (idempotent 204) are registered in api/mod.rs with ABAC resolver entries.
- **Migration**: `2026-11-13-000062` uses the next free sequence number, portable SQL (portability gate passes), backfills slug←team id and lead←workspace Owner, drops the never-writable `member_ids` column (no membership endpoints existed pre-task-122, so it was always `'[]'` — verified claim), and has a real pre-migration regression test (I ran it).

## Scope note: default reviewers (not a finding)

The spec bullet "Default reviewers (MRs in repos owned by team X auto-assign team X)" is **not implemented**, and this is the correct call: no spec models a repo→team ownership association (verified — `Repository` has no team field; no spec in `specs/system/` defines repo-team ownership), the task's acceptance criteria do not include it, and the omission is transparently recorded in both the coverage row and the task file rather than papered over. Implementing it would have required inventing an unspecced association (goal rule 4 violation). It remains open for a future task that first models the ownership link.

## Non-blocking observations (recorded for the verifier/future work)

- `budget_by_team` reads the audit trail via the port's `list_by_workspace(ws, 0, 100_000)` — the port requires a `limit`, so workspaces with >100k call records would under-report (oldest rows truncated; the code documents the cap). Not blocking: the port offers no unbounded listing, and the cap is explicit.
- Per-record `agents.find_by_id` in the aggregation loop is N+1 — a performance note only.
- `PUT` with only `name` changed keeps the existing slug (partial-update semantics, documented in api-reference.md) — deliberate and reasonable.
- No frontend changes: the spec section is backend-only and no acceptance criterion touches the UI; authored web source is unchanged in this diff.

## Verifier must run (deferred by the stage build wrapper — infrastructure, not a code defect)

- `cargo test -p gyre-server --lib api::users::tests::team` — API flow (slug derivation, lead validation, member add/remove with `added_at`, duplicate slug 409, invalid slug 400, non-member lead/member 400, workspace scoping 404, Developer 403).
- `cargo test -p gyre-server --lib api::specs::tests::team_slug_human_approvers_resolve_through_membership_e2e` — team member approves; non-member and unresolvable-team arms stay Pending.
- `cargo test -p gyre-server --lib api::specs_assist::tests::save_spec_notifies_team_members_from_manifest_approvers` — Viewer team member receives `SpecApprovalRequested` via `team:platform` routing.
- `cargo test -p gyre-server --lib api::budget::tests::budget_by_team_aggregates_usage_through_membership` — aggregation + unattributed bucket.
- `cargo test -p gyre-server --lib mem::team_contract_tests` — mem adapter duplicate guards.
- Full `cargo test --all` (CI) — the task's final acceptance criterion.
