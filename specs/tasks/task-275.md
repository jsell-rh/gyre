---
title: "Apply operator decision for spec-44383e89f5c1275953582cde: Amend the stale table list to match shipped reality"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Approved operator decision

{
  "id": "3e7bbde727f5dcf55afb0217",
  "parent": "spec-44383e89f5c1275953582cde",
  "question": "Should specs/development/database-migrations.md's 'Which Tables Get tenant_id' list be amended to match the shipped schema (drop projects, move users to tenant-bound), or should the code be reverted to that list?",
  "context": "specs/development/database-migrations.md classifies database tables into 'tenant-scoped' (rows carry a tenant_id column isolating each customer's data) and 'global' (no tenant column). Two entries in that list now contradict the rest of the canonical specs and the production schema. (1) USERS: the spec says users are global with no tenant_id, 'a user can belong to multiple tenants'. But specs/system/user-management.md's User entity carries 'pub tenant_id: Id', migration 2026-10-08-000058 adds 'ALTER TABLE users ADD COLUMN tenant_id TEXT' (indexed by 2026-11-13-000065_user_tenant_scope), and the auth extractor in crates/gyre-server/src/auth.rs rejects API-key logins for users with no tenant binding rather than inventing one \u2014 schema.rs documents the column as 'the nullable tenant scope column' with fail-closed auth. So each user is bound to exactly one tenant; 'users is global' is false. (2) PROJECTS: the spec lists 'projects' as tenant-scoped, but the Project entity was removed entirely \u2014 specs/index.md M33 ('Project entity removed; Workspace becomes the primary grouping entity') and migration 2026-03-24-000013_remove_projects, whose up.sql runs 'DROP TABLE IF EXISTS projects' and makes workspace_id NOT NULL on repositories. The tenant-scoped tables that actually exist all carry tenant_id, which scripts/check-tenant-filter.sh derives mechanically from the migrations (the migrations are ground truth, not a hand-maintained list). The assigned task says 'Never alter its contract', so the implementer cannot fix the stale list themselves; the goal's rule 4 also forbids implementing around a spec. Blindly implementing the old spec would mean deleting users.tenant_id (breaking tenant-bound auth and user-management.md) and resurrecting the projects table (undoing M33 with destructive migrations against deployed databases). Blindly following only the newer specs leaves database-migrations.md \u2014 still a canonical spec \u2014 asserting falsehoods that future audits and implementers will keep tripping over. The conflict is documentary: the code follows the newer canonical specs; the older spec's table list was never updated.",
  "options": [
    {
      "id": "option_a",
      "title": "Amend the stale table list to match shipped reality",
      "summary": "Spec-lifecycle amendment to database-migrations.md only: remove 'projects' from the tenant-scoped list (superseded by M33 workspaces), move 'users' out of the global list into a tenant-bound category (nullable tenant_id column, auth fail-closes on unbound users), and note the supersessions. No code change.",
      "tradeoffs": "Smallest truthful change; code already conforms, so the task unblocks with zero migration risk and no contradiction of user-management.md or M33. Costs: a doc-only commit plus review, and the list remains a hand-maintained snapshot that can drift again as tables are added. Risk is low because the amendment documents what the migrations already enforce.",
      "next_step": "Create a requirements-amendment task with a versioned decision record; after independent review and merge of the amended spec section, resume the database-migrations.md reconciliation task against the corrected contract."
    },
    {
      "id": "option_b",
      "title": "Revert the code to the old spec's classification",
      "summary": "Treat database-migrations.md as authoritative: drop the users.tenant_id column (users become global again) and restore a tenant-scoped projects table alongside workspaces.",
      "tradeoffs": "Preserves the letter of the older spec. But it contradicts newer canonical specs (user-management.md's User entity, specs/index.md M33) and requires destructive migrations against databases that already ran 000013 and 000058 \u2014 rollback data loss and a re-foundation of the M33+ hierarchy model. Auth would lose the per-user tenant binding the fail-closed extractor relies on. Highest-risk option; effectively unwinds two milestones to satisfy one stale list.",
      "next_step": "Create an amendment-plus-implementation task chain: first amend user-management.md and index.md to restore the pre-M33 model, then write the column-drop and table-restore migrations with downgrade protection and BCP snapshots before any deployment."
    },
    {
      "id": "option_c",
      "title": "Replace the hand-maintained list with a migration-derived rule",
      "summary": "Amend the spec so the authoritative tenant-scoped table set is defined as 'the set derived from the migrations' (the same rule scripts/check-tenant-filter.sh already uses), demoting the current listing to an informative snapshot dated to its last verification.",
      "tradeoffs": "Eliminates the whole drift class \u2014 the spec can no longer contradict the schema because it points at the schema's source of truth instead of duplicating it. Costs: a broader editorial change to a canonical spec, a less directly readable section (readers must derive or run the check to see the list), and the global-vs-tenant-scoped policy split (e.g. api_keys, siem_targets 'TBD') is only partly derivable from columns, so some hand-maintained policy text survives anyway.",
      "next_step": "Create a requirements-amendment task that rewrites the section around the derivation rule, keeps the policy-only classifications explicit, and records the decision; after review and merge, resume the reconciliation task against the restructured section."
    }
  ],
  "recommendation": "option_a",
  "selection": {
    "option": "option_a",
    "actor": "jsell",
    "source": "local cockpit",
    "instruction": null,
    "at": 1795101402.6628273
  },
  "sources": [
    "specs/development/database-migrations.md",
    "specs/system/user-management.md",
    "specs/index.md",
    "specs/tasks/spec-44383e89f5c1275953582cde.md",
    "specs/system/spec-lifecycle.md"
  ]
}

