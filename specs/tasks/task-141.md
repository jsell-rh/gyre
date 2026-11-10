---
title: "Implement repo-defined persona push detection and approval reset"
spec_ref: "platform-model.md §2 Repo-Defined Personas"
depends_on:
  - task-140
progress: complete
coverage_sections:
  - "platform-model.md §Repo-Defined Personas"
commits: ["7ffe72535721289c39b3ffca17d9c422439ab61c"]
---

## Spec Excerpt

From `platform-model.md` §2 Repo-Defined Personas:

A repo can define personas in its codebase (e.g., `specs/personas/domain-reviewer.md`) and register them via the API or as part of bootstrap. The content lives in git (versioned, diffable), but the registration and approval live in the forge's database.

On push, if a persona file changes, the forge:
1. Detects the change (like spec lifecycle)
2. Updates the persona entity with new content + content_hash
3. Resets approval to pending
4. Creates a task: "Persona change requires human approval"
5. Notifies the persona owner

## Implementation Plan

1. **Extend push-receive processing** — In the git push receive hook (same path where spec-lifecycle processing runs), add persona file change detection:
   - Scan pushed commits for changes to files matching `specs/personas/*.md`
   - For each changed persona file, extract the new content

2. **Persona update on push:**
   - Look up the persona entity by name (derived from filename, e.g., `security.md` → `"security"`) and repo scope
   - If persona entity exists:
     - Update `prompt` with new file content
     - Recompute `content_hash` (SHA-256 of new content)
     - Increment `version`
     - Reset `approval_status` to `Pending`
     - Clear `approved_by` and `approved_at`
     - Update `updated_at`
   - If persona entity does NOT exist:
     - Create a new persona entity with `PersonaScope::Repo`, `approval_status: Pending`

3. **Auto-create approval task:**
   - Create a Task with title "Persona change requires human approval: {persona_name}"
   - Link to the repo, set priority High, label `["persona-change", "auto-created"]`

4. **Notify persona owner:**
   - Send a notification to the persona's `owner` user ID (use existing notification system)

5. **Tests:**
   - Integration test: push a change to `specs/personas/security.md` → persona entity updated, approval reset to Pending
   - Integration test: push a NEW persona file → new persona entity created with Pending status
   - Integration test: push with no persona changes → no persona entities modified
   - Unit test: approval task auto-created with correct title and labels

## Acceptance Criteria

- [ ] Pushing a change to `specs/personas/*.md` updates the corresponding persona entity
- [ ] Persona `approval_status` resets to `Pending` on content change
- [ ] `content_hash` and `version` are updated
- [ ] A task is auto-created for human approval
- [ ] The persona owner is notified
- [ ] New persona files create new repo-scoped persona entities
- [ ] Non-persona file pushes are unaffected
- [ ] Tests pass

## Shipped

Implemented in `7ffe72535` (`crates/gyre-server/src/git_http.rs`, `crates/gyre-common/src/notification.rs`).

### What ships

- `process_persona_changes` runs in the post-receive spawn immediately after `process_spec_lifecycle` — same push-processing pipeline, not a separate hook. On default-branch pushes it scans `git diff --name-status --diff-filter=AMDR` (empty-tree base for initial pushes) for `specs/personas/<slug>.md` via `parse_persona_changes`, a filter over the shared `parse_name_status` parser (`parse_spec_changes` unchanged; the spec-lifecycle watched-path ignore is untouched).
- Per changed persona path: entity looked up by slug + `PersonaScope::Repo(repo_id)`. On hit, `system_prompt` is replaced with file content at the pushed SHA (`git show`; invalid UTF-8 skipped, not mangled), `content_hash` recomputed (SHA-256), `version` incremented, `approval_status` reset to `Pending`, `approved_by`/`approved_at` cleared, `updated_at` refreshed. On miss, a new repo-scoped persona is registered as `Pending` (version 1). Deleted files (and rename old-paths) reset approval without a content change — a removed persona file must not keep running on a stale approval.
- Approval task auto-created with the exact contract title `Persona change requires human approval: {slug}`, priority High, labels `["persona-change", "auto-created"]`, scoped to the repo's real workspace, `spec_path` set to the persona file, `task_type: None` (push-hook tasks never trigger agent spawning), deduplicated against open tasks with the same title, announced via `TaskCreated`.
- Owner notified through the existing inbox system with `NotificationType::PersonaChangePendingApproval` (priority 2). The owner must resolve to a known user and the workspace tenant must resolve; otherwise the notification is skipped and logged — no fabricated scope identity, no undeliverable rows. A newly discovered persona has no owner yet, so no notification is sent for it.

### Test evidence

- End-to-end (real git commits pushed through the real `git_receive_pack` HTTP handler, refs verified to move):
  - `push_persona_change_updates_entity_resets_approval_and_notifies_owner` — approved persona + owner user; push modifies `specs/personas/security.md` → entity updated (content, independently recomputed SHA-256 hash, version 1→2), approval reset (Pending, approved_by/at cleared), exactly one approval task with contract title/labels/priority/scoping, owner notified.
  - `push_new_persona_file_registers_pending_repo_scoped_entity` — push adds `specs/personas/domain-reviewer.md` → new `PersonaScope::Repo` entity, Pending, version 1, correct hash, approval task created, no notification (no owner).
  - `push_without_persona_changes_leaves_personas_untouched` — push touches only non-persona files; after positive controls (spec-lifecycle task for a `specs/system` file, knowledge-graph delta for the pushed commit) the persona entity is byte-identical (still Approved) and no persona task/notification exists.
- Unit: `parse_persona_changes` (add/modify/delete/rename, non-persona and nested paths ignored), `persona_slug_from_path`, `persona_approval_task` shape.
- Mutation check (previous round): with the `process_persona_changes` call removed, both positive end-to-end tests fail; restored and green — the tests discriminate real behavior.
- This repair round re-ran the focused probes on the restored checkout: the 3 end-to-end push tests, 6 persona unit tests, and `gyre-common` notification tests all pass (see `/tmp/stage/review-evidence/task-141.md`). No product code changed in this round — the executor restored the assigned task document after a protected-text violation (checked acceptance boxes); the production fix from `7ffe72535` is preserved unchanged.

## Agent Instructions

- Read `crates/gyre-server/src/api/git.rs` for the push receive hook implementation
- Read `crates/gyre-server/src/spec_lifecycle.rs` (or equivalent) for the spec change detection pattern — persona detection follows the same approach
- Read `crates/gyre-domain/src/workspace.rs` for `Persona` struct fields
- Read `crates/gyre-ports/src/persona.rs` for `PersonaRepository` operations
- The persona detection must be in the same push-processing pipeline as spec lifecycle, not a separate hook
- `specs/personas/` is currently explicitly ignored in spec-lifecycle processing — you need to ADD persona processing, not modify the spec ignore
