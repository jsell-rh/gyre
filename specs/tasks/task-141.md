---
title: "Implement repo-defined persona push detection and approval reset"
spec_ref: "platform-model.md §2 Repo-Defined Personas"
depends_on:
  - task-140
progress: not-started
coverage_sections:
  - "platform-model.md §Repo-Defined Personas"
commits: []
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

## Agent Instructions

- Read `crates/gyre-server/src/api/git.rs` for the push receive hook implementation
- Read `crates/gyre-server/src/spec_lifecycle.rs` (or equivalent) for the spec change detection pattern — persona detection follows the same approach
- Read `crates/gyre-domain/src/workspace.rs` for `Persona` struct fields
- Read `crates/gyre-ports/src/persona.rs` for `PersonaRepository` operations
- The persona detection must be in the same push-processing pipeline as spec lifecycle, not a separate hook
- `specs/personas/` is currently explicitly ignored in spec-lifecycle processing — you need to ADD persona processing, not modify the spec ignore
