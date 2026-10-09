# Implementation round

Implement the assigned task against its cited spec. Read the relevant spec
section and surrounding code before editing. Treat task prose as a plan, and
the spec as the contract. Inspect existing behavior and tests; make the
smallest production change that satisfies the requirement.

Use real storage, authorization, side effects, and failure handling. Follow
AGENTS.md and the repository's port boundaries. Add a focused regression test
when it can fail on the old behavior. Run focused checks while working. The
controller runs the full deterministic gates on the integrated commit.
Commit attribution is refreshed mechanically after rebases and checkpoints;
inspect the current diff instead of reconstructing old commit-hash mappings.
Leave `commits:` frontmatter to the controller's separate bookkeeping commits;
never amend a product commit to insert its own SHA.
OpenShell disallows loopback listeners. Leave the full server test suite and
lint suites to the controller; run focused probes that work in this sandbox.

If an existing review reports defects, address each concrete finding. Record
integration and full-suite failures from the supplied repair handoff as
concrete findings too. Reproduce where the sandbox supports the probe; supplied
exact-SHA logs establish failures that require host or GitHub execution.
Once current code and a failing assertion identify the defect, make the repair;
trace historical commits only when the task requires provenance. Compare with
current main for unrelated failures. Use an isolated worktree for baseline probes;
never stash or reset the assigned checkout to run them. Preserve the spec and
verification gates.
Do not silence failures by deleting tests, weakening checks, growing
exemption files, or marking unfinished requirements complete. When implementation
is ready for an independent review, set `progress: ready-for-review`; never set
`complete` in an implementation round. Explain any unresolved gap in the task.
Once the focused checks pass and the task is ready for review, end the round
with a concise result so the independent reviewer can take over.
