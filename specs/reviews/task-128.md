# Review — task-128: ABAC policy engine core (entity, conditions, attribute model)

**Candidate:** `7df447e7101a691837b6e0ee2921f0281ad76afa` (product commit `29ad56fb`, merge of base `c5deca801` at `8ca9afafb`)
**Verdict:** Changes requested (3 low-severity findings; core implementation verified real and correct)
**Evidence:** `/tmp/stage/review-evidence/` (review-notes.md, test transcripts, mutation probes)

## What was verified

Against abac-policy-engine.md §Core Concepts–§Policy Examples and the task's
acceptance criteria, with independent probes at the candidate head:

- **Policy entity / enums**: all spec fields including `immutable`,
  `scope_id: Option<Id>`, `created_by: Id`; `PolicyScope`
  {Tenant, Workspace, Repo}; `PolicyEffect` {Allow, Deny}; all 8
  `ConditionOp` variants; `ConditionValue` String/StringList/Number/Bool.
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation
  time, type-preserving, fail-closed on unresolvable references. Verified by
  temporary probe (NotEquals + `$resource.missing` on an Allow → Deny) and by
  the resolution tests (Allow flips to Deny when only the referenced
  attribute changes).
- **Attribute extraction**: real stores throughout — memberships/teams for
  `subject.workspace_role`/`workspace_ids`/`team_ids`, entity lookups for
  `resource.tenant_id`/`owner`/`approval_status` (spec ledger, personas,
  tasks, agents, MRs, teams; repo→workspace→tenant scope chain), budget
  system for `env.budget_remaining`, merge-queue pause state for
  `env.main_health`, socket-peer `env.ip` (CWE-348). Insert-only JWT claim
  merge (claims cannot override extractor identity facts). The live tower
  oneshot middleware tests kill real regressions (e.g. the viewer-membership
  test is seeded against `builtin-developer-write-access` at priority 800, so
  broken membership extraction would flip FORBIDDEN→OK).
- **Port + adapters**: `PolicyRepository` CRUD + scope listing; the built-in
  delete contract is now enforced in code in the SQLite, Postgres AND mem
  adapters (`delete_rejects_built_in_policy` proves rejection + survival).
  No new migration was needed — the String→Id change is Rust-side only and
  wire-compatible (`Id` is a string newtype; frontend `scope_id` usage
  unaffected); migration numbering untouched.
- **Tests**: gyre-server lib 1456/0, domain+common+ports 116+432+8/0,
  adapters lib 382/0 (12 ignored). All repo invariant scripts pass, including
  check-arch, check-abac-route-registry (duplicate count = frozen baseline),
  check-task-commit-attribution, and every security-pattern check.
  Exemption files: only line renumbering vs base; entry counts identical for
  every file — no new exemptions, no baseline raises.
- **Mutation probes**: removing the deny-before-allow tie-break is killed by
  `deny_overrides_allow_at_same_priority_and_scope`. Disabling fail-closed
  dynamic-reference resolution is NOT killed by any test (finding 2), though
  the production behavior is correct.

## Findings (all low severity)

1. **code** — `abac_middleware.rs` `extract_path_attributes`: on
   repos-parented child routes with non-repo resource types
   (`/api/v1/repos/:id/gates|worktrees|abac-policy|post-merge-gates`),
   `resource.id` is set to the parent repo id. The doc comment and the
   workspace-prefix analog (`resource_type != "workspace"` suppresses
   resource.id) say `:id` there names the parent; the repos prefix lacks the
   equivalent `resource_type != "repo"` guard. Policies conditioning on
   `resource.id` for those resource types evaluate against the repo id, and
   `/repos/:id/worktrees/:worktree_id` never surfaces the worktree id.
2. **test** — `dynamic_reference_unresolvable_fails_closed` cannot kill a
   fail-open regression: with `resolve_dynamic_value` mutated to fabricate
   `""` for unresolvable references, the entire 1456-test suite still passes
   (Contains yields Deny via both condition-failure and default deny).
   Production behavior verified correct by probe; add a killing case (e.g.
   `subject.type NotEquals $resource.missing` on an Allow must Deny).
3. **process** — coverage rows 2–7 of
   `specs/coverage/system/abac-policy-engine.md` remain `task-assigned` with
   empty notes; per the task-105 convention the implementer flips its
   sections to `implemented` with evidence notes. Under-claiming, not fake
   completion.

## Deferred to verification

`cargo test --all` (integration binaries) and workspace-wide `cargo check
--all-targets` (the executor wrapper rejects workspace-wide invocations in
this sandbox); GitHub CI. No web/ changes in the diff, so the frontend suite
is unaffected.
