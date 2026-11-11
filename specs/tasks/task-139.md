---
title: "Implement forge enforcement policies for spec-to-code binding at merge"
spec_ref: "agent-gates.md §Forge Enforcement Policies"
depends_on:
  - task-138
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Forge Enforcement Policies"
  - "agent-gates.md §Integration with Agent Gates"
commits: ["99edd0bcaddd740a5b560e61ef2937b156d780fb"]
---

## Spec Excerpt

From `agent-gates.md` §Forge Enforcement Policies:

| Policy | Effect |
|---|---|
| `require_spec_ref` | MRs must include a spec_ref. MRs without one are rejected. |
| `require_approved_spec` | The referenced spec SHA must have an approval. Unapproved specs block merge. |
| `warn_stale_spec` | If spec HEAD has moved past referenced SHA, warn but allow. |
| `require_current_spec` | The referenced spec SHA must match spec HEAD. Forces re-review if spec changed. |
| `require_signed_approval` | Spec approvals must include Sigstore signature. |

Policies are configurable per repository.

From §Integration with Agent Gates:

The `spec-alignment-review` gate type (using Accountability agent persona) is designed for this:
1. Gate agent receives MR diff AND spec at pinned SHA
2. Agent compares: does code actually implement what spec describes?
3. Agent checks: are there spec requirements that code doesn't address?
4. Agent submits Approved or ChangesRequested with specific findings

## Implementation Plan

1. **Spec enforcement policy configuration:**
   - Add `spec_enforcement_policy` to repo settings (stored in DB or repo config)
   - Fields: require_spec_ref (bool), require_approved_spec (bool), warn_stale_spec (bool), require_current_spec (bool), require_signed_approval (bool)
   - Default: all false (opt-in per repo)

2. **Add `spec_ref` field to MR model:**
   - Extend MergeRequest entity with `spec_ref: Option<String>` (format: `"path@sha"`)
   - Migration to add column to merge_requests table
   - MR creation and update accept spec_ref

3. **Merge-time enforcement checks:**
   - In merge processor, before executing merge:
   - `require_spec_ref`: reject if MR has no spec_ref
   - `require_approved_spec`: look up spec_ref SHA in spec_approvals ledger, reject if no Approved entry
   - `warn_stale_spec`: compare spec_ref SHA to current spec HEAD SHA, emit warning if different
   - `require_current_spec`: reject if spec_ref SHA ≠ current spec HEAD SHA
   - `require_signed_approval`: reject if approval lacks signature field

4. **Enforcement result reporting:**
   - On policy violation, return clear error to merge processor
   - Error propagated to MR status / author notification
   - Warning-only policies (warn_stale_spec) add advisory comment but don't block

5. **Spec-alignment gate integration:**
   - When AgentReview gate with accountability persona is configured:
   - Gate agent receives spec content at the pinned SHA (not HEAD)
   - This ensures review is against the version the code was written for

6. **API: repo spec policy endpoints:**
   - Check if `GET/PUT /api/v1/repos/:id/spec-policy` already exists
   - Extend to include new enforcement policy fields
   - Or create new endpoint if not present

## Acceptance Criteria

