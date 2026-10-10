# Review — task-233 (Repair verified failure on main 27bd585ca7eb)

Task: repair the verified upstream failure of `scripts/check-task-commit-attribution.sh` on base `27bd585ca7eb429905ccbded1f48b4d0167c0c20` — commit `27bd585c` (`feat(task-155): Implement gyre search CLI command`) touches product surface (`crates/gyre-cli/src/client.rs`, `crates/gyre-cli/src/main.rs`) but was absent from `specs/tasks/task-155.md`'s `commits:` frontmatter.
Commit under review: `030a7ad2` (candidate, sole commit over the assigned base).
Verdict: **approved**.

## Round 1

Probes (all from `/tmp/gyre` at candidate HEAD `030a7ad2`; evidence under `/tmp/stage/review-evidence/`):

- Reproduction (base state): reverted `specs/tasks/task-155.md` to the base blob and ran `bash scripts/check-task-commit-attribution.sh` → **exit 1**, identical violation text (`27bd585c task-155 ...`) to the recorded baseline failure. Evidence: `r233-repro-before.txt`.
- Candidate state: `bash scripts/check-task-commit-attribution.sh` at `030a7ad2` → **exit 0** ("OK: every task-labeled product-surface commit is recorded ..."). Evidence: `r233-after.txt`.
- Mutation (test-the-repair): removed `, "27bd585ca7eb429905ccbded1f48b4d0167c0c20"` from the frontmatter list → check **fails again, exit 1, identical violation**; restored the candidate blob → **exit 0** again, tree byte-identical to the candidate (sha256 match against `git show 030a7ad2:specs/tasks/task-155.md`). Evidence: `r233-mutation.txt`. The pass is attributable to the recorded SHA, not to gate drift or environmental luck.

Diff review (`git diff 27bd585c 030a7ad2`, 2 files, +106/−1):

- `specs/tasks/task-155.md`: exactly one line changed — the base commit's full SHA appended to the existing 7-entry `commits:` array. Full SHA matches `git rev-parse 27bd585c` exactly; list still parses as JSON, 8 unique entries. This is the check's own documented remedy (script header: "Fix drift by adding the SHA to the task's frontmatter"), the same repair shape as the previously approved task-224 (`a11ba8d3`) and task-228 (`05709c24`) rounds.
- `specs/tasks/task-233.md`: new task file only — required behavior, recorded baseline failure, shipped notes. No production content.

No weakening, no exemptions, no scope creep:

- `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); `FROZEN_EXEMPTION_COUNT=3` unchanged.
- `git diff 27bd585c 030a7ad2 -- scripts/ crates/ web/ docs/` is empty — no check, source, or doc surface changed. No skip, no `|| true`, no `continue` short-circuit, no exemption growth; the only way this check passes now is that the drift is genuinely recorded.
- No blocked feature was implemented (out of scope by task contract; none needed).

Attribution consistency:

- `python3 scripts/dev-attribution.py task-233` derives an empty hash list — the round changes only `specs/tasks/` records, and the candidate commit `030a7ad2` touches no `crates/|web/src|web/tests` path (verified via `git show --name-only`). `commits: []` in `specs/tasks/task-233.md` is attribution-canonical, matching the `implement(task-233):` subject label against zero product-surface commits.
- The candidate commit is labeled `implement(task-233):` and touches only `specs/tasks/`, so it does not trip the attribution check itself (product-surface filter, script line 114).

Findings: none. The repair is minimal (one frontmatter line), the exact remedy the failing check prescribes, and the mutation probe proves the fixed gate actually bites.

Verification notes for the verifier: the check needs full git history (`fetch-depth: 0`); it was run from a full clone here. No server/browser probe is relevant — the change touches no runtime surface. Full workspace suites and GitHub checks remain owned by the verification stage.
