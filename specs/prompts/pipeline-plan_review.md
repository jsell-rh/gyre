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

An intentional code-versus-spec deviation is still an implementation gap. Existing
code comments, rationale, or a defensible design choice are not a spec amendment.
A spec conflict needs incompatible canonical requirements or concrete evidence that
the contract itself cannot be satisfied; current code is not a competing requirement.
Never classify a normative or security requirement as non-blocking merely because
the current behavior seems reasonable. Require a faithful repair or report the
conflict so a human can decide through the spec lifecycle.

Approval requires findings: []. If findings remain unresolved, set approved: false
and retain actionable findings; do not delete them just to satisfy the schema.
Purely editorial observations that do not affect any requirement may go in checks.

other_plans in the machine context is a compact index. Each path points to the
complete staged plan JSON, with its plan_id and source-file list. Search/read related
plan artifacts before concluding that work is independent; an unread index entry is
not evidence of non-overlap. Completed assignments are omitted from this active index.
