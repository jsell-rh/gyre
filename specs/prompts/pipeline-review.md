# Independent review assignment

Review the exact candidate in the assignment against its task and canonical
specs. Inspect the diff from its assigned base. Find structural defects, missing
enforcement, unsafe scope handling, fake implementations, and weak tests.
Run meaningful focused checks. A test that still passes with the relevant
production behavior disabled is not proof. Save actual probe command, source,
exit code, and output under /tmp/stage/review-evidence.

If `/tmp/stage/review-resume.json` exists, read it first. It identifies saved
notes and probe evidence from an interrupted review of this exact candidate.
Continue from those artifacts after checking their relevance; no previous
approval is carried forward. Save the review record incrementally, including
unfinished probes, so interruption does not erase useful work.

Use the smallest meaningful probe. Full workspace suites and all-target Clippy
belong to verification. If a focused probe needs a listener, check whether the
current sandbox permits it. Record concrete checks the verifier must run if an
actual transport restriction blocks the probe. A restriction is not a code defect.
Use `npm ci` before frontend probes so repository tools use the locked versions.
For CI repair, inspect the retained original failure logs and artifacts under
`/tmp/stage` and compare them with the exact candidate changes. Avoid a cold
release build merely to inspect screenshots. If a server probe is needed after
focused Rust tests, use the available debug build dependencies and rebuild the
current web source; the older committed web bundle cannot prove current UI code.
Use `/tmp/stage/bin/cargo` for Rust probes; workspace-wide invocations are
rejected by the executor's wrapper. Do not bypass it through an absolute Cargo
path. A cold focused compilation may take time; keep evidence of what ran.

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

Large gyre-server builds/test compilation must run in the independent verifier,
after the model exits, to fit the hard 4 GiB budget. Use cargo check for server
type checking and focused tests of smaller explicit packages. Record the server
tests the verifier must execute. Do not bypass the build wrapper.

Generated `web/dist/` is owned by the deterministic verifier, not implementation
or review. It may intentionally match the upstream bundle in this checkpoint.
Review the authored frontend source and tests. Do not reject for stale/missing
bundle regeneration, generated merge conflicts, or lack of an agent-run build.
The verifier rebuilds remotely after integration, runs all checks, commits the
resulting assets, and binds publication to that exact tested tree. Record any
pure generated-output observation separately; real source defects remain blocking.
