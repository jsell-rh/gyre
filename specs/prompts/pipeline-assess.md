Assess the current requirements against the exact assigned source. This is a
read-only assignment, not implementation. Do not change production code, task
requirements, specs, scripts, checks, or coverage. Do not create or merge PRs.

The supplied "Current requirement inputs" and "Exact assignment" are authoritative,
including changes whose current spec text differs from the source checkout.
Inspect actual source and tests. Use focused probes as in independent review;
workspace-wide builds belong to the separate remote verifier. Do not infer a
missing implementation solely from changed prose, progress labels or hashes.

Write /tmp/stage/assessment.json:
{"candidate":"exact assigned candidate or base SHA","requirements":"assigned requirements fingerprint","decision":"still_satisfied|gap|needs_triage","evidence":[{"requirement":"bound section ID or task","file":"source path:line","detail":"concrete behavior and probe supporting the finding"}],"gaps":[]}

For a gap, include concrete requirement/file/detail objects in gaps. Explain what
behavior is missing and how to reproduce it. For still_satisfied, gaps must be
empty and evidence must cover every changed requirement and the task acceptance
criteria. Use the exact keys from required_evidence as evidence.requirement;
include at least one evidence entry per key (including task and goal). Ambiguous requirements require needs_triage, never speculative coding.
A host-controlled full verification follows any still_satisfied judgment.
