Independently review the spec audit and decomposition in Planning context. Inspect the
actual code at the assigned base and read the full canonical spec yourself. Do not
trust the auditor's evidence, planner's claims, coverage labels or completed tasks.
This assignment is read-only: do not change tracked files or run tests/builds.

Challenge EVERY section, including satisfied/context decisions: look for stubs,
inert enforcement, omitted integration, adapter divergence, fabricated completion,
missing error paths, and requirements improperly dismissed as rationale. Challenge
new tasks for duplicate active or pending planned work, wrong dependencies, weakened requirements,
test-only busywork, missing gaps and unverifiable acceptance criteria. Reused tasks
must actually cover their assigned gaps. No-gap audits require the same scrutiny.
For rejected work give concrete correction instructions; do not quietly repair it.

Write /tmp/stage/planning.json:
{"plan":"EXACT PLAN DIGEST from Exact assignment", "approved":true,
 "findings":[], "checks":[{"id":"EVERY EXACT HEADING ID",
 "detail":"Independent evidence and reasoning confirming or rejecting this section's disposition and plan"}]}

Reject when any requirement is omitted or evidence is not sufficient. Approval means
only that the audit/plan is sound; it does not implement a spec, verify coverage,
merge a PR or authorize requirements changes.

Inspect other_plans too. If two plans propose equivalent work, prefer the earlier
audit (then lexicographically earlier document for a tie); reject overlapping
new ownership with specific references so the later planner can reuse the work
once its task definitions merge. Do not silently add dependencies to reused tasks.

Read /tmp/stage/planning-context.json for machine-readable inputs and any retained
previous_report. If previous_report is supplied, it is an untrusted draft from these exact inputs.
Correct the validator error and check affected conclusions first. Preserve useful
work rather than repeating broad exploration. It is not evidence of approval.
