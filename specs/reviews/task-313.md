# Review — task-313 (Publish reviewed spec backlog for specs/development/dogfooding.md)

Spec: `specs/development/dogfooding.md` §Dogfooding (bullet 3 gap, plan `9f3a9c5f2c13ec023664c34fb6a43bab3e0536e2f2e4be0391c690724f3f23f1`).
Candidate under review: `a5062ca4` (branch `pipeline/task-313/3d5517fc3f5f4aadaf93aba04d959cb8-1`, base `82aed3db`).
Verdict: **approved** — no findings. The publication is faithful, additive-only, and structurally sound.

## Probes run (evidence under `/tmp/stage/review-evidence/`)

- `git diff 82aed3db..a5062ca4 --name-only/--stat` → exactly 3 files: `specs/tasks/task-312.md` (new), `specs/tasks/task-313.md` (new), `specs/task-spec-inputs.json` (diff-scope.txt).
- Semantic JSON diff of `specs/task-spec-inputs.json` vs base → exactly 2 additions (`sections/backlog/69fff34a83938a4731034172` and its binding), nothing removed or modified; no dangling binding targets anywhere in the registry (registry-semantic-diff.txt).
- Production registry machinery (`scripts/pipeline/requirements.py` `snapshot()` with the real `catalog.contract`): task-312 resolves `errors=[]` with input `backlog/69fff34a83938a4731034172`; task-313 resolves `errors=[]`; the bound section component equals the full canonical `specs/development/dogfooding.md` body; the heading `["Dogfooding"]` matches a unique node (registry-snapshot-resolution.txt).
- Plan fidelity (programmatic comparison against the plan embedded in the assigned task body): task-312's required_behavior, all 6 acceptance items, negative_probe (Verification intent), title, `depends_on: []`, and `progress: not-started` all match the reviewed plan verbatim (plan-fidelity.txt).
- Audit challenge (audit-facts.txt): re-verified the load-bearing absence evidence independently — grep for `gyre-server`/`localhost:3000`/`/api/v1` across `scripts/pipeline/` and all `scripts/dev-*.py` returns **0 matches**; the cited task REST surface exists at `api/mod.rs:407-415`; both orchestrator-spawn routes exist as unused product surface; `signal_chain.rs:802` skips non-Implementation task types ("untyped → never spawned"), confirming the plan's untyped-registration strategy; `eligibility.py:79` evaluates dependencies from local pipeline state only; `catalog.py:43 sync_checkout`, `execution.py:106` gateway setting, and `stages.py` publication rewrite all check out. The gap decision (bullet 3 unimplemented — bespoke pipeline, not the platform, holds the dev-orchestrator role) is sound.
- Duplicate work / precedent (duplicate-and-precedent.txt): no other task or file binds section `69fff34a83938a4731034172`; no other task addresses dogfooding; task-298 (orchestrator-spawn removal) and task-297 (approval gate) interactions are reconciled by design (untyped registration, no spawn routes, no `gyre_spawn_worker`) rather than duplicated or left as hidden dependencies. The commit follows the exact pattern of prior publication commits (`93a834af`, `fe91bf67`), and prior publication task docs were likewise `ready-for-review` in their publication commit.

## Acceptance criteria check

- Every planned task faithfully implements its assigned canonical requirements: task-312 is a verbatim publication of the single planned task; the plan itself was re-challenged and its audit evidence independently re-verified (see above).
- No duplicate active assignment or missing dependency: single task, `depends_on: []` per plan, section bound once, no overlap with task-297/298.
- No product behavior, spec requirement, coverage status or enforcement changed: additive-only 3-file diff; registry semantic diff confirms zero modifications to existing entries.
- New tasks remain not-started until publication merges: task-312 is `progress: not-started`; task-313 (the publication task itself) is `ready-for-review`, matching precedent.

## Non-blocking observation (recorded for completeness, upstream audit prose)

The audit prose says "all 42 scripts/pipeline/*.py modules"; the actual count is 41 at both the audit base `88202d6` and the publication base `82aed3db`. The assignment requires publishing the reviewed plan exactly, which the candidate does; the zero-match absence evidence is unaffected either way. No action required for this publication; noted so the count is not cited as fact later.

## Verifier notes

No server or Rust compilation is needed for this candidate — the diff touches only task documents and the spec-input registry, and both were validated through the production Python machinery that consumes them (`requirements.snapshot`, `requirements.sections`). Nothing to defer.
