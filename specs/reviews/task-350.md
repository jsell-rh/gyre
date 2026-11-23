# Review — task-350 (Publish reviewed spec backlog for specs/system/agent-runtime.md)

Spec: `GOAL.md — real implementations and meaningful verification` (backlog publication for `specs/system/agent-runtime.md`, plan `da093bb0d16da84dcae7a1f8fc0ba4a14ecb9e8275c579ccae32fe282b48fdcc`, revision-4 audit `7db16b5d9dc226dc1a99fbcf46f11e946fa694ba58d53b3e122ad2f572e9ae28`, requirements `e2624c4730e70e80879acbc050b81404fa9129fc90924610a81c15658ce31de5`).
Candidate under review: `1c45652b` (branch `pipeline/task-350/4649504f1b404b7f90db19fe70711780-1`, base `973068a8`).
Verdict: **approved** — no findings. The publication is exact, minimal, and preserves everything it must.

## Contract

Publish exactly the independently reviewed missing-work plan: create only the assigned task documents, add their exact section bindings, preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation.

## Probes (docs-only candidate; no runtime probe distinguishes it from the base)

- `git diff --stat 973068a8..1c45652b` → 3 files, +1803/−0: `specs/tasks/task-349.md` (new), `specs/tasks/task-350.md` (new), `specs/task-spec-inputs.json` (+3 lines, one binding entry). Working tree clean at the candidate.
- Audit-base drift check: `git diff --name-only 0b33cb3a..973068a8` → Dockerfile, docs/server-config.md, specs/coverage/system/lsp-call-graph.md, specs/tasks/task-323.md only — none of the audited agent-runtime code files, so the audit's code-state claims remain valid at the candidate.
- Plan fidelity (python `raw_decode` on both): the plan JSON embedded in task-350.md is identical to the reviewed plan in the assignment; task-349.md reproduces its plan task (`amendment-table-docs-and-binding`) verbatim — required behavior, all four acceptance criteria, the negative probe (as Verification intent), `depends_on: [task-298, task-302]`, and a discovery-evidence JSON block identical to the audit's section entry `a40446f46dbb0536f3c30ed0`.
- `specs/task-spec-inputs.json` parses as valid JSON; the one new binding `Spec gap spec-41a92c20354c6a108e6074e3 / amendment-table-docs-and-binding / e2624c4730e7 => [backlog/a40446f46dbb0536f3c30ed0]` targets an existing sections-registry entry (`Agent Runtime / Relationship to Existing Specs`) and matches the key format of the precedent publication (93a834af, revision 3). No registry entry removed or altered.
- Evidence saved under `/tmp/stage/review-evidence/publication-fidelity.txt`.

## Audit challenge (re-read against the canonical spec and code)

Every spot-checked factual claim of the revision-4 audit holds at the candidate: both orchestrator-tier MCP spawn tools are live production code (mcp.rs:545, :605); `/api/v1/personas` routes remain registered (api/mod.rs:1054 ff.); `specs/system/message-bus.md` contains no `SpecApproved` (count 0) while the kind exists in code; `docs/agent-protocol.md` and `specs/system/ralph-loop.md` still exist; the Workspace entity has no `max_agent_iterations` (only Repository); the reject handler transitions agents to `Stopped` synchronously (specs.rs:1567) with the 60-second grace existing only as payload text (specs.rs:1610); the thorough-preview (api/mod.rs:1332) and cascade-test trigger (api/mod.rs:747) surfaces exist; hierarchy-enforcement.md:346 still carries the stale in-memory meta_spec_sets note. All quoted canonical-spec sentences exist verbatim in `specs/system/agent-runtime.md`, including the amendment-table rows task-349 owns.

## Decomposition, dependencies, duplicate work

- task-349's dependencies exist: task-298 (not-started), task-302 (complete).
- The retirement-sequencing correction is accurate: task-310 and task-269 are complete (the revision-3 draft's task-311/task-272 references were misnumbered; task-311 is a complete architecture.md publication).
- "No active plan owns cascade testing" holds: task-280 (cascade test execution) and task-285 (dependency-graph publication) are complete.
- No active task owns the doc rows task-349 claims (message-bus SpecApproved documentation, agent-protocol.md retirement, HSI §5, meta-spec-reconciliation retired-endpoint rows, Agent task-binding row): all tasks referencing them are complete. Existing task-298's revision-3 contract covers only the code-side amendment rows, so task-349's rows (1)–(6) do not duplicate it; the ownership split is explicit in the plan and backed by the dependency edge. The plan's broadened task-298 contract (both orchestrator-tier spawn tools) is carried by the published plan text for the executing worker, per the assignment's "preserve every existing task" instruction — the publication correctly did not edit task-297/298/300/303.
- task-349 remains `not-started`; task-350's frontmatter matches the assigned task body exactly (progress `ready-for-review`, commits `[]`); the publication's commit subject is not task-labeled and touches no product surface, so commit-attribution is out of the check's scope (same shape as the precedent publication 93a834af).
- "Shipped" states task definitions and bindings only — no implementation claimed, no coverage status touched.

## Non-blocking observations (recorded for the controller, not defects of this candidate)

- Inside the exact published plan text, task-255 is called "the active message-bus plan" while task-255 is complete. Wording inaccuracy in the controller-reviewed plan itself; the substantive no-overlap claim holds (no active task owns the SpecApproved doc row).
- The plan references task-321 (ci-docs-release docs-site task), which does not exist in specs/tasks/ yet — a forward reference to a controller-assigned number outside this plan's assigned_names; the plan gives it no dependency edge, only a close-out note. When that plan is published, its docs-site task should consume docs/ dynamically as assumed.

## Verifier must run

Nothing beyond the standard suite — the diff is docs-only (specs/tasks/ + specs/task-spec-inputs.json); no Rust or web behavior changed. The standard deterministic guards and suite runs on the merged tree are sufficient.