- [ ] 5 enforcement policies configurable per repo
- [ ] `spec_ref` field on MergeRequest entity (path@sha format)
- [ ] `require_spec_ref`: MRs without spec_ref rejected at merge
- [ ] `require_approved_spec`: unapproved spec SHAs block merge
- [ ] `warn_stale_spec`: warning when spec HEAD ≠ referenced SHA
- [ ] `require_current_spec`: rejection when spec HEAD ≠ referenced SHA
- [ ] `require_signed_approval`: rejection when approval lacks signature
- [ ] Enforcement errors reported clearly to author
- [ ] Spec-alignment gate receives spec at pinned SHA, not HEAD
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` §Forge Enforcement Policies and §Integration with Agent Gates. Existing spec policy: check `gyre-server/src/api/spec_policy.rs` and routes at `GET/PUT /api/v1/repos/:id/spec-policy` (registered in `gyre-server/src/api/mod.rs` at line ~171). MR model: `gyre-domain/src/merge_request.rs` or equivalent. Merge processor: grep for `merge_processor\|process_merge` in `gyre-server/src/`. Spec approvals: depends on task-138's ledger. Git SHA comparison: need to resolve current HEAD SHA for a spec file path in the repo. Check migration numbering: currently at 000049.

## Shipped

All five forge enforcement policies now run at merge time with real ledger
and git lookups, violations reach the MR author directly, and the
spec-alignment gate receives the spec at the pinned SHA. Four of the five
policies (`require_spec_ref`, `require_approved_spec`, `warn_stale_spec`,
`require_current_spec`) and the `spec_ref` MR field already shipped with the
task-138 lineage; this assignment adds the missing pieces with production
code, not bookkeeping.

### require_signed_approval (the missing 5th policy)

- `SpecPolicy.require_signed_approval` (`gyre-domain/src/spec_policy.rs`),
  persisted via migration `2026-11-09-000059_require_signed_approval`
  (portable `ALTER TABLE ... ADD COLUMN ... INTEGER NOT NULL DEFAULT 0`) and
  implemented in the SQLite and Postgres adapters + mem store.
- `verify_signed_spec_ref` (`api/gates.rs`): same exact-(path, sha) ledger
  query as `verify_spec_ref`, but the active approval must additionally
  carry a non-empty signature; one signed active approval among several
  rows for the version satisfies the policy, whitespace-only signatures do
  not.
- Merge processor enforces it before executing the merge; an MR with no
  spec_ref is also rejected under this policy (no spec_ref → no referenced
  approval, signed or otherwise).
- `GET/PUT /api/v1/repos/:id/spec-policy` round-trips the field (Admin-only
  PUT, defaults false — opt-in per repo).

### Enforcement errors reported to the author

- Every blocking spec-policy rejection now fails the queue entry with the
  specific reason AND emits a `GateFailure` workspace event AND delivers an
  inbox notification to the author's spawning user
  (`notifications::notify_spec_policy_violation`, same delivery rule as
  `notify_gate_failure`).
- `warn_stale_spec` keeps the existing `StaleSpecWarning` event and now also
  records a deduplicated advisory MR comment (`forge:spec-policy` author)
  visible via `GET /api/v1/merge-requests/:id/comments`, without blocking.

### Spec-alignment gate receives spec at pinned SHA

- New `GET /api/v1/merge-requests/:id/spec` serves the spec content at the
  MR's pinned `spec_ref` SHA via `git cat-file blob` (SHA validated 40-hex
  before reaching the command line). 404 when the MR has no spec_ref or the
  pinned blob is unreachable. Route registered in `api/mod.rs` with an ABAC
  `RouteResourceMapping` (resource `merge_request`).
- `gate_executor` passes `GYRE_SPEC_URL` to AgentReview and AgentValidation
  gate agent processes (documented in `docs/agent-protocol.md`), so review
  runs against the version the code was written for — not HEAD.
- Web: RepoSettings policy form now binds the real API field names
  (`require_approved_spec`, `warn_stale_spec` — the previous
  `require_approval`/`stale_spec_warning` bindings were dead) and adds
  `require_current_spec` + `require_signed_approval` toggles, so all five
  policies are configurable from the UI as well as the API.

### Test evidence

Focused suites distinguishing real behavior (this sandbox, base
22f275e26a320cc8ab0f27f016688a138e62be8f, commit
99edd0bcaddd740a5b560e61ef2937b156d780fb):

- `signed_spec_ref_requires_signature_on_active_approval` (api::gates):
  approved+signed verifies; approved-unsigned and blank-signature fail
  naming the missing signature; no-approval fails; one signed row among
  several satisfies.
- `require_signed_approval_blocks_unsigned_spec_approval`
  (merge_processor): unsigned approval → entry Failed with reason naming the
  signature, author notification delivered (asserted via
  `NotificationRepository::list_for_user`); control MR with a signed
  approval merges (Merged).
- `warn_stale_spec_adds_advisory_comment_without_blocking`
  (merge_processor, real git repo with HEAD moved past the referenced
  blob): merge proceeds (Merged) and the advisory comment lands on the MR.
- `get_mr_spec_serves_pinned_sha_not_head` (api::merge_requests, real git
  repo with spec v1 committed then v2 at HEAD): endpoint returns v1 content
  and the pinned SHA; no-spec_ref and unreachable-SHA MRs 404.
- `set_require_signed_approval_round_trips` (api::spec_policy) and
  `set_and_get_round_trips_all_five_policies` (sqlite adapter, fresh DB →
  proves migration 000059 runs).
- Regression control: `require_current_spec_blocks_stale_spec_ref`,
  `revoked_approval_no_longer_verifies_spec_ref`,
  `find_by_spec_sha_scopes_to_exact_version` still pass.

Suite results: `cargo test -p gyre-server --lib` 1531/1531; `-p gyre-domain`
442/442; `-p gyre-adapters --lib` 405/405 (incl. all migrations on fresh
SQLite DBs); integration `git_integration` 17/17, `api_integration` 66/66,
`graph_integration` 38/38; frontend `vitest run
src/__tests__/RepoSettings.test.js` 54/54. Mechanical invariants pass:
arch, migration-versions, migration-SQL-portability, ABAC route registry
(+ new route registered), ABAC exempt handlers, mem-port-contracts,
inert-enforcement, dead-message-kinds, api-auth, and the other check-*
scripts with output identical to the pristine baseline. Full workspace
`cargo test --all`, all-target Clippy, and GitHub CI on the exact head
remain with verification/publication.

### Rebase rounds

- **Rebase onto main head bd49c7847** (merge commit 98b0bfa32c): single
  textual conflict in `docs/api-reference.md` — kept the task-139
  spec-policy row (with `require_signed_approval` and the advisory MR
  comment) and retained main's new
  `/api/v1/repos/{id}/settings/spec-lifecycle` row. Post-merge re-runs on
  that head: `cargo test -p gyre-server --lib` 1664/1664; `gyre-domain`
  426 passed + 6 ignored; `gyre-adapters --lib` 448/448; integration
  `git_integration` 17/17, `api_integration` 66/66, `graph_integration`
  38/38; frontend `vitest run src/__tests__/RepoSettings.test.js` 58/58;
  all check-* mechanical invariants pass.

- **Rebase onto main head cfeefbda9** (task-091 lineage, this round): two
  content conflicts, both resolved by combining both sides' features
  rather than picking one:
  - `gate_executor.rs`: main's rework moved review-agent context into
    `build_review_agent_context`/`GateReviewContext` (`ctx.spec_ref`,
    `ctx.spec_content` at the pinned SHA). Resolution keeps main's
    ctx-based access and this task's `GYRE_SPEC_URL` env delivery, with
    the pinned-spec URL now derived from `ctx.spec_ref`.
  - `notifications.rs`: main added `notify_gate_passed` (with per-(user,
    MR) de-duplication) at the same site where this task added
    `notify_spec_policy_violation`. Resolution keeps both functions.
    `notify_spec_policy_violation` was adapted to main's richer
    `notify_rich` signature (repo_id + entity_type "gate") and now takes
    a real `tenant_id` parameter instead of the previous literal
    `"default"` — `report_spec_policy_violation` (merge_processor)
    resolves it from the workspace record and skips + logs when the
    workspace cannot be resolved
    (check-fabricated-scope-defaults.sh posture).
  - Post-merge verification on this head (sandbox, see
    review-evidence): `cargo check -p gyre-server` and `cargo check -p
    gyre-server --tests` clean; `cargo test -p gyre-domain` 463+8
    passed; `cargo test -p gyre-adapters --lib` 443 passed + 6 ignored
    (migrations incl. `2026-11-09-000059_require_signed_approval` run on
    fresh SQLite DBs); frontend `vitest run
    src/__tests__/RepoSettings.test.js` 58/58; all 20 check-*
    mechanical invariants pass (incl. migration-version uniqueness:
    main's date-prefixed `2026-10-09-000059/000060` and
    `2026-11-08-000059` do not collide with
    `2026-11-09-000059_require_signed_approval`).

  The sandbox build wrapper enforces the 4 GiB budget by refusing
  gyre-server test compilation, so `cargo test -p gyre-server --lib`
  and the integration suites must run in the independent verifier:
  merge_processor::{require_current_spec_blocks_stale_spec_ref,
  require_signed_approval_blocks_unsigned_spec_approval,
  warn_stale_spec_adds_advisory_comment_without_blocking},
  api::gates::{signed_spec_ref_requires_signature_on_active_approval,
  revoked_approval_no_longer_verifies_spec_ref},
  api::merge_requests::get_mr_spec_serves_pinned_sha_not_head,
  api::spec_policy::set_require_signed_approval_round_trips,
  notifications::gate_passed dedup tests, and gate_executor review
  context (pinned-SHA delivery) tests, plus git_integration /
  api_integration / graph_integration.
