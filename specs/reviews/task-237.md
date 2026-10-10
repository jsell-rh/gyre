# Review — task-237: Repair verified failure on main 27bd585ca7eb

Candidate: `ffd7b3124437e8d9cb32a903f343ac819c96941c` (single commit on base
`27bd585ca7eb429905ccbded1f48b4d0167c0c20`). Diff: `specs/tasks/task-155.md`
+1/-1 line, `specs/tasks/task-237.md` new (+108). No `crates/`, `web/`,
`scripts/`, `.github/`, or `.pre-commit-config.yaml` changes
(`git diff 27bd585c ffd7b312 -- scripts/ .pre-commit-config.yaml .github/` is
empty).

Verdict: **approved**.

## What the baseline failure was

`GYRE_BASELINE_FAILURE_JSON` names probe `bash
scripts/check-task-commit-attribution.sh`. The gate requires every non-merge
commit whose subject labels `task-NNN` and whose fileset touches product
surface (`crates/|web/src|web/tests`) to have its short SHA recorded in that
task's `commits:` frontmatter (or the frozen 3-entry exemption file).
`27bd585c` ("feat(task-155): Implement gyre search CLI command") touches
`crates/gyre-cli/src/client.rs` (+123) and `crates/gyre-cli/src/main.rs`
(+670) — genuine product surface — and was absent from task-155's
`commits:` list, which held 7 checkpoint-branch SHAs (squash-drift class
recorded by tasks 213/219/222/224/228: the squashed landing commit cannot
contain its own SHA).

## Independent checks (all from a clean checkout at the stated SHA)

- **Reproduce at base**: checked out `27bd585c`, ran the gate → exit 1 with
  exactly `27bd585c task-155 feat(task-155): Implement gyre search CLI
  command` — matches the baseline JSON verbatim.
  (`task-237-repro-at-base.txt`)
- **Repair at candidate**: at `ffd7b312`, gate → exit 0, "OK: every
  task-labeled product-surface commit is recorded…". The diff appends full
  SHA `27bd585ca7eb…c0c20` to task-155's existing 7-SHA list — the check's
  own documented remedy (`check-task-commit-attribution.sh:141-143`); nothing
  removed. All 8 listed SHAs exist as commits in this repository.
  (`task-237-after-repair-at-candidate.txt`)
- **Mutation (test-the-repair)**: removed the appended SHA via sed → gate
  FAILs exit 1 with the identical violation; restored via `git checkout` →
  exit 0. The pass is attributable to the recorded SHA, not gate drift.
  (`task-237-mutation-removed.txt`)
- **No weakening**: exemption file unchanged at its 3-entry frozen baseline
  (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`);
  `FROZEN_EXEMPTION_COUNT=3` untouched; zero diff under `scripts/`,
  `.github/`, `.pre-commit-config.yaml`.
- **Scoped fmt gate (CI's `Format` job, `check-rustfmt-diff.py HEAD^1`)**:
  exit 0 — "changed lines clean (0 Rust files checked)". The candidate
  touches no `.rs` file, so CI's changed-lines fmt gate passes. Note:
  repo-wide `cargo fmt --check` at this head reports ~100 pre-existing
  diffs across `gyre-adapters`/`gyre-cli`/`gyre-server` — out-of-scope old
  debt by the gate's explicit design ("without reformatting old debt");
  the candidate adds none (no Rust files touched).
  (`rustfmt-diff-gate-candidate.txt`, `fmt-check-candidate.txt`)
- **Attribution script no-op**: `dev-attribution.py task-237` with the
  candidate fetched as `origin/main` derives an empty commit list — the
  round adds only task records, no product surface — so task-237's
  `commits: []` is attribution-canonical. Tree remained clean after the
  run (the script rewrites the task file in place when non-empty).
- **Dev-controller suites**: `test_pipeline*.py` 57/57 OK. `test_dev*.py`
  39/40 with 1 error — `test_dev_cargo_clean` timed out at its own 30s
  subprocess budget while a clippy `--all-targets` build was compiling
  concurrently on this sandbox; environment-induced, unrelated to a
  specs-only diff (failure is in cargo timing, not any candidate file).

## Non-live baseline-log findings (rustfmt/clippy lines)

The baseline log's rustfmt violations (`lib.rs:1143-1149`, `ws.rs:704-779`)
and clippy warnings reference checkout path `ebbf8aec91e54d69800006ddec9f7bac`
— a gate-runner attempt tree that does not exist in this sandbox. At the
assigned base/candidate tree: `ws.rs` is 614 lines (flagged lines 704-779 do
not exist), `lib.rs` is 1592 lines, and the only authoritative probe named in
the baseline JSON (`check-task-commit-attribution.sh`) is the one reproduced
and repaired above. Confirmed the implementer's claim.

## Clippy gate

`check-clippy-diff.py` requires a full `cargo clippy --all-targets
--all-features` compile of the workspace; in this sandbox the run exceeded
the 300s foreground budget while dependencies were still building (no
verdict either way). Recorded for the verifier: the candidate touches no
Rust source — `changed` file map in the gate is empty, so no warning can
land on a changed line, and the only way the gate could fail is a compile
`error`, which the identical-at-base Rust tree excludes (base's own CI
clippy job ran green on the same Rust content per the baseline log showing
warnings, not errors). Verifier should run
`python3 scripts/check-clippy-diff.py HEAD^1` on a warm target dir or CI.

## Transport note

`/tmp/stage/capabilities.json` records TCP listener probes as unsupported
(errno 95) in this sandbox. No server/browser probe is relevant to this
specs-only repair; the authoritative probe is a git-history shell gate, run
directly above.
