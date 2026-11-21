# Review — task-305 (Publish reviewed spec backlog for specs/system/agent-runtime.md)

Spec: `specs/system/agent-runtime.md` (canonical spec re-read; audit plan `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`, audit base `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`, requirements hash `4a3f940495a8…`).
Candidate under review: `93a834af34ff795d9de271705f2f25ad2bef64a2` (branch `pipeline/task-305/95a08232ac1141f483ea1d2d591b877c-1`, base `bacba2b2`). Single commit `chore: publish reviewed spec backlog 6a0d0dc0bb14`.
Verdict: **approved** — the publication is a faithful, additive rendering of the assigned plan; no blocking findings.

## What the candidate does

Diff from base: 10 files, 2361 insertions, 0 deletions — `specs/tasks/task-297.md` … `specs/tasks/task-304.md` (the 8 gap tasks), `specs/tasks/task-305.md` (the publication task itself), and `specs/task-spec-inputs.json` (+122 lines: 12 `backlog/<section-id>` section entries for agent-runtime.md and 8 `Spec gap spec-41a92c20354c6a108e6074e3 / <task-key> / 4a3f940495a8` binding keys). Nothing else is touched: no spec, coverage matrix, registry entry, script, or product file is modified.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `structural-fidelity-check.txt` — mechanical comparison of every published task file against the plan JSON extracted from the assigned task body: frontmatter (title, depends_on, progress, commits), Required behavior, Acceptance Criteria, Verification intent, and the Discovery-evidence gap sections.
- `audit-quote-probe.txt` — spot-checks of the audit's evidence quotes against the candidate tree.
- `task-spec-inputs-check.txt` — JSON integrity, binding-target resolution, overlap and duplicate-work checks.

## Verified

- **Task fidelity (task-297 … task-304).** For each of the 8 planned tasks: title matches; `depends_on` matches the plan's dependency edges mapped through `assigned_names` (298→302→301, 303→298; no cycles); `progress: not-started` and `commits: []` (the "new tasks remain not-started" acceptance criterion); Required behavior, Acceptance Criteria, and negative_probe (Verification intent) are byte-verbatim from the plan — the only addition is the standing anti-fake-completion trailer sentence ("Update the relevant coverage rows only after implementing the behavior…"), which is the established convention (present in 38 prior task docs). Discovery evidence contains exactly the covered gap sections with their full audit entries (id, decision, requirement_quote, detail, evidence) verbatim; every one of the 12 gap section ids is bound to exactly one new task, matching the plan's `covers` sets.
- **Publication task.** `specs/tasks/task-305.md` is byte-identical to the assigned task body (which embeds the full reviewed plan JSON, including plan_id `6a0d0dc0bb14…` matching the commit subject).
- **task-spec-inputs.json.** Parses cleanly; the 12 new backlog section entries all point at `specs/system/agent-runtime.md` headings; the 8 new binding keys resolve to those entries; no other binding overlaps the new section ids; no existing entry is altered (additive diff only).
- **No duplicate active assignment.** Prior agent-runtime tasks (task-115…119, 137, 190, 191) are all `progress: complete`; the coverage matrix's §2 rows attributed to task-116 reflect storage-layer work that task-116 completed — the new tasks 301/302 cover the audit's post-completion residual gaps (route-path divergence, scope-level `required` gate, bindings cascade, stale-pin cascade), not a re-assignment of open work. No open task covers any of the 12 gap sections.
- **Audit grounded in reality (spot checks).** The audit's key evidence quotes resolve in the candidate tree: `gyre_spawn_worker` dispatch (mcp.rs:5264); unscoped `agents://` listing (mcp.rs:4676); decorative `"grace_period_secs": 60` on the reject path (api/specs.rs:1541); tenant-Admin-only registry write gate (api/meta_specs.rs:8661); `/api/v1/meta-specs-registry` route prefix (abac_middleware.rs:782); Dockerfile build guidance in the spawn failure path (api/spawn.rs:1771) with `docker/gyre-agent/Dockerfile` still present (`FROM node:22-alpine`). The corresponding spec quotes ("There is no other trigger…", "It never spawns agents…", "The Dockerfile is replaced by the Nix flake", "no knowledge of other agents…") resolve in the canonical spec. The gap/decision structure is consistent with the canonical spec text re-read for this review.

## Minor (non-blocking)

- `task-301.md` and `task-304.md` list their two covered gap sections in the Discovery-evidence block in swapped order relative to the plan's `covers` arrays. Content is byte-identical and the bound set is exact, so the section bindings are exact as sets; ordering within one evidence block carries no semantic weight.

## Verifier notes

No product code, build, or test surface is touched by this diff — there is nothing to compile or execute beyond the JSON/Markdown integrity checks recorded above. Standard pre-commit/CI mechanical checks on the merged tree remain the verifier's responsibility as usual.
