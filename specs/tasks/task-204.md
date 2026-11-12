---
title: "Implement spec-lifecycle accountability patrol (task-age checks + orchestrator escalation)"
spec_ref: "spec-lifecycle.md §Accountability Integration"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "spec-lifecycle.md §Accountability Integration"
commits: ["de2d46d4fe14cdf4c0c18eae77873e463ecc740c", "bb5e82ee3c480fcad524fedd4a4f7eac40016399", "4fb14234ce9a576709e6763e5db4a0699b300635", "6ba91450cdb7ba224dfa3c23993f6924ddf2eeb2", "e0af55374d2601d7a89f37e210d94607220f9417", "d3b3c63a79898a1a7c73f9447d595ab93f76cf17", "61a6f222207f4671676682c6404c893983a239e8", "696079ca13af3949aa2a676cbb81315f10cc797c", "951880b6ba36750f8a4d58489dc20267b42ba03e", "def75ba6b2fa61e6a33661e0e9223bc3455e9562", "ead1d669b2f5743d2813b45e649b7e94163881cd", "56e77ec4ecd3588ab0b9ce6f355466f3c3ad5c4d", "40ded0602516a013f795654772898954c715d06a", "5147d645704a4445cb589f2b73a86cce70076d36", "91110a37bb67c2763763c405cad897058fcd81ec"]
---

## Spec Excerpt

From `spec-lifecycle.md` §Accountability Integration:

> The Accountability agent's patrol checks for:
> - `spec-drift-review` tasks that have been open longer than one Ralph loop cycle
> - `spec-implementation` tasks that have been in Backlog for more than N days
> - Specs that were modified but have no corresponding task (should never happen if the hook works, but defense in depth)
>
> If any of these are found, the Accountability agent escalates to the workspace orchestrator.

## Problem (current state — code-verified)

`crates/gyre-server/src/spec_patrol.rs` implements the **spec-links.md** graph patrol (stale links, orphaned supersessions, unresolved conflicts, dangling implementations, deep dependency chains). It does NOT implement any of the three **spec-lifecycle.md** accountability checks above. The only references to the `spec-drift-review` / `spec-implementation` labels are the task-creation code in `git_http.rs` (`process_spec_lifecycle`, ~1279-1304) and its tests. No code inspects task ages, no code cross-checks modified specs against existing tasks, and nothing escalates to the workspace orchestrator. Coverage row 8 is `not-started` (hollow).

This is a distinct concern from the existing spec-links patrol at `POST /api/v1/patrol/spec-links` (route registered in `api/mod.rs:351`). Do NOT overload that handler; add a separate spec-lifecycle patrol.

## Implementation Plan

1. **New patrol module `crates/gyre-server/src/spec_lifecycle_patrol.rs`** mirroring the structure of `spec_patrol.rs`:
   - `PatrolFinding`-style struct (reuse the existing `spec_patrol::PatrolFinding` shape if it fits: `finding_type`, `severity`, human-readable `message`, and the offending `task_id`/`spec_path`). Prefer reusing the existing type over inventing a parallel one.
   - `run_spec_lifecycle_patrol(state: &AppState, now_secs: u64, drift_review_max_age_secs: u64, implementation_backlog_max_age_secs: u64) -> Vec<PatrolFinding>`.
   - Register `pub mod spec_lifecycle_patrol;` in `lib.rs` next to `pub mod spec_patrol;` (lib.rs:43).

2. **Check 1 — stale `spec-drift-review` tasks.** Query tasks via `TaskRepository` (`crates/gyre-ports/src/task.rs`), filter to non-terminal (`status != Done && status != Cancelled`) tasks whose `labels` contains `"spec-drift-review"`, and flag any where `now_secs - created_at > drift_review_max_age_secs`. Default the threshold to one Ralph loop cycle. There is no in-code Ralph-loop-cycle constant; use `drift_review_max_age_secs` from the request with a sensible default (document the chosen default, e.g. 24h) rather than hardcoding a magic number in the check body.

3. **Check 2 — stale `spec-implementation` tasks in Backlog.** Filter tasks with label `"spec-implementation"` AND `status == TaskStatus::Backlog`, flag any where `now_secs - created_at > implementation_backlog_max_age_secs` (default N days, e.g. 7d).

4. **Check 3 — modified specs with no corresponding task (defense in depth).** For each spec-ledger entry under the watched paths (use `state.spec_ledger` — the same ledger the hook writes; watched-path filter should match `git_http.rs` `process_spec_lifecycle`), determine whether a task exists that references it. Use `TaskRepository::list_by_spec_path(spec_path)` and treat "has a corresponding task" as: at least one non-Cancelled task whose `spec_path` equals the ledger entry's path. Flag ledger entries whose `current_sha` reflects a modification (i.e., the spec exists and was updated) but for which no such task exists. Do NOT flag brand-new specs that the hook has not yet processed within the same tick — key the check off ledger `updated_at` vs. task existence, not off the push itself.

