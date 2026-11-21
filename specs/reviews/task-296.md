# Review — task-296 (Publish reviewed spec backlog for specs/system/activity-dashboard.md)

Spec: `specs/system/activity-dashboard.md` (single section "Activity Dashboard / Feed", five normative bullets).
Candidate under review: `d8f3793832885efdfe648d9e51f9065f342f5728` (base `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`).
Verdict: **approved** — faithful backlog publication; no findings. Evidence under `/tmp/stage/review-evidence/` (evidence-log.txt, check_fidelity.py, check_hashes.py, capture.sh).

## Scope verified (diff base..candidate)

Exactly three spec-side files, additions only (312 insertions, 0 deletions): new `specs/tasks/task-295.md` (implementation task, `progress: not-started`, `commits: []`), new `specs/tasks/task-296.md` (publication record), and `specs/task-spec-inputs.json` (+9 lines: one `sections` entry, one `bindings` entry). No product code, canonical spec, coverage file, registry exemption, or gate touched. Working tree clean. `scripts/check-task-commit-attribution.sh` passes. Commit message follows the established publication convention (`chore: publish reviewed spec backlog edd4a0642e19`; plan_id prefix matches; precedent: task-277's `1c088af5`).

## Audit challenged and confirmed

All seven `source_evidence` sha256 hashes match the base tree (`git show base:file | sha256sum` — see check_hashes.py output). Every factual claim re-verified on the tree:

- Unified `activity_handler` (crates/gyre-server/src/api/activity.rs:30) uses only `params.limit`; its declared `since` is never read, and `NotificationRepository::list_recent` (gyre-ports/src/notification.rs:136) is limit-only — defect (b) is real.
- `loadActivity` is one-shot per workspace-scope change in both WorkspaceHome.svelte (def :1007, single call site :1287) and CrossWorkspaceHome.svelte (:471/:272) — defect (a) is real.
- api.js `activity(limit)` (line 81) passes only `limit`; `agentTtyUrl` (line 292) has zero consumers — dead code, defect (e) is real.
- "Tailscale" appears nowhere in `crates/` or `web/src/`; the only "NixOS" occurrence is the admin.rs:876 seed task title; no try-it/ephemeral/PreviewEnvironment code exists — defects (c), (d), (f) are real.
- Latest committed migration is `2026-11-20-000069_scim_group_member_display`, so the plan's "next is 000070" is correct.
- The audit's `requirement_quote` matches `specs/system/activity-dashboard.md` verbatim (one heading, five bullets); the gap decision is justified.

## Plan → task fidelity (programmatic)

`required_behavior` is byte-identical to the plan; all 7 acceptance criteria verbatim; the negative probe is carried into "Verification intent"; the discovery-evidence JSON is byte-identical to the plan's gap object (10 evidence items, id `dba01522c9fb7fb9aa3be898`). See check_fidelity.py.

## Registry and duplicate-work analysis

`specs/task-spec-inputs.json` is valid JSON; the new `sections["backlog/dba01522c9fb7fb9aa3be898"]` entry follows the existing `backlog/<id>` convention (28 pre-existing) and correctly names file `specs/system/activity-dashboard.md` with heading path `["Activity Dashboard / Feed"]`. The new binding key `Spec gap spec-b128f93cd5d035d28c8bbf7f / activity-dashboard-completion / f0cbfae7cd02` matches task-295's `spec_ref` exactly, and its requirements-hash prefix matches the audit's full `requirements` hash. No other section entry or binding references this spec/section; no active (not-started/in-progress) task covers the activity dashboard (related tasks task-022/096/114/120/127/252/255 are complete or bound to other specs — task-252 covered the message-bus telemetry endpoint, a different spec). `existing_task: null` holds. Dependencies `[]` is correct: the task builds entirely on existing machinery (ports/adapters, migrations, jobs.rs, compute targets, WireGuard mesh, /ws channel), and the noted WorkspaceHome.svelte overlap with the ui-layout plan is correctly framed as file hygiene, not a dependency.

## Constraints on the published task

task-295 remains `not-started` with `commits: []` until this backlog publication merges, as required. The task text honors repo invariants the implementation must respect (port contract across mem/SQLite/PostgreSQL adapters, next migration sequence 000070, ABAC RouteResourceMapping with no exemption-file entries, store! pattern instead of in-memory AppState maps, per-handler workspace-membership scoping with a 403 negative probe).

No verifier follow-up required for this publication itself; the shipped implementation of task-295 will need the full suite (its negative probes and acceptance criteria are recorded in the task document).
