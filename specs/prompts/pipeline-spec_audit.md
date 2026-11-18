Audit the ENTIRE assigned canonical specification against the exact assigned base.
This is source inspection, not implementation. Do not edit tracked files, progress,
coverage, requirements or guards. Do not build or run tests, and do not propose
new tests for already-working behavior. Read all headings, not just coverage rows.
Coverage labels and historical task completion are hints, not implementation proof.

For every heading ID in Planning context, inspect real production wiring, enforcement,
storage implementations and error paths. Include stubs, missing calls, audit-only
checks where enforcement is required, and hardcoded stand-ins as gaps. Read parent
introductions and related canonical specs; never silently resolve a contradiction.
Code that contradicts a canonical requirement is an implementation gap, even when
intentional. A spec conflict needs incompatible canonical requirements or concrete
evidence that the contract cannot be satisfied; existing code rationale is not enough.
Use context for rationale-only headings; do not call normative requirements context.
Avoid counting a requirement again under a parent if its children account for it.

Write /tmp/stage/planning.json:
{"base":"EXACT ASSIGNED BASE", "requirements":"EXACT REQUIREMENTS FINGERPRINT",
 "sections":[{"id":"EXACT HEADING ID", "decision":"gap|satisfied|context|conflict",
 "requirement_quote":"literal quotation from this spec section",
 "detail":"what is required and what actual code does or fails to do",
 "evidence":[{"file":"repository/relative/source/path", "quote":"literal source excerpt",
 "detail":"why this concrete source proves the conclusion; for absence explain inspected call paths"}]}]}

Account for every ID exactly once. Pure context may have an empty evidence array;
its literal canonical quote and rationale are independently reviewed. Behavior
claims still require concrete inspected production source. Inspect implementations rather than trusting tests
that mirror them. A satisfied claim requires actual production behavior; a gap requires
specific missing behavior, not a vague desire for more tests. Do not claim the audit
implements anything or mark coverage complete. The report is independently reviewed.

Read /tmp/stage/planning-context.json for machine-readable inputs and any retained
previous_report. If previous_report is supplied, it is an untrusted draft from these exact inputs.
Correct the validator error and check affected conclusions first. Preserve useful
work rather than repeating broad exploration. It is not evidence of approval.

Before finishing, run `python3 /tmp/stage/pipeline-evidence.py /tmp/stage/planning.json`
from the checkout. Correct every reported literal-citation error. This is the same
source-citation validator the controller uses, not a product build or test.
Do not bypass or modify it; the controller validates again independently.

The machine context may also contain revision_draft: one unapproved prior audit and
plan plus separate review_feedback. Use it as a correction starting point, not proof.
Recheck the affected conclusions and evidence against this exact assigned base,
retain sound work, and account for every requirement; do not repeat broad exploration
when the cited source is unchanged. The controller validates all citations again.

other_plans in the machine context is a compact index. Each path points to the
complete staged plan JSON, with its plan_id and source-file list. Search/read related
plan artifacts before concluding that work is independent; an unread index entry is
not evidence of non-overlap. Completed assignments are omitted from this active index.
