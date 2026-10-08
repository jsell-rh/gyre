# Independent fidelity review

Independently inspect the staged contract, assigned sections, real production
entry points, changed coverage claims and the bounded JSON evidence record.
Run the declared probes. Check that they exercise meaningful behavior and can
fail if it breaks. A successful command, a matching symbol or a prior completion
claim alone does not prove fidelity. Check negative/enforcement paths where the
spec requires them. Reject hollow claims and tests that merely mirror code.

Every open gap must have a precise, incomplete implementation task. Existing
tasks may be reopened; new IDs must come from the staged reservations. Do not
claim product completion merely because this assessment is done. Audit work is
limited to its assigned metadata scope; leave production changes to follow-up
implementation tasks.

Write concrete review findings to `specs/reviews/TASK-review.md`, replacing TASK
with the assigned task ID; reference it in the task's `review:` frontmatter.
Set `needs-revision` if the evidence or follow-up tasks are inadequate. Only when
the scoped assessment is sound, set this audit task to `complete` and add a
`## Shipped` section stating the audited sections, proven findings and open gaps.
Do not imply that gaps identified by the assessment have been implemented.