Only option `option_a` is authorized. The other alternatives are recorded history, not requirements.

## Selected direction

Amend the stale table list to match shipped reality

Spec-lifecycle amendment to database-migrations.md only: remove 'projects' from the tenant-scoped list (superseded by M33 workspaces), move 'users' out of the global list into a tenant-bound category (nullable tenant_id column, auth fail-closes on unbound users), and note the supersessions. No code change.

Create a requirements-amendment task with a versioned decision record; after independent review and merge of the amended spec section, resume the database-migrations.md reconciliation task against the corrected contract.

## Required behavior

Prepare the task/spec amendment authorized by the recorded choice. This is a
requirements correction, not permission to guess at product behavior. Amend the
canonical spec, related real task bindings (the spec audit is a controller record, not a task file to create), and documentation
so they express the selected direction consistently. Preserve historical intent
in clearly labelled history; never claim the newly selected behavior is already
implemented. Update specs/task-spec-inputs.json if a spec binding changes. Preserve registry
bindings still referenced by historical tasks, including this amendment task.

Do not change product code, tests, scripts, CI, exemptions or verification gates.
The amendment must pass independent review, remote verification and GitHub checks.
Once it merges, the controller will reassess the parent task against the new
requirements and implement any remaining gaps. Keep changes within specs/ and
docs/ (plus this task's own bookkeeping). Do not modify unrelated task contracts.

## Acceptance Criteria

- [ ] Canonical spec and parent task agree with the recorded choice.
- [ ] The parent task is no longer instructed to implement contradictory behavior.
- [ ] The choice, rationale and authority remain recorded in this task document.
- [ ] Existing implementation status is reported honestly; no fabricated coverage.
- [ ] No production code, tests or enforcement have been changed in this amendment.

## Shipped

Specs-only requirements amendment applying operator decision
`3e7bbde727f5dcf55afb0217` (option_a — "Amend the stale table list to match
shipped reality"). No production code, tests, scripts, CI, exemptions, or
verification gates were touched; the change set is confined to `specs/`.

### What changed

- `specs/development/database-migrations.md` — the "Which Tables Get
tenant_id" section is amended to match the shipped schema, with the full
versioned decision record at the head of the section:
  - `projects` removed from the tenant-scoped list, with the M33
    supersession recorded (migration `2026-03-24-000013_remove_projects`
    drops the table and makes `workspace_id` NOT NULL on `repositories`;
    the workspace is the grouping entity per `specs/index.md` M33).
  - `users` moved out of the global list into a new **tenant-bound**
    category: nullable `tenant_id` (migration
    `2026-10-08-000058_user_entity_profile_fields`, indexed by
    `2026-11-13-000065_user_tenant_scope`), exactly one tenant per user
    per `specs/system/user-management.md`, API-key auth fail-closes on
    unbound users.
  - The prior classification is preserved verbatim under a labelled
    "Historical classification (superseded)" heading.
  - Two directly contradicting neighbors in the same spec were aligned
    with the same correction: the Tenant Enforcement example now uses
    `repositories` (a tenant-scoped table that exists) instead of the
    dropped `projects`, and Tenant Resolution's API-key bullet now reads
    "user's tenant binding (fail-closed)" instead of "user's active
    tenant".
- `specs/system/user-management.md` — one cross-reference bullet added to
  "Relationship to Existing Specs" recording that
  database-migrations.md now classifies `users` as tenant-bound per this
  decision. The spec already carried the authoritative single-tenant User
  entity (`pub tenant_id: Id`; cross-tenant sharing explicitly not
  supported) and needed no requirement change.
- `specs/tasks/task-275.md` — this task: the approved decision (choice,
  rationale, authority — operator `jsell`, local cockpit selection,
  2026-11-19) is retained verbatim above; progress set to
  `ready-for-review`.

### Parent task (controller record)

The parent, `spec-44383e89f5c1275953582cde`, is the controller-held
spec-planner task ("Reconcile specs/development/database-migrations.md —
Never alter its contract"), not a repo task file; per this assignment's
contract it was not created or edited here. The contradiction it could not
resolve is closed at its source: the canonical spec it reconciles against
no longer classifies a dropped table as tenant-scoped or a tenant-bound
table as global, so reverting the code to the old list (dropping
`users.tenant_id`, resurrecting `projects`) is no longer spec-conformant
behavior. Once this amendment merges, the controller's spec-planner sweep
detects the changed spec content and regenerates the reconciliation task
against the corrected contract.

### Registry

`specs/task-spec-inputs.json` required no change: no repo task's
`spec_ref` binds to `specs/development/database-migrations.md` or any of
its sections, and the amended section keeps its heading ("Which Tables Get
tenant_id"), so no existing binding was invalidated or retargeted. All
existing bindings are preserved unchanged.

### Verification evidence

- Every schema fact cited by the amendment was verified against the repo
  before writing it:
  - `crates/gyre-adapters/migrations/2026-03-24-000013_remove_projects/up.sql`
    — `DROP TABLE IF EXISTS projects`; recreated `repositories` table has
    `workspace_id TEXT NOT NULL`.
  - `crates/gyre-adapters/migrations/2026-10-08-000058_user_entity_profile_fields/up.sql`
    — `ALTER TABLE users ADD COLUMN tenant_id TEXT;` (nullable, no
    default).
  - `crates/gyre-adapters/migrations/2026-11-13-000065_user_tenant_scope/up.sql`
    — `CREATE INDEX idx_users_tenant ON users (tenant_id);`.
  - `crates/gyre-adapters/src/schema.rs` — `users.tenant_id` documented as
    the nullable tenant binding with fail-closed auth extractor.
  - `crates/gyre-server/src/auth.rs` (API-key path) — rejects users with
    no tenant binding (403 + AuthFailure audit event) instead of
    fabricating a scope.
- `git diff --name-only` — only `specs/development/database-migrations.md`,
  `specs/system/user-management.md`, and `specs/tasks/task-275.md`
  touched, all within the authorized amendment paths. No `docs/` file
  repeats the stale classification (checked; `docs/development.md` only
  links to the spec), so no docs change was needed.
- No test suite was run and none is required: the change is
  documentation-only and touches no compiled or executed surface. The
  remote verifier's GitHub checks run on the merged result as usual.

### Implementation status (honest)

- The amended classification documents behavior that already ships
  (migrations + fail-closed auth); this amendment claims no new
  implementation and changed none.
- The residual `siem_targets` "may be global or per-tenant - TBD" entry is
  unchanged: option_a authorizes correcting the two stale entries only,
  not the broader migration-derived-rule restructuring (that was option_c,
  recorded history, not a requirement).
- The list remains a hand-maintained snapshot that can drift again;
  `scripts/check-tenant-filter.sh` derives the authoritative tenant-scoped
  set from the migrations either way.
