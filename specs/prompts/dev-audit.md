# Bounded fidelity audit

Read AGENTS.md and the assigned spec sections. The staged audit contract is
authoritative for scope and task ID reservations. Inspect actual entry points,
storage, enforcement, call sites and existing tests. A completed task or a green
test with no meaningful assertion is not evidence that a requirement works.

Edit only the assigned coverage matrix, this audit task and its review/evidence
files, existing task files referenced by the assigned rows, and new task files
using the reserved IDs. Do not edit production code, requirements, gates or the
coverage summary (the integrator regenerates that summary).

For missing behavior, reopen an existing task as `needs-revision`, or create a
reserved task with precise production acceptance criteria, spec references and
dependencies. Change the row to `task-assigned`, referencing those tasks. Leave
the gap open. Do not create a redundant implementation task for behavior that
already works, and do not add tests for already-working behavior.

Write `specs/reviews/audit-TASK.json`, replacing TASK with the assigned task ID:

```json
{
  "version": 1,
  "task": "task-000",
  "code_generation": "the exact value from the staged contract",
  "findings": [
    {
      "row": 1,
      "status": "verified",
      "explanation": "What the entry point actually does, including failure cases",
      "production": [{"path": "crates/.../src/example.rs", "symbol": "actual_function"}],
      "probes": [{"argv": ["cargo", "test", "-p", "actual-crate", "actual_test"], "cwd": ".", "timeout": 300}]
    },
    {"row": 2, "status": "gap", "explanation": "Concrete missing behavior", "tasks": ["task-001"]},
    {"row": 3, "status": "n/a", "explanation": "Why this section has no implementable requirement"}
  ]
}
```

Include exactly the assigned rows. A verified row needs real production paths
and bounded, reproducible acceptance probes. Use existing meaningful tests or
repository scripts. Never invent evidence. If existing verification cannot
establish the requirement, keep that verification gap open in an implementation
task. Probes accept `cargo test`, `npm test`, or `python3`/`node`/`bash` followed
by a repository script; paths and working directories must remain in the repo.

Run `python3 /tmp/stage/dev-audit-check.py /tmp/stage/audit-contract.json --base origin/main --replay`.
When the scoped assessment and follow-up tasks are ready, set the audit task to
`ready-for-review`. The next round independently reviews it. Summarize a few
concrete findings in the task file; keep detailed evidence in the JSON record.
