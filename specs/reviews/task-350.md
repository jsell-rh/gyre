# Review — task-350 (Publish reviewed spec backlog for specs/system/agent-runtime.md)

Spec: backlog publication for `specs/system/agent-runtime.md`, plan_id
`da093bb0d16da84dcae7a1f8fc0ba4a14ecb9e8275c579ccae32fe282b48fdcc` (audit base
`0b33cb3a4a586c587e2566ecbcd82c03da94d84b`, requirements
`e2624c4730e70e80879acbc050b81404fa9129fc90924610a81c15658ce31de5`).
Candidate under review: `9ba615cc` (branch
`pipeline/task-350/c093159360d549c586dc3a309498b074-1`, base `f3ca7db3`; publication commit
`1c45652b`, prior-round process commit `b4014211`, merge reconciliation `d59d94b9`/`9ba615cc`).
Verdict: **approved** — no findings. Evidence under `/tmp/stage/review-evidence/`.

## Round 2 (fresh verdict; the prior round's approval does not transfer)

This round's candidate is the merge reconciliation of the previously published backlog with
current main (`f3ca7db3`, which merged the dependency-graph publication task-341/task-342).
Probes run (documentation-only candidate — no Rust/web artifact touched, so no build or test
run is needed or meaningful; all mechanical checks operate on unchanged inputs):

- Cumulative diff base→candidate is exactly three files: `specs/tasks/task-349.md` (new, 104
  lines), `specs/tasks/task-350.md` (new, 1700 lines), `specs/task-spec-inputs.json` (+3 lines,
  one binding). No path under `specs/coverage/`, `specs/system/`, `crates/`, `web/`, `scripts/`,
  or `docs/` is touched — no product behavior, spec text, coverage status, or gate changes.
- The 1673-line plan JSON embedded in `task-350.md` is **byte-identical** to the assigned plan
  in the task body (programmatic extraction and comparison; probe 02).
- `task-349.md` faithfully carries the plan's `amendment-table-docs-and-binding` entry: title,
  `spec_ref` (`Spec gap spec-41a92c20354c6a108e6074e3 / amendment-table-docs-and-binding /
  e2624c4730e7`), `depends_on: [task-298, task-302]`, `progress: not-started`, the
  `required_behavior` / all four acceptance items / `negative_probe` verbatim, and the full
  discovery-evidence audit section for `a40446f46dbb0536f3c30ed0`. It is byte-identical to the
  previously published candidate `d59d94b9`'s copy.
- Registry: `specs/task-spec-inputs.json` is valid JSON; the union merge retains BOTH sides —
  the new agent-runtime `amendment-table-docs-and-binding` binding
  (`backlog/a40446f46dbb0536f3c30ed0`, whose `sections` entry already exists) and main's
  dependency-graph `dep-edge-orphaned-on-target-archive-delete` binding. Every prior binding is
  preserved.
- Audit accuracy spot-checks against the tree (all confirmed, probe 03): both
  `gyre_spawn_worker` and `gyre_spawn_repo_orchestrator` are live in `mcp.rs`; the reject
  handler's `grace_period_secs` exists only as message payload (`specs.rs:1610`) with no timer;
  `message-bus.md` contains no `SpecApproved`; `docs/agent-protocol.md` exists;
  `/api/v1/personas` routes are live (`api/mod.rs:1054`); `current_task_id: Option<Id>`
  (`agent.rs:105`); `Workspace` has no `max_agent_iterations` (repo→10 direct resolution,
  `ralph_loop.rs:33/411`). The gaps the plan tasks target are real.
- Duplicate-work and dependency analysis (probe 05): existing tasks 297/298/300/303 are
  byte-unchanged with dependency sets matching the plan; `task-302` is complete as the plan
  asserts. The only cross-generation double binding of a backlog section is
  `a40446f46dbb0536f3c30ed0` — bound under the superseded audit generation (4a3f940495a8 →
  task-298's original contract) and under this plan's generation (e2624c4730e7 → task-349);
  the plan explicitly partitions ownership (task-298: code-side amendment rows; task-349:
  doc-side rows, Agent task-binding row, close-out, declaring 298/302 as dependencies), so
  there is no duplicate active assignment in the current generation. `plan_id da093bb0...`
  appears only in task-349.md and task-350.md.

Minor (non-blocking, recorded for completeness): `task-350.md`'s merge-reconciliation note
states the file "differs only in the progress field" from `d59d94b9`; in fact the Shipped
section was also rewritten to describe this round's reconciliation (that disclosure is itself
the new content, so nothing is concealed — an imprecise sentence, not a misrepresentation of
shipped work).

Verifier: nothing to execute beyond the standard clean-tree checks — the candidate changes no
compiled, scripted, or generated artifact.