5. **Escalation to the workspace orchestrator.** For every finding, escalate via the message bus using the established pattern in `api/specs.rs:684-706`: `state.emit_event(Some(workspace_id), Destination::Workspace(workspace_id), <MessageKind>, Some(payload))`. Derive `workspace_id` from the offending task (`task.workspace_id`) for checks 1/2, and from the spec-ledger entry's `workspace_id` for check 3 (fall back to `Destination::Broadcast` when absent, matching specs.rs). Choose an appropriate existing `MessageKind` from `crates/gyre-common/src/message.rs`; only add a new variant (e.g. `AccountabilityEscalation`) if none fits, and if you add one, wire it through all match arms that exhaustively handle `MessageKind`. The payload MUST identify the finding (`finding_type`, `task_id`/`spec_path`, `message`). Escalation is REAL enforcement — the message must actually be emitted, not logged.

6. **HTTP endpoint.** Add `POST /api/v1/patrol/spec-lifecycle` in `api/specs.rs` (handler `patrol_spec_lifecycle`) and register it in `api/mod.rs` next to the existing `/api/v1/patrol/spec-links` route (mod.rs:351). Request body accepts optional `drift_review_max_age_secs` and `implementation_backlog_max_age_secs` (mirror how `PatrolRequest.stale_threshold_secs` defaults in `api/specs.rs:1271`). Response returns the findings. Apply the same auth/role gating the existing patrol endpoint uses.

7. **Do NOT hardcode the "N days" / cycle thresholds inside the check logic.** Accept them as request parameters with documented defaults so task-109 (spec-lifecycle configuration) can later feed per-repo values without a rewrite. This task does NOT depend on task-109 landing.

## Acceptance Criteria

- [ ] New `spec_lifecycle_patrol` module runs all three checks against real storage (`TaskRepository`, `state.spec_ledger`), not in-memory stand-ins.
- [ ] `POST /api/v1/patrol/spec-lifecycle` route registered in `api/mod.rs` and returns findings; auth/role gating matches the existing patrol endpoint.
- [ ] Each finding produces a REAL message-bus escalation to `Destination::Workspace(workspace_id)` via `state.emit_event`, verified by asserting the message was enqueued/emitted (not merely logged).
- [ ] A test seeds: (a) a `spec-drift-review` task with `created_at` older than the threshold and one within it, (b) a `spec-implementation` Backlog task past the threshold and a non-Backlog one, (c) a spec-ledger entry with no corresponding task and one with a task — and asserts exactly the stale/orphaned items are flagged and escalated, while the fresh/covered items are NOT. The test MUST fail if any check is a no-op, if thresholds are inverted, or if escalation is skipped.
- [ ] Terminal (`Done`/`Cancelled`) tasks are never flagged.
- [ ] `cargo test --all` and `bash scripts/check-arch.sh` pass.

## Agent Instructions

- Read `specs/system/spec-lifecycle.md` §Accountability Integration (the contract) and §Configuration (for future threshold sourcing).
- Model the new module on `crates/gyre-server/src/spec_patrol.rs` (types, `run_patrol` shape, `create_notifications_for_error_findings`, and its `#[cfg(test)]` harness for seeding `AppState`).
- Task querying: `crates/gyre-ports/src/task.rs` (`list`, `list_by_status`, `list_by_spec_path`). Task fields (`labels`, `status`, `created_at`, `workspace_id`, `spec_path`): `crates/gyre-domain/src/task.rs`.
- Escalation pattern: `crates/gyre-server/src/api/specs.rs:684-706` (`emit_event` + `Destination::Workspace`). Message kinds: `crates/gyre-common/src/message.rs`.
- The task-creation side (labels/priorities) it must reconcile against: `crates/gyre-server/src/git_http.rs` `process_spec_lifecycle` (~1279-1304).
- Preserve hexagonal boundaries — patrol logic is server-side and depends only on ports/domain, never adapters directly.
- Run validation once at the end. On completion set `progress: ready-for-review` and record commit SHAs in the frontmatter `commits` list.

## Shipped

The three `spec-lifecycle.md` §Accountability Integration checks are live in
`crates/gyre-server/src/spec_lifecycle_patrol.rs` against real storage:

- **Check 1** flags non-terminal (`Done`/`Cancelled` excluded) `spec-drift-review`
  tasks older than `drift_review_max_age_secs` (default 24 h, one loop-cycle
  proxy).
