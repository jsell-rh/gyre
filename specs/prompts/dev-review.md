# Task review round

Review the assigned task's changed code against the cited spec and its review
history. Inspect the actual diff and call sites. Check whether the behavior is
enforced, scoped, durable, and wired through the real entry point. Look for
tests that could pass without the behavior. Run focused probes where they can
settle a material doubt. The controller separately runs deterministic gates
on the eventual integration commit.
Use the supplied current review comparison base. Commit attribution is refreshed
and checked mechanically; reconstruct historical rebases only when a concrete
behavioral finding requires it. Evaluate current behavior and focused regression
evidence rather than repeating unchanged, previously supported findings.
OpenShell disallows loopback listeners. Use supplied exact-SHA logs for failures
that require a running server or browser; the host and GitHub run those gates.
Do not install browsers or repeat full suites here to reproduce those failures.
Give isolated worktrees their own CARGO_TARGET_DIR. Sharing it can reuse another
checkout's test binary and make a mutation pass or a repair appear to fail.
For expensive probes, save the command, source revision and diff, working
directory, output, and actual exit status under /tmp/stage/review-evidence/.
Background job handles disappear when the model session ends. On a resumed
round, inspect persisted results before starting another build; reuse evidence
only after confirming the relevant source is unchanged. Run mutations in an
isolated worktree and restore them even when the command is interrupted.
Sandboxes have a bounded build lane to fit their memory limit. Run good and
mutant builds sequentially in the same isolated worktree and its private target
directory, changing and restoring the actual source between runs. Do not launch
multiple empty Cargo targets or frontend suites in parallel inside one sandbox.

If a repair handoff is supplied, independently check each reported failure
and its reproduction. Reject gate weakening, deleted meaningful tests, new
exemptions, and unrelated changes made just to obtain a passing result.
Edit only the assigned task and its review findings. Preserve the code and
verifiers under review. If they require changes, record `needs-revision` so
the implementation role repairs them before another independent review.

Write concrete findings with file and behavior evidence to the task's review
file. If any material gap remains, set `progress: needs-revision` and leave a
clear repair path. Set `progress: complete` only when the task really meets the
spec. Do not add generic checklist prose to prompts; turn mechanically
detectable failure classes into scripts or focused tests.

Before marking a task complete, add a `## Shipped` section to its task file
with 2–4 concise bullets describing the behavior actually delivered. Base
these on the reviewed code; this section becomes the GitHub merge description.
Once the evidence supports a verdict, write the findings and task status and
end the round. Leave the full integration verification to the controller.
