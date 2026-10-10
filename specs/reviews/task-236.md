# Review — task-236 (Repair verified failure on main 27bd585ca7eb)

Task: `specs/tasks/task-236.md` — repair the verified baseline failure of `scripts/check-task-commit-attribution.sh` on main `27bd585ca7eb429905ccbded1f48b4d0167c0c20` (violation: `27bd585c task-155 feat(task-155): Implement gyre search CLI command`).
Candidate under review: `b26508e1c46c04b447014a302ae1f4adee3d5375` (base `27bd585ca7eb429905ccbded1f48b4d0167c0c20`).
Verdict: **approved**.

## Round 1

### Diff scope

`git diff base..candidate` touches exactly two files: `specs/tasks/task-155.md` (1 line — appends the full landing SHA `27bd585ca7eb429905ccbded1f48b4d0167c0c20` to the existing 7-entry `commits:` inline array, which remains valid JSON, parses to 8×40-hex entries) and `specs/tasks/task-236.md` (new task file). Zero changes to `crates/`, `web/`, or `scripts/` (`git diff base..candidate -- crates/ web/ scripts/` is empty; `git diff --check` clean). No exemption file touched; `scripts/task-commit-attribution-exemptions.txt` remains frozen at its 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`).

### Baseline failure reproduced (independent)

Isolated worktree at base `27bd585ca7eb429905ccbded1f48b4d0167c0c20`: `bash scripts/check-task-commit-attribution.sh` → exit 1 with the exact recorded violation `27bd585c task-155` (evidence: `task-236-review-base-repro.txt`).

### Root cause confirmed (independent)

- Landing commit `27bd585c` touches product surface: `crates/gyre-cli/src/client.rs`, `crates/gyre-cli/src/main.rs`, `docs/cli.md`, `specs/tasks/task-155.md` (`git show --name-only`).
- All 7 SHAs previously recorded in task-155's frontmatter exist in the repo but **none is an ancestor of HEAD** (verified individually with `git merge-base --is-ancestor`): subjects `fix(task-155)`, `feat(cli)`, `checkpoint`, `wip(task-155)` — pipeline-branch commits squashed into the landing commit. Squash-drift confirmed: the landing surface was invisible to review scoping (task-095 R3-F4 flaw class).
- Landed surface content-identical to recorded branch tip `4c0df440` on all three product files (`git diff --quiet 4c0df440 27bd585c -- <each file>` → identical), so recording the landing SHA preserves the review-scope link to the same code that was reviewed.

### Repair verified (independent)

At candidate HEAD: `bash scripts/check-task-commit-attribution.sh` → exit 0, `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` (evidence: `task-236-review-candidate-pass.txt`).

**Mutation check (pass is attributable to the repair, not gate drift):** in the isolated worktree at candidate, `sed`-removing the recorded SHA from `specs/tasks/task-155.md` → check FAILs with the identical baseline violation, exit 1 (evidence: `task-236-review-mutation.txt`); restoring the file → exit 0 again (evidence: `task-236-review-restore.txt`). The check's matcher (`frontmatter_commits`, line 96: `grep -oE '[0-9a-f]{7,40}' | cut -c1-8`) matches the appended 40-char SHA as `27bd585c`, satisfying the `^27bd585c$` frontmatter grep at line 123.

### Secondary claims verified (independent)

- The baseline log's `new verification exemptions forbidden: scripts/scope-literal-defaults-exemptions.txt` line does **not** reproduce on main's history: `git diff 06d70009 27bd585c -- scripts/scope-literal-defaults-exemptions.txt` is empty; the `lib.rs:494/538` entries exist only on commits that are not ancestors of HEAD (`38c2c5e7`, `ba78ab2a`, `c83aaf8a`, `1327be03`, `1ee8221e` — all `NOT ancestor of HEAD`); `bash scripts/check-scope-literal-defaults.sh` → exit 0 on HEAD. Correctly not "repaired" — repairing a non-live gate output would have been noise.
- Prior parallel attempt `74bf94b1` (task-235) performed the identical repair but is not an ancestor of HEAD, and `specs/tasks/task-235.md` does not exist on main — re-issuing under task-236 is the correct path, not a duplicate landing.
- Task-236's own `commits: []` is attribution-canonical: `python3 scripts/dev-attribution.py task-236` → empty list, exit 0. The candidate's only delta is `specs/tasks/task-155.md` (a task file, not product surface — exempt from attribution) plus its own task file. Consistent with the convention of all prior repair tasks (task-228/227/224/219 all ship `commits: []`).

### Task contract compliance

- **Real repair, not weakening:** the repair is exactly the check's own documented remedy (script header lines 27-30: "Fix drift by adding the SHA to the task's frontmatter... not by growing the file"). No exemptions added, no check/skip/gate weakened, no frozen count raised.
- **Shape precedent:** identical repair shape to landed ancestors `06d70009` (task-228, task-196 drift), `18c44f1a` (task-227), `770785f7` (task-224, task-068 drift), `e96d25ab` (task-219, task-200 drift) — all touched the drifted task file + their own task file, all ancestors of HEAD.
- **No consumer regression:** the only gate consuming `specs/tasks/` frontmatter is the attribution check itself; `bash scripts/stats.sh` → exit 0; frontmatter still parses as valid JSON inline array. Since `crates/`/`web/`/`scripts/` are untouched, rustfmt/clippy changed-line gates and all static gates are trivially unaffected (they diff against `HEAD^1` = base, zero changed lines in scope).

### Notes (non-blocking)

- The implementer's claimed evidence paths (`/tmp/stage/review-evidence/task-236-attribution-*.txt`) do not exist in this review sandbox — they lived in the implementation agent's own environment. Not a code defect: every material claim was independently re-probed and reproduced in this review (baseline repro, post-repair pass, mutation/restore, ancestor checks, content identity), with evidence saved as `task-236-review-*.txt`.
- No server/browser probe is required for this specs-only repair; `capabilities.json`'s TCP-listener restriction is irrelevant here.

## Verdict

The verified baseline failure is genuinely repaired by a minimal, correct, precedent-shaped change: the landing commit `27bd585c` is now recorded in task-155's `commits:` frontmatter, making the landed surface visible to review scoping. The check passes at the candidate, the pass is attributable to the recorded SHA (mutation kills it), no gate was weakened, and no other surface changed. Independent evidence supports every task-contract claim.
