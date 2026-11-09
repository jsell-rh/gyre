---
title: "Implement repo-defined persona push detection and approval reset"
spec_ref: "platform-model.md §2 Repo-Defined Personas"
depends_on:
  - task-140
progress: ready-for-review
coverage_sections:
  - "platform-model.md §Repo-Defined Personas"
commits: ["7ffe72535"]
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

- [x] Pushing a change to `specs/personas/*.md` updates the corresponding persona entity
- [x] Persona `approval_status` resets to `Pending` on content change
- [x] `content_hash` and `version` are updated
- [x] A task is auto-created for human approval
- [x] The persona owner is notified
- [x] New persona files create new repo-scoped persona entities
- [x] Non-persona file pushes are unaffected
- [x] Tests pass

## Shipped

Implemented in `7ffe72535` (single commit: `crates/gyre-server/src/git_http.rs`, `crates/gyre-common/src/notification.rs`).

### What ships

- `process_persona_changes` runs in the post-receive spawn immediately after `process_spec_lifecycle` — the same push-processing pipeline, not a separate hook. On default-branch pushes it scans `git diff --name-status --diff-filter=AMDR` (empty-tree base for initial pushes) for `specs/personas/<slug>.md` files via a new `parse_persona_changes` filter on a shared `parse_name_status` parser (`parse_spec_changes` behavior unchanged; the spec-lifecycle watched-path ignore is untouched).
- Per changed persona path: the entity is looked up by slug + `PersonaScope::Repo(repo_id)`. On hit, `system_prompt` is replaced with the file content at the pushed SHA (`git show`, invalid UTF-8 skipped rather than mangled), `content_hash` recomputed, `version` incremented, `approval_status` reset to `Pending`, `approved_by`/`approved_at` cleared, `updated_at` refreshed. On miss, a new repo-scoped persona is registered as `Pending` (version 1). Deleted files (and rename old-paths) reset approval with no content change — there is no new content, but a removed persona file must not keep running on a stale approval; a delete with no registered entity is a logged no-op.
- The approval task is auto-created with the exact contract title `Persona change requires human approval: {slug}`, priority High, labels `["persona-change", "auto-created"]`, scoped to the repo's real workspace (resolved before create), `spec_path` set to the persona file, `task_type: None` (push-hook tasks never trigger agent spawning), deduplicated against open tasks with the same title, and announced via a `TaskCreated` bus event.
- The owner is notified through the existing inbox system with a new `NotificationType::PersonaChangePendingApproval` (priority 2). The owner must resolve to a known user and the workspace tenant must resolve; otherwise the notification is skipped and logged — no fabricated scope identity, no undeliverable rows. A newly discovered persona has no owner yet, so no notification is sent for it.

### Test evidence

- End-to-end (real git commits authored with plumbing and pushed through the real `git_receive_pack` HTTP handler, refs verified to move):
  - `push_persona_change_updates_entity_resets_approval_and_notifies_owner` — seeded approved persona + owner user; push modifies `specs/personas/security.md` → entity updated (content, independently recomputed SHA-256 hash, version 1→2), approval reset (Pending, approved_by/at cleared), exactly one approval task with contract title/labels/priority/scoping, owner notified (`PersonaChangePendingApproval`, entity_ref `persona:security`, correct workspace/repo).
  - `push_new_persona_file_registers_pending_repo_scoped_entity` — push adds `specs/personas/domain-reviewer.md` → new `PersonaScope::Repo` entity, Pending, version 1, correct hash, approval task created, no notification (no owner).
  - `push_without_persona_changes_leaves_personas_untouched` — push touches only `Cargo.toml`/`src/main.rs`/`specs/system/other.md`; verified after two positive controls (spec-lifecycle task for the specs/system file, and the knowledge-graph delta for the pushed commit — the pipeline's last write) that the persona entity is byte-identical (still Approved) and no persona task/notification exists.
- Unit: `parse_persona_changes` (add/modify/delete/rename, non-persona and nested paths ignored, rename-away still detected), `persona_slug_from_path`, `persona_approval_task` shape.
- Mutation check: with the `process_persona_changes` call removed, both positive end-to-end tests fail (persona never updated / never registered); restored and re-verified green — the tests discriminate real behavior.
- Suites: `cargo test -p gyre-server --lib` **1498 passed, 0 failed**; `cargo test -p gyre-common --all-targets` **116 passed, 0 failed**; the 3 integration tests re-run 3× stable. Clippy/fmt clean on task-changed lines; `check-arch.sh` and the byte-slice/relative-path/fail-open-ref/lossy-secret/fabricated-scope/scope-literal/inert-enforcement/forwarded-header/unbounded-http mechanical checks all pass. Full evidence: `/tmp/stage/review-evidence/task-141.md`.

## Agent Instructions

- Read `crates/gyre-server/src/api/git.rs` for the push receive hook implementation
- Read `crates/gyre-server/src/spec_lifecycle.rs` (or equivalent) for the spec change detection pattern — persona detection follows the same approach
- Read `crates/gyre-domain/src/workspace.rs` for `Persona` struct fields
- Read `crates/gyre-ports/src/persona.rs` for `PersonaRepository` operations
- The persona detection must be in the same push-processing pipeline as spec lifecycle, not a separate hook
- `specs/personas/` is currently explicitly ignored in spec-lifecycle processing — you need to ADD persona processing, not modify the spec ignore