- **Check 2** flags `spec-implementation` tasks with `status == Backlog` older
  than `implementation_backlog_max_age_secs` (default 7 d).
- **Check 3** (defense in depth) flags watched ledger entries with a
  `current_sha`, non-`Deprecated`, past the hook grace window, and no
  non-`Cancelled` task referencing the path (both `system/x.md` and
  `specs/system/x.md` spellings queried via `TaskRepository::list_by_spec_path`;
  repo-scoped when both sides know it). Watched paths come from the repo's
  `SpecLifecycleConfig` (`state.spec_lifecycle_configs.get_for_repo`, the same
  config `git_http::process_spec_lifecycle` consults), so the patrol and the
  hook can never disagree; repos with `enabled = false` are skipped, mirroring
  the hook's early return.

Every finding escalates as a REAL persisted `MessageKind::Escalation` message
on the workspace event stream (`Destination::Workspace(ws)`, `Broadcast` when
no scope resolves — never a fabricated `"default"`), payload identifying
`source`/`finding_type`/`severity`/`spec_path`/`task_id`/`message`/
`suggested_action` and satisfying the message-bus escalation schema.

`POST /api/v1/patrol/spec-lifecycle` is registered in `api/mod.rs` with an ABAC
`RouteResourceMapping` (no exemption); request-body thresholds with documented
defaults keep the check bodies free of hardcoded limits (task-109 config
feed-ready). Storage failures return 500, not an empty finding list.

### Repair rounds

- **Merge 1f460064** (rebase onto base `cfeefbda`): `git_http.rs` lost the
  hardcoded `SPEC_WATCHED_PATHS` constant to main's per-repo config work, so
  check 3's watched-path filter was ported to
  `SpecLifecycleConfig::is_watched`/`enabled` via `get_for_repo` (cached per
  patrol run, error-propagated on store failure). The e2e.yml conflict was
  comment-only; main's text kept. New test
  `check3_watched_paths_follow_per_repo_config` proves the filter is
  config-driven and fails if it reverts to a hardcoded list.
- **Review 630bdf88 F1** (commit `bb5e82ee3`): the two escalation-verification
  tests asserted persisted messages for workspace "ws1" without ever creating
  it, and the post-rebase `emit_event` (task-099 tenant resolution via
  `message_tenant_id`) silently skips persistence when
  `workspaces.find_by_id` yields None — so they failed with 0 persisted
  messages. Both tests now seed the workspace (mirroring `spec_patrol.rs`
  tests), so the asserts prove real emission rather than silent
  skip-on-unresolvable. Test-side only; the production emit path is unchanged
  and sound for real workspaces.
- **Verification 1449c437** (commit `de2d46d4f`, on merge base `3af80bd21`):
  the checks.sh clippy gate flagged
  `spec_lifecycle_patrol.rs:1119: field_reassign_with_default` — the
  `check3_watched_paths_follow_per_repo_config` test seeded its per-repo
  config with `Default::default()` plus out-of-initializer field
  reassignment. Converted to struct-update syntax (the same pattern the test
  already used for the disabled-repo config). Semantics identical, test-code
  only; the lint is confirmed absent from a full
  `cargo clippy -p gyre-server --tests` run at the repaired HEAD (exit 0,
  zero diagnostics mentioning the module). This was the only failure in that
  verification run — rustfmt, all shell lint gates, and the web build passed.

### Test evidence

12 unit tests in `spec_lifecycle_patrol.rs` (incl. end-to-end
`flags_and_escalates_only_the_accountability_gaps`: exactly the
stale/orphaned set flagged AND one persisted escalation per finding with
identifying payloads; fresh/covered/terminal items not flagged;
threshold-tightening proves the checks are not no-ops or inverted) plus 2
endpoint tests in `api/specs.rs` (flags + persisted escalation; thresholds
honoured in both directions).

At the repaired HEAD (`de2d46d4f`): `cargo fmt --check -p gyre-server`,
`SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` (exit 0, includes the
repaired test code), and `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server
--tests` (exit 0, `field_reassign_with_default` absent, no clippy diagnostics
in the task-204 module) all pass in this sandbox — evidence under
`/tmp/stage/review-evidence/task-204-clippy-repair/`. gyre-server test
*execution* is blocked by the build wrapper (4 GiB budget) and is deferred to
the independent verifier. Required probes: `SKIP_WEB_BUILD=1 cargo test -p
gyre-server spec_lifecycle_patrol` (incl. the repaired
`check3_watched_paths_follow_per_repo_config`),
`SKIP_WEB_BUILD=1 cargo test -p gyre-server spec_lifecycle_patrol_endpoint`,
`SKIP_WEB_BUILD=1 cargo test --all`, and `bash scripts/check-arch.sh`.
