# Implementation assignment

Implement the assigned task in production code. Read its referenced specs and
relevant development docs. The task contract and durable findings are the input;
do not reproduce old diagnostic ledgers in the task or prompt.

Resolve an active merge or rebase first. Repair concrete review, verification, or CI
findings. Read `/tmp/stage/findings.log` when present for retained verification
and CI evidence; controller artifact paths in findings are not sandbox paths.
Preserve useful source checkpoints. Run focused tests that distinguish
correct behavior from a real defect. Save expensive probe commands, source SHAs,
exit codes, and output under /tmp/stage/review-evidence so they can be retained.
Do not weaken verifiers, add exemptions, inflate tests, or claim fake completion.

For CI repair, use the current PR head and run IDs in the durable finding.
Historical task review notes and upstream baseline logs are context, not the
current failing test list. Inspect current failure artifacts before changing
visual snapshots; preserve the specified UI and existing assertion strength.
Inspect staged CI artifacts before attempting local browser setup. Avoid a cold
release build of the whole server merely to investigate screenshot differences;
exact-head GitHub checks remain mandatory after independent review.

Full workspace suites, architecture checks, all-target Clippy, and GitHub CI are
owned by verification and publication. Use the smallest relevant probe here.
Install frontend dependencies with `npm ci` before running repository test tools;
use the locked versions rather than letting `npx` download an unrelated release.
Do not repeat full gates inside this assignment. If a focused probe needs a
listener, first check whether this sandbox permits it. Record any actual transport
restriction for remote verification; do not infer a code defect from that restriction.

When implementation is ready, set the task's progress to `ready-for-review` and
write a concise `## Shipped` explanation of actual behavior and test evidence.
Keep that heading exact and use `###` for any subheadings. Replace older shipped
notes rather than appending diagnostic ledgers or new top-level sections.
An independent reviewer and deterministic gates decide approval. If unfinished,
leave truthful progress and specific remaining work. The executor checkpoints
and publishes the source when this single assignment ends.

Large gyre-server builds/test compilation must run in the independent verifier,
after the model exits, to fit the hard 4 GiB budget. Use cargo check for server
type checking and focused tests of smaller explicit packages. Record the server
tests the verifier must execute. Do not bypass the build wrapper.
