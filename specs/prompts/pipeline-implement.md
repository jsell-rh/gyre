# Implementation assignment

Implement the assigned task in production code. Read its referenced specs and
relevant development docs. The task contract and durable findings are the input;
do not reproduce old diagnostic ledgers in the task or prompt.

Resolve an active merge or rebase first. Repair concrete review, verification, or CI
findings. Preserve useful source checkpoints. Run focused tests that distinguish
correct behavior from a real defect. Save expensive probe commands, source SHAs,
exit codes, and output under /tmp/stage/review-evidence so they can be retained.
Do not weaken verifiers, add exemptions, inflate tests, or claim fake completion.

For CI repair, use the current PR head and run IDs in the durable finding.
Historical task review notes and upstream baseline logs are context, not the
current failing test list. Inspect current failure artifacts before changing
visual snapshots; preserve the specified UI and existing assertion strength.

Full workspace suites, architecture checks, all-target Clippy, and GitHub CI are
owned by verification and publication. Use the smallest relevant probe here.
Do not repeat full gates inside this assignment. If a focused probe needs a
listener, first check whether this sandbox permits it. Record any actual transport
restriction for host verification; do not infer a code defect from that restriction.

When implementation is ready, set the task's progress to `ready-for-review` and
write a concise `## Shipped` explanation of actual behavior and test evidence.
An independent reviewer and deterministic gates decide approval. If unfinished,
leave truthful progress and specific remaining work. The executor checkpoints
and publishes the source when this single assignment ends.
