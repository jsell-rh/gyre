# Implementation round

Implement the assigned task against its cited spec. Read the relevant spec
section and surrounding code before editing. Treat task prose as a plan, and
the spec as the contract. Inspect existing behavior and tests; make the
smallest production change that satisfies the requirement.

Use real storage, authorization, side effects, and failure handling. Follow
AGENTS.md and the repository's port boundaries. Add a focused regression test
when it can fail on the old behavior. Run focused checks while working. The
controller runs the full deterministic gates on the integrated commit.

If an existing review reports defects, address each concrete finding. Record
task-labeled product commits in the task's `commits:` field. When implementation
is ready for an independent review, set `progress: ready-for-review`; never set
`complete` in an implementation round. Explain any unresolved gap in the task.
