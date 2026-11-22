# Review — task-340 (Publish reviewed spec backlog for specs/system/spec-registry.md)

Spec: `specs/system/spec-registry.md` — publication of independently reviewed missing-work plan `831e7d8656a4b362fd8a8e5e7b832c0c885c4b1a8c08c05832118ddb1e584c0f`.
Candidate under review: `7ac3cc7acb1dd214253870864d2ab9fa99657d90` (base `c42bfe033949ef04f8c4c41681126f116ab6a9de`, single commit `chore: publish reviewed spec backlog 831e7d8656a4`).
Verdict: **approved** — no findings. Documentation-only publication; no product code touched.

## Scope of the diff

Nine new files/hunks, all under `specs/`: task docs `task-333.md` … `task-339.md`, the assignment record `task-340.md`, and 85 added lines in `specs/task-spec-inputs.json` (8 section entries + 7 binding entries). Zero changes to production code, scripts, coverage matrices, canonical specs, or gates — exactly the "task creation is not implementation" contract. Working tree clean.

## Plan fidelity (mechanical comparison)

The JSON embedded in `specs/tasks/task-340.md` is **identical** to the assigned plan (parsed and compared deep-equal; `plan_id` `831e7d…`). For each of the 7 planned tasks the corresponding `task-33N.md` was compared programmatically against the plan entry: title, `required_behavior` (whitespace-normalized equality), all acceptance bullets, the negative probe (carried under "Verification intent"), and every `covers` section id — **all faithful**; all 7 carry `progress: not-started` and empty `commits`, as required for a not-yet-merged backlog. Task numbering matches the assigned names (333 approver-owner-default … 339 accountability-manifest-patrol; 340 is the publication task itself).

## Bindings and duplicate-work analysis

- All 8 new `backlog/<section-id>` keys appear exactly twice in `task-spec-inputs.json` (once as a section, once in a binding); no key is reused across bindings. `task-spec-inputs.json` parses as valid JSON after the change.
- Every heading path in the new section entries exists verbatim in `specs/system/spec-registry.md` (Manifest Schema, Manifest Rules, API Surface, Auto-Generated Index, Spec Lifecycle / Agent Gates / Spec-to-Code Binding / Accountability Agent integration sections).
- Prior bindings to spec-registry.md sections belong exclusively to `complete` tasks (task-017 manifest enforcement, task-169 supply-chain AIBOM, task-193 §9 resolution, task-194 lifecycle integration, task-195 gate selection) — no duplicate active assignment. The plan's `existing_task: null` for all seven is correct.
- Cross-plan coordination boundaries reference real tasks: the staged spec-lifecycle plan (task-302/task-305 carry spec-lifecycle-sha-pin/binding-refs/dedup-update) exists; task-334's boundary text correctly fences it.

## Audit challenge (gap claims re-verified against production code at the candidate)

Every audited gap was independently confirmed in the code (evidence: `/tmp/stage/review-evidence/04-audit-spot-checks.txt`):

1. `is_valid_human_approval` (spec_registry.rs) — `a.human_approvers.is_empty() || …contains(…)` treats an omitted list as any-human; no `[owner]` default anywhere in the parse path. Gap real.
2. `process_spec_lifecycle` (git_http.rs:3401) — `if !config.enabled { return; }` runs before the manifest is read; invalidation keys on `config.auto_invalidate_approvals` (line ~3501). Gap real.
3. `specs/manifest.yaml` — 31 entries, zero self-entries; no `manifest.yaml` special-case in the hook. Gap real.
4. `get_spec` (api/specs.rs:385) — single-spelling `list_by_spec_path` while the hook writes the git-rooted spelling and the patrol queries both forms (`for query in [rooted_path, alt]`). Gap real.
5. `spec_index` — table is `| Spec | Status | SHA |` only; `state.tasks` never queried in the handler. Gap real.
6. Dual approval stores — `pub spec_approvals:` (SpecApprovalRepository) and `pub spec_approval_history:` (SpecApprovalEventRepository) both in AppState. Gap real.
7. `verify_spec_ref` (api/gates.rs:819) — queries only `spec_approvals.find_by_spec_sha`; `requires_approval` appears 0 times in gates.rs and merge_processor.rs. Gap real.
8. `spec_lifecycle_patrol.rs` — `mod tests` starts at line 519; all orphan/stale references are test fixtures; production coverage keys on `config.is_watched` (line 269); no orphan/stale-entry/pending-age check exists. Gap real.

The audit's "satisfied" claims interleaved with these gaps also spot-check true (team-ref expansion in api/specs.rs, patrol dual-spelling, per-SHA merge-gate query, AIBOM section). Requirement quotes for all 8 bound sections exist verbatim in the canonical spec. Base delta between the audit base (`52d7b66b`) and the candidate base (`c42bfe03`) is docs-only (the task-324 publication), so the audit's code evidence remains valid at the candidate.

## Decomposition and dependency check

The dependency graph is acyclic and justified: 335 (self-registration) depends on 334 (manifest must be authoritative for its own file's gating); 337 (store unification) depends on 333 (the unified store must carry the corrected owner-default predicate, not the any-human default); 338 (merge-gate manifest policy) depends on 337 (building on the dual store would be discarded work). 333, 334, 336, 339 are independent. Every audited gap section is covered by exactly one task; the two API read-path gaps share one task (336) which is coherent since both are read-path completion of the same surface.

## Verifier notes

Documentation-only change; no build or test execution is affected. Standard mechanical checks (arch, task-commit attribution on merge) should be run by the verifier as usual, but nothing in this diff can alter runtime behavior.
