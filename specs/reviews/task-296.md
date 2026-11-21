# Review — task-296 (Publish reviewed spec backlog for specs/system/activity-dashboard.md)

Spec: `specs/system/activity-dashboard.md` (single section, audit gap `dba01522c9fb7fb9aa3be898`).
Candidate under review: `74764aa6` (branch `pipeline/task-296/724ab3211ca34c5899ece107420408dc-1`, base `9e7cf62e`; merge commit `1f19a665` reconciles the publication onto the base that carried the dependency-graph backlog).
Verdict: **approved** — no findings. Documentation/registry publication only; no product code touched.

## Scope of the diff (base `9e7cf62e` → candidate `74764aa6`)

Exactly three files, insertions only:

- `specs/task-spec-inputs.json` (+9): one `backlog/dba01522c9fb7fb7aa3be898` section definition (`file: specs/system/activity-dashboard.md`, `heading: ["Activity Dashboard / Feed"]` — exact spec heading) and one task binding (`Spec gap spec-b128f93cd5d035d28c8bbf7f / activity-dashboard-completion / f0cbfae7cd02 → [backlog/dba01522c9fb7fb9aa3be898]`), schema-identical to the sibling dependency-graph entries the base added. `python3 -m json.tool` validates; both base-side entries and this plan's entries are present (merge kept both sides, no loss).
- `specs/tasks/task-295.md` (new, 92 lines): the assigned implementation task.
- `specs/tasks/task-296.md` (new, 223 lines): the publication task record, embedding the reviewed plan verbatim.

Zero changes under `crates/`, `web/`, `scripts/`, `specs/coverage/`, `specs/system/`, or any pre-existing task file — verified by empty diffs for each of those paths.

## Checks performed

- **Faithfulness (programmatic):** task-295.md's title, `depends_on: []`, `progress: not-started`, required-behavior paragraph, all seven acceptance criteria, and the negative probe were compared as exact strings against the reviewed plan embedded verbatim in the assignment body — all match (script: `/tmp/stage/review-evidence/check_task295_faithful.py`, PASS). The discovery-evidence JSON block reproduces the audit's gap entry.
- **Audit challenge (required behavior):** re-read the canonical spec (one heading, five normative bullets — matches the audit's `requirement_quote` verbatim) and re-verified every load-bearing audit claim against the current post-rebase tree:
  - `activity_handler` declares `since: Option<u64>` and reads only `params.limit`; `NotificationRepository` has only limit-only `list_recent` and type-specific `list_by_workspace_type_since` — the unified feed cannot filter by time.
  - `agentTtyUrl` in `web/src/lib/api.js:292` has zero consumers (dead code); the client `activity()` passes only `limit`.
  - "Tailscale" appears nowhere in `crates/`/`web/src`; "NixOS" only as a seed task title (`admin.rs:876`); no try-it/ephemeral/preview-environment route exists in `api/mod.rs`.
  - The one `since`-honoring surface (`workspace_activity_handler`) reads the in-memory `TelemetryBuffer` ring buffer.
  - All four spot-checked audit `source_evidence` sha256 hashes (`activity.rs`, `api.js`, `message.rs`, `tty.rs`) match the current tree byte-for-byte — the rebase onto `9e7cf62e` did not invalidate the audit. Gap decision holds.
- **Decomposition/dependencies:** single-section spec → one task (`task-295`) covering gap `dba01522c9fb7fb9aa3be898` with empty `depends_on` (correct; the noted ui-layout overlap on `WorkspaceHome.svelte` is file hygiene, explicitly scoped in the task text). The plan's migration-numbering claim (latest `2026-11-20-000069_scim_group_member_display`, next `000070`) matches the migrations directory.
- **Duplicate work:** no other task file or registry binding references `activity-dashboard`; the registry contains exactly one definition and one binding for the gap id. Task numbers 295/296 are the assigned names with no collision (highest pre-existing is 285).
- **Preservation:** no coverage status, spec text, product file, or gate changed; `task-295` remains `not-started` until this publication merges, per the acceptance criteria.

## Runtime probes

None — the candidate diff contains zero production code, so no compile or server probe distinguishes it from the base. All verification is the structural/faithfulness work above; evidence bundle saved under `/tmp/stage/review-evidence/` (diff-stat, registry validation, faithfulness script, audit-challenge greps, hash comparisons).

Verifier note: nothing task-specific to execute beyond the standard repo checks; the next executable surface is task-295 itself (its negative probes — `GET /api/v1/activity?since=T` exclusion and the try-it 403/side-effect-free probe — are correctly documented as failing on the current tree, which is the expected pre-implementation state).
