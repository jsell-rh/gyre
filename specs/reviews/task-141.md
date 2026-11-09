# Review: task-141 — Repo-defined persona push detection and approval reset

- **Candidate:** `e7eaedcb4e5058156c7716c62f8c2f9f943902f7`
- **Base:** `e5415ae1237b9751ca95297adc7a241d4e94ec76`
- **Product commit:** `7ffe72535721289c39b3ffca17d9c422439ab61c` (`crates/gyre-server/src/git_http.rs`, `crates/gyre-common/src/notification.rs`)
- **Verdict:** Approved (R1, independent review; probe evidence in `/tmp/stage/review-evidence/task-141.md`)

## What was verified

**Contract compliance (platform-model.md §2 Repo-Defined Personas).** All five specced push behaviors are implemented in production code: `process_persona_changes` runs in the `git_receive_pack` post-receive spawn immediately after `process_spec_lifecycle` (same pipeline, not a separate hook); changed `specs/personas/<slug>.md` files are detected via `git diff --name-status` (flat `.md` files only, nested/non-md ignored); the repo-scoped persona entity is updated with the file content at the pushed SHA, `content_hash` recomputed (SHA-256 via domain `refresh_content_hash`), `version` incremented, approval reset to `Pending` with `approved_by`/`approved_at` cleared; new files register new `PersonaScope::Repo` entities as `Pending`; the approval task is auto-created with the exact contract title, High priority, and `["persona-change","auto-created"]` labels; the owner is notified through the real inbox with the new `NotificationType::PersonaChangePendingApproval` (priority 2). The SQLite adapter's `find_by_slug_and_scope` backs the production storage path.

**Behavior-preserving refactor.** `parse_spec_changes` became a filter over a shared `parse_name_status` parser with the watched-path logic moved verbatim; `SPEC_WATCHED_PATHS` is unchanged, so `specs/personas/` remains outside spec-lifecycle watched paths and persona processing is purely additive, per the task instructions.

**Scope safety.** Task and notification scoping is derived from the real repo row and its workspace; a missing repo, unknown owner user, or unresolvable tenant skips the affected write with a log line rather than fabricating a scope identity (task-097 F3 posture). Invalid UTF-8 file content is skipped and logged, not lossy-converted.

**Test discrimination (mutation check).** With the `process_persona_changes` call disabled, both positive end-to-end tests (`push_persona_change_updates_entity_resets_approval_and_notifies_owner`, `push_new_persona_file_registers_pending_repo_scoped_entity`) fail at their poll deadlines while the negative test stays green — the tests prove real production behavior, not plumbing. Restored and re-run green.

**Probes run (independent, this round):** focused persona suite (`cargo test -p gyre-server --lib persona`: 24 passed / 0 failed, cold build), `gyre-common` notification tests (4 passed / 0 failed), the mutation check above, `check-arch.sh` and `check-dead-message-kinds.sh` (both exit 0). Working tree clean; no source modified in the final state.

**Bookkeeping.** Protected task text intact (acceptance boxes still unchecked); the `## Shipped` summary and frontmatter updates are the sanctioned non-protected additions; the product commit is recorded in `commits:` frontmatter.

## Observations (non-blocking, no findings)

- Approval-task dedup is title-only against the global task list — identical to the pre-existing spec-lifecycle dedup pattern in the same file. Theoretical edge: two repos with the same persona slug pending simultaneously would share one open task title. Inherited pattern, not a new defect class; a follow-up could scope the dedup by `repo_id`.
- Deleted persona files reset approval without deleting the entity (entity keeps its last registered prompt). The spec is silent on deletes; the shipped notes document this as a deliberate choice.
