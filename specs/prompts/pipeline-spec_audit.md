Audit the ENTIRE assigned canonical specification against the exact assigned base.
This is source inspection, not implementation. Do not edit tracked files, progress,
coverage, requirements or guards. Do not build or run tests, and do not propose
new tests for already-working behavior. Read all headings, not just coverage rows.
Coverage labels and historical task completion are hints, not implementation proof.

For every heading ID in Planning context, inspect real production wiring, enforcement,
storage implementations and error paths. Include stubs, missing calls, audit-only
checks where enforcement is required, and hardcoded stand-ins as gaps. Read parent
introductions and related canonical specs; never silently resolve a contradiction.
Use context for rationale-only headings; do not call normative requirements context.
Avoid counting a requirement again under a parent if its children account for it.

Write /tmp/stage/planning.json:
{"base":"EXACT ASSIGNED BASE", "requirements":"EXACT REQUIREMENTS FINGERPRINT",
 "sections":[{"id":"EXACT HEADING ID", "decision":"gap|satisfied|context|conflict",
 "requirement_quote":"literal quotation from this spec section",
 "detail":"what is required and what actual code does or fails to do",
 "evidence":[{"file":"repository/relative/source/path", "quote":"literal source excerpt",
 "detail":"why this concrete source proves the conclusion; for absence explain inspected call paths"}]}]}

Account for every ID exactly once. Inspect implementations rather than trusting tests
that mirror them. A satisfied claim requires actual production behavior; a gap requires
specific missing behavior, not a vague desire for more tests. Do not claim the audit
implements anything or mark coverage complete. The report is independently reviewed.
