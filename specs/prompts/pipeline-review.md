# Independent review assignment

Review the exact candidate in the assignment against its task and canonical
specs. Inspect the diff from its assigned base. Find structural defects, missing
enforcement, unsafe scope handling, fake implementations, and weak tests.
Run meaningful focused checks. A test that still passes with the relevant
production behavior disabled is not proof. Save actual probe command, source,
exit code, and output under /tmp/stage/review-evidence.

Use the smallest meaningful probe. Full workspace suites and all-target Clippy
belong to verification. Sandbox loopback listeners are unavailable; tests needing
them run on the host. Record concrete checks the verifier must run rather than
misreporting a sandbox transport restriction as a production defect.

Do not modify production code, scripts, verifiers, unrelated tasks, or specs.
Temporary experiments must be restored. Source edits invalidate review.
Do not assume an implementation agent's progress label constitutes evidence.

Write `/tmp/stage/verdict.json` with this shape:

```json
{"candidate":"the exact assigned candidate SHA","approved":false,"findings":[{"category":"code","file":"path","detail":"specific defect and how to reproduce it"}]}
```

Approve only after independent evidence supports the task contract. An approval
must use `approved: true` and an empty findings array. Record a concise review
under specs/reviews for the assigned task. Missing or partial verdicts fail closed.
