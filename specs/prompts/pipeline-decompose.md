Decompose the confirmed gaps in Planning context into a bounded, dependency-aware plan.
This is read-only planning: do not edit tracked files, specs, coverage or code and do
not run tests/builds. Read the canonical spec and evidence; preserve its full behavior.

First inspect the supplied task catalog, other pending plans and relevant task documents. Reuse an unfinished
existing task only if its actual contract covers the gap. A finished task is historical
context, not proof the behavior works. If it missed behavior, propose a narrowly scoped
delta task rather than replaying the entire original implementation. Avoid overlapping
active work. Partition genuinely independent capabilities; dependencies must express
real requirements, not arbitrary serialization. No speculative features or test-only
busywork. A negative probe must describe a meaningful failure that proves enforcement
or real behavior, rather than asserting that a symbol exists.

Write /tmp/stage/planning.json:
{"audit":"SHA256 DIGEST OF SUPPLIED AUDIT (provided in Exact assignment)",
 "tasks":[{"key":"stable-short-key", "title":"Concrete deliverable",
 "covers":["gap heading IDs, each owned exactly once across tasks"],
 "required_behavior":"Precise behavior faithful to the canonical spec, including enforcement",
 "acceptance":["Concrete observable requirement"],
 "negative_probe":"A targeted probe and the real defect that must make it fail",
 "dependencies":["another local key or existing task-NNN"],
 "existing_task":null}]}

Use 1–8 tasks. For reused tasks set existing_task to its name; otherwise null. Every
confirmed gap must be covered exactly once. Do not mark tasks complete, amend specs,
weaken acceptance criteria or make production/coverage changes.

A reused task keeps its current dependencies. Do not specify new dependency edges
for it as though this plan could silently modify an existing task contract.

Read /tmp/stage/planning-context.json for machine-readable inputs and any retained
previous_report. If previous_report is supplied, it is an untrusted draft from these exact inputs.
Correct the validator error and check affected conclusions first. Preserve useful
work rather than repeating broad exploration. It is not evidence of approval.

The machine context may also contain revision_draft: one unapproved prior audit and
plan plus separate review_feedback. Use it as a correction starting point, not proof.
Recheck the affected conclusions and evidence against this exact assigned base,
retain sound work, and account for every requirement; do not repeat broad exploration
when the cited source is unchanged. The controller validates all citations again.

other_plans in the machine context is a compact index. Each path points to the
complete staged plan JSON, with its plan_id and source-file list. Search/read related
plan artifacts before concluding that work is independent; an unread index entry is
not evidence of non-overlap. Completed assignments are omitted from this active index.
