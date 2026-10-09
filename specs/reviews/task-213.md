# Review — task-213 (Repair verified failure on main 8c2d17750585)

Spec: GOAL.md — real implementations and meaningful verification. Assigned base `8c2d177505852b3e39cd77f4f782fb355de245aa`, candidate `fdb86859e5d3e76dacd06064f800deb822375799` (3 commits: `85432a3a` fix, `60668c82` process, `fdb86859` attribution record).
Verdict: **approved**.

## Scope and reproduction

- Diff base→candidate touches exactly two files: `specs/tasks/task-210.md` (+1 line: full SHA `a781ede2ea21a5153dbcb49c65771f990344a093` appended to `commits:`) and the new `specs/tasks/task-213.md`. No Rust, web, script, spec, or gate file changed. `git diff --check` clean.
- Baseline reproduction (this sandbox, base checked out): `bash scripts/check-task-commit-attribution.sh` → exit 1, `FAIL: ... a781ede2  task-210  feat(task-210): Repair verified failure on main cd1c5f044e49` (`/tmp/stage/review-evidence/attribution-base-repro.txt`). Matches the assignment's baseline log exactly.
- At candidate HEAD: same probe → exit 0, OK (`attribution-candidate.txt`).
- The repaired commit is real and product-surface: `a781ede2` touches `crates/gyre-server/src/api/admin.rs` and 21 files under `web/src` (31 files, +966/−893) — it genuinely needed attribution; the recorded SHA resolves (`git rev-parse` confirms full-SHA identity; the gate's `frontmatter_commits` truncates to 8 chars, so the full SHA matches the short form).
- The exemption file is untouched: still exactly 3 entries, matching `FROZEN_EXEMPTION_COUNT=3`. No new exemptions anywhere (`scripts/` diff base→candidate is empty). No check, skip, or gate weakened — the check script itself is byte-identical to base.

## The repair is correct, not cosmetic

- Root-cause claim independently verified: 14 prior commits recording `a781ede2` in task-210's frontmatter exist in the object store (`8d28d65e`, `6ae8207f`, `63d66b46`, `a3530260`, `4ef94106`, `e1e16020`, `b97aa290`, `69a63669`, `4dd13430`, `04ce9194`, `7523084c`, `dd9a7159`, `ce96eb64`, `60b8b2fb`) and `git merge-base --is-ancestor` confirms none is an ancestor of the candidate HEAD — each landed on unmerged checkpoint branches, exactly as the Shipped section states. The drift on main's lineage was therefore real and this branch fixes it in the lineage the gate scans.
- The task-213 `commits: []` final state is a faithful artifact of `scripts/dev-attribution.py task-213`: `85432a3a` touches only `specs/tasks/task-210.md` (no `crates/`/`web/src`/`web/tests`), and `rev-list origin/main..HEAD` is empty for the pipeline's own branch bookkeeping, so no SHA is scoping-relevant. The attribution gate enforces the same rule (only product-surface commits need recording) and passes. Mid-branch `commits: ["85432a3a"]` then `[]` matches the tool's output convention seen in prior merged tasks (e.g. task-208 `process: record task-208 branch commits`).

## Post-merge ground truth (the concurrent-duplicate question)

A duplicate repair of the same failure (`f38abb7e`, task-211) was merged to origin/main 4 minutes before this candidate was finalized, fixing the identical line in task-210.md with the identical SHA. I constructed the exact merge commit of candidate + current origin/main (`git merge-tree --write-tree` + `commit-tree`, merge commit `98791f50` in a temp worktree, since removed) and ran the real gates on it:

- `bash scripts/check-task-commit-attribution.sh` → exit 0 OK (`attribution-postmerge.txt`). Both task-210.md repairs merge to the same content; task-211.md and task-213.md coexist.
- `python3 scripts/check-rustfmt-diff.py HEAD^1` → clean (0 Rust files in the merge diff).
- No gate can fail on the duplicate: the attribution gate only requires *presence* of product-surface SHAs in frontmatter, never absence, and the exemption file is not touched by either branch.

## Other baseline-log failures (not this task's probe)

The baseline log also showed a rustfmt FAIL on `crates/gyre-server/src/api/graph.rs` changed lines. Verified this was an artifact of the superseded attempt checkout the baseline ran in, not current tree state: `python3 scripts/check-rustfmt-diff.py 8c2d177505852b3e39cd77f4f782fb355de245aa` at the candidate reports "0 Rust files checked" — the candidate changes no Rust, so no formatting debt is introduced. (task-211's merged Shipped section documents the same diagnosis independently.)

## Gates run (all at candidate HEAD, all exit 0)

check-task-commit-attribution, check-rustfmt-diff (vs assigned base), check-arch, check-hierarchy, check-abac-route-registry, check-abac-exempt-handlers, check-mcp-write-tools, check-migration-versions, check-migration-sql-portability, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-scope-literal-defaults, check-inert-enforcement, check-forged-scope-fields, check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http, check-mem-port-contracts, check-commit-msg (fix commit). Evidence under `/tmp/stage/review-evidence/`.

## Concurrency note (not a defect, no action required)

task-213 and the already-merged task-211 (`f38abb7e` on origin/main) are duplicate repairs of the same baseline failure — the pipeline double-dispatched the same assignment. Both are individually correct and mutually compatible (verified post-merge above). Whichever merges second carries the other's file content unchanged. This is process redundancy, not a code defect; flagging it so the verifier doesn't mistake the duplicate task file for drift.

## Verifier / host checks (sandbox cannot run these)

Sandbox transport probe is unsupported (`accept` errno 95 per `/tmp/stage/capabilities.json`) — no server/browser probes, and per the dev-check harness the full Rust test suite runs on the host (OpenShell cannot accept loopback sockets even for library tests). Required on host/CI: full `bash tools/checks.sh`, `cargo test --all`, `cd web && npm ci && npm test`, and GitHub checks on the merge SHA. Given the candidate is specs-only with byte-identical product tree to base, the Rust/web suites exercise identical code to already-green main; the attribution gate (the actual probe of record) is proven green on the exact merge SHA in this review.

Commit message format note: `60668c82` (`process(task-213): ...`) and `fdb86859` (`process: record ...`) do not match `scripts/check-commit-msg.sh`'s conventional pattern, but `process:`-prefixed subjects are the established pipeline convention throughout main's history (dozens of merged examples: `f32791e0`, `d5ed37a0`, `4b36cef4`, and every `process: record task-NNN branch commits` commit) and the hook is not applied to these pipeline-authored commits; `85432a3a` (the substantive fix) passes the linter.
