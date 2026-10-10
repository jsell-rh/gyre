# Review — task-217 (Repair verified failure on main f4acb4ebcaf9)

Spec: task contract "Reproduce and repair this verified upstream failure… Do not weaken checks, add skips or exemptions." Verified failure per `GYRE_BASELINE_FAILURE_JSON`: `bash scripts/check-task-commit-attribution.sh` fails on base `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` — task-189's product-surface commit `f4acb4eb` is absent from `specs/tasks/task-189.md`'s `commits:` frontmatter.

Candidate under review: `ca5a28c5f003e149cda702b9388a79b1030d5ec7` (one commit on base). Verdict: **approved**.

## Round 1

Independent probes (evidence: `/tmp/stage/review-evidence/`):

- **Reproduction at pristine base** (`attribution-base-repro.txt`): temp worktree at `f4acb4eb` with no local changes; `bash scripts/check-task-commit-attribution.sh` → FAIL, exit 1, listing exactly `f4acb4eb task-189 feat(task-189): Fix persona scope resolution to walk the real parent chain`. The offending commit touches `crates/gyre-server/src/api/personas.rs` (+280 lines) — genuine product surface, so the failure is real review-scoping debt, not a script artifact.
- **Fix passes** (`attribution-candidate-ok.txt`): at candidate HEAD (clean tree) the same check → `OK: every task-labeled product-surface commit is recorded…`, exit 0. The candidate diff from base is exactly two files: `specs/tasks/task-189.md` (one line — appends full SHA `f4acb4ebcaf9…` to the `commits:` array) and the new `specs/tasks/task-217.md` task record. No production code changed, which is correct: the verified failure is attribution bookkeeping, and the check's own remediation text prescribes exactly this fix ("Fix by adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter").
- **Causality / negative control** (`attribution-negative-control.txt`): in a temp worktree at the candidate, reverting only the one-line task-189 frontmatter change restores the identical FAIL, exit 1. The pass is caused by this fix, not incidental drift elsewhere in history.
- **Parser registration** (`frontmatter-parse.txt`): ran the script's own `frontmatter_commits` awk over the fixed task file — `f4acb4eb` is among the six SHAs extracted; the full-length SHA matches the script's `[0-9a-f]{7,40}` → `cut -c1-8` extraction, so the fix registers through the real parser, not a format quirk.
- **No weakening** (`gates-and-invariants.txt`): `git diff f4acb4eb..ca5a28c5 -- scripts/` is empty; `scripts/task-commit-attribution-exemptions.txt` unchanged with exactly the 3 frozen entries (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); the script's freeze counter (`FROZEN_EXEMPTION_COUNT=3`) passed inside the exit-0 run. No skips, no exemptions, no gate edits.
- **Task-217's own attribution is correct**: the branch's sole commit touches only `specs/tasks/` — not product surface per the script's definition (`crates/|web/src|web/tests`) — so `commits: []` in task-217's frontmatter is right. Independently confirmed by running the repo's own `python3 /tmp/stage/dev-attribution.py task-217`, which produced no tree change (empty computed list). Same convention as the accepted task-211 repair.
- **Other baseline-log rustfmt failures are checkout artifacts, as the task claims**: `check-rustfmt-diff.py` diffs `BASE..HEAD` for `.rs` files; running it with base `f4acb4eb` at the candidate reports `changed lines clean (0 Rust files checked)`, exit 0 — the branch changes no Rust files. The flagged files (`traces.rs`, `mem.rs`, `otlp_receiver.rs`) were last touched by commits (`d7940e85`, `f31bef8f`, `998a3518`) that are all ancestors of the base, confirming those failures came from the superseded attempt's diff base, not this tree.
- **Commit message lint**: `git log -1 --format=%s ca5a28c5 | bash scripts/check-commit-msg.sh /dev/stdin` → passed, exit 0 (`feat(task-217): …` conventional form).
- **CI controller jobs run locally**: `python3 -m unittest discover -s scripts -p 'test_dev*.py'` → 40 tests OK; `-p 'test_pipeline*.py'` → 57 tests OK; `node --test scripts/dashboard/test/*.test.mjs` → 0 failures. Attribution check needs full history and runs in CI with `fetch-depth: 0` — the local run had full history and passed.

### Verification left to host CI (sandbox limitation, not a code defect)

- The full `cargo nextest run --all`, web `npm ci && npm run build && npm test`, and release-build jobs exercise code the candidate does not touch (branch diff is specs-only; base was green on them per the baseline log's clippy/arch/ABAC lines). The locally runnable gates covering the changed surface all pass as recorded above.

No findings. The repair is minimal, causal, and matches both the check's documented remedy and the precedent set by task-211 for the identical flaw class.
