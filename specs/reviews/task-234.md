# Review — task-234 (Repair verified failure on main 27bd585ca7eb)

Task: `specs/tasks/task-234.md` — repair the verified baseline failure of
`scripts/check-task-commit-attribution.sh` at base `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
(commit `27bd585c` labeled task-155, touching product surface, absent from
task-155's `commits:` frontmatter).

Candidate: `2c4a170812ed91643941abedc1716ef7244fb35c` (single commit on base).
Verdict: **approved**.

## Scope of change (diff base → candidate)

- `specs/tasks/task-155.md` — one line: appends the full landing SHA
  `27bd585ca7eb429905ccbded1f48b4d0167c0c20` to the existing `commits:` array,
  preserving all 7 candidate-branch SHAs.
- `specs/tasks/task-234.md` — new task file (baseline log, shipped summary,
  evidence pointers), frontmatter shape identical to sibling repair tasks
  (task-227/task-228).
- Everything else: zero bytes (`git diff base..candidate -- scripts/ crates/ web/`
  is empty). No exemption file change, no check weakened, no skip added.

## Independent probes (evidence in /tmp/stage/review-evidence/, run in a detached
worktree; `task-234-*-independent.txt`)

- Reproduction at base: `bash scripts/check-task-commit-attribution.sh` → exit 1,
  identical violation `27bd585c task-155 feat(task-155): Implement gyre search
  CLI command` (`task-234-attribution-repro-independent.txt`).
- After repair at candidate: exit 0
  (`task-234-attribution-after-repair-independent.txt`).
- Mutation check (the pass is attributable to the recorded SHA, not gate drift):
  sed-removing the SHA from the worktree copy reproduces the exact FAIL (exit 1);
  restoring it returns to exit 0 (`task-234-mutation-independent.txt`,
  `task-234-restored-independent.txt`).
- `check-arch.sh` → OK at candidate.

## Correctness of the repair

- The check scans full history for non-merge, non-`review:`/`process:` commits
  whose subject matches `task-NNN` and whose files match
  `^(crates/|web/src|web/tests)`. `27bd585c` touches `crates/gyre-cli/src/client.rs`
  and `main.rs` — in scope. Its 8-char short SHA is exactly `27bd585c`, and
  `frontmatter_commits()` lowercases and truncates each hex run to 8 chars, so the
  appended 40-char SHA matches. This is the check's own documented remedy
  ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter"),
  not an exemption — the frozen exemption file stays at its 3-entry baseline with
  no `27bd585c` entry.
- Precedent-consistent: same drift class and same fix shape as task-068's
  `a11ba8d3` and task-196's `05709c24` (both recorded their landing SHAs).
- Review scope is preserved, not narrowed: all 7 pre-existing frontmatter SHAs
  remain resolvable and reachable from `origin/pipeline/task-155/*` and
  `origin/devloop/task-155/*` refs. Regenerating via `scripts/dev-attribution.py`
  would also include `27bd585c` (label + product surface match), so the static
  append matches tooling semantics.
- Frontmatter remains machine-valid: 8 unique 40-hex SHAs; the only script in the
  tree that parses `specs/tasks/*.md` in-place is the attribution check itself
  (passes); the `dev-*` tools use synthetic temp repos.

## No collateral regression

- Zero Rust/JS/scripts/migrations changed, so rustfmt/clippy-diff, cargo tests,
  web tests, and all source-pattern gates are provably unaffected (baseline log
  already showed them green at base; nothing they read changed).
- CI dev-controller suite at candidate: `test_pipeline*.py` 57 OK;
  `test_dev*.py` 40 OK sequentially. One `test_dev_inference`
  omp-RPC test failed once under concurrent sandbox load and passes 5/5 in
  isolation at both base and candidate — environmental flake, not a regression
  (zero script bytes changed base→candidate).
- The candidate commit subject `implement(task-234): pipeline checkpoint` is the
  pipeline's established checkpoint convention (cf. `f0ff1855` task-227,
  `1e5400c7` task-228); the conventional-commits hook is pre-commit-only and not
  run over history by CI, and the commit touches no product surface so the
  attribution check never scans it.
- Markdown em-dashes in `specs/tasks/*.md` are pre-existing convention; CI's
  em-dash step is crates-only with `|| true`.

## Sandbox limitation (recorded, not a code defect)

TCP `accept()` is unsupported here (errno 95, `/tmp/stage/capabilities.json`).
Irrelevant to this task: the failing probe is a pure git-history script needing
no server, listener, or browser. Full gates and GitHub checks run in the
verification/publication stage.
