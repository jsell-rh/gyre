# Review — task-237 (Repair verified failure on main 27bd585ca7eb)

Task: `specs/tasks/task-237.md`. Candidate: `d61eea2ad763e81710c5945c9c9d7ef887a43f27`, base `db37fd02fcaf0b4ad1b00ca4abde6bf12e7dbae8` (assignment), verify-time base→merge diff for the failed round-1 verification was `db37fd02 → d9a6b29f`.

Verdict: **complete** — the verified upstream failure is repaired by real production changes, no checks weakened, no exemptions added, no fake completion.

## What the baseline failure was and what this candidate does

Two distinct failures were in play, and the candidate correctly separates them:

1. **The assigned baseline failure** (task-155 attribution): round-1 candidate `ffd7b312` appended the landing SHA `27bd585ca7eb…` to `specs/tasks/task-155.md`'s `commits:` frontmatter. That repair is correct and still in place at the candidate (`task-155.md:8` carries all 7 checkpoint SHAs plus the landing SHA). `bash scripts/check-task-commit-attribution.sh` at candidate HEAD → **OK, exit 0** (`review-evidence/task-237-attribution-check.txt`).

2. **The round-1 host-verification failure** (durable finding `08dbce51…`): `checks.sh` exited 1. Independently reproduced here: `git diff --check db37fd02 d9a6b29f` → **exit 2** with exactly one offender, `specs/tasks/task-237.md:19: trailing whitespace.` — the generator-embedded baseline-log excerpt line ending in backtick + space + tab (`review-evidence/` whitespace repro below). The durable log tail shows the web build (the LAST gate) completing and `web/dist` cleanup running, consistent with `dev-check.sh`'s collect-and-continue design (`FAILED=1` at gate 1, all later gates green) — so this one line was the sole failing gate, not one among several.

## Verified in this review

### R1: The exact verify-time gate now passes

- `git diff --check db37fd02 d61eea2a` → **exit 0** (assignment base → candidate).
- `git diff --check d9a6b29f d61eea2a` → **exit 0**.
- `git diff --check HEAD^1 HEAD` at candidate HEAD → **exit 0**.
- Full simulation of the verify stage's merge topology (`stages.py:187-190` — base checked out, approved candidate with `progress: complete` rewrite committed, `--no-ff` merge): `git diff --check HEAD^1 HEAD` → **exit 0**.

Reproduction of the failure first: `git diff --check db37fd02 d9a6b29f` exits 2 with the single `task-237.md:19` offender, byte-level confirmed (`git show ffd7b312:specs/tasks/task-237.md` line 19 ends `` ` `` + space + tab; `review-evidence` below).

### R2: The committed-file repair preserves content exactly

Round-1 line 19 → candidate line 19: byte lengths 20192 → 20190; the stripped suffix is exactly `' \t'` and the remainder is byte-identical. Every line 1–58 (frontmatter + `## Baseline failure` section) of the candidate equals `rstrip()` of the round-1 file. The only other differences are in the operational `## Shipped` section (lines 59+), which is contract-excluded. No content lost.

### R3: The generator fix is real production code with a red/green-verified test

`scripts/pipeline/stages.py:396-397` (`baseline_repair`): generated excerpts are now sanitized per-line (`'\n'.join(line.rstrip() for line in …splitlines())`) with an accurate comment naming the `git diff --check` gate dependency. This is the only path that embeds log text into repo-committed files (`grep` over `scripts/` for `baseline_log`/`log_tail`/`[-24000:]`: `log_tail` fields feed prompts/findings outside the repo via `execution.py:195-207`; `dev-ci.py` writes logs into attempt directories, never the repo) — so the class is closed at its single origin.

Mutation probe (production code only, test untouched):

- **RED**: reverting the sanitizer to the pre-fix `excerpt = Path(...)read_text()[-24000:]` makes `test_pipeline_execution.BaselineExcerptTest.test_generated_task_body_carries_no_trailing_whitespace` **FAIL** with `trailing whitespace on lines [20, 21]` (`review-evidence/task-237-red.txt`).
- **GREEN**: with the candidate's sanitizer restored (worktree verified byte-identical to `d61eea2a` by `git diff`), the test passes (`review-evidence/task-237-green.txt`).

The test is genuinely anchored to the production behavior, not self-confirming: it drives the real `baseline_repair` handler with a real `Store` and a log fixture containing trailing space+tab, `middle\t `, and a lone `\r`, then asserts the generated task body has zero trailing-whitespace lines. A second test pins dedup stability (same task returned, one row).

Edge probes on the sanitizer semantics (`review-evidence/task-237-sanitizer-semantics.txt`): logs containing `\r` (CRLF or lone-CR) produce no CR-terminated and no trailing-whitespace lines in the generated body (`splitlines()` splits on `\r`; `rstrip()` strips it — and `git diff --check` flags CR-before-LF as trailing whitespace, so this matters and is handled); a `[-24000:]` byte-tail cut mid-multibyte is decoded cleanly by `read_text()` and the excerpt is preserved; generation hash is stable across repeated calls.

### R4: Contract safety — no requirements_changed loop

Using the repo's own `scripts/dev-contract.py`: `generation()` over the round-1 body and the candidate body (with the full current specs corpus) yields the **identical** hash `4f03dfea…`, and `requirement_parts` returns equal frontmatter and equal contract prose (`review-evidence/task-237-contract-generation.txt`). `Baseline failure` is contract-excluded for repair-titled tasks (`dev-contract.py:18-20`); `Shipped` likewise. So the whitespace strip and the `Shipped` rewrite cannot fence the task or invalidate receipts.

Body-flow safety: `dev-pipeline.py:100-108` persists the sanitized materialized body via `task_override`/`generation_for`; `sync_checkout` re-puts the checkout body when generations match; the `baseline_repair` dedup path (`stages.py:410-412`) returns the existing task by `baseline_key` without resurrecting any stale stored body. The dirty round-1 body has no resurrection path into future commits.

### R5: No weakening, no scope violations

- Exemption diff `db37fd02 → d61eea2a` across all 34 `scripts/*-exemptions.txt`: **zero additions, zero removals**; the attribution exemption file stays at 3 entries (`review-evidence/task-237-no-weakening.txt`).
- Diff surface is exactly `scripts/pipeline/stages.py`, `scripts/test_pipeline_execution.py`, `specs/tasks/task-237.md` (plus the round-1 `specs/tasks/task-155.md` already carried by the merge). No `crates/`, `web/src`, `web/tests` paths → `commits: []` is the canonical `dev-attribution.py` derivation for this round, not a dodge: the attribution check derives product-surface commits from `crates/`/`web/src`/`web/tests` paths and none exist in range.
- `bash scripts/check-task-commit-attribution.sh` → OK.

### R6: Broader surfaces at the candidate

- Full scripts test suite: `python3 -m unittest discover -s scripts -p 'test_*.py'` → **100 tests, OK** (`review-evidence/task-237-scripts-suite.txt`).
- `python3 -m unittest discover -s scripts -p 'test_pipeline*.py'` → 59 tests, OK.
- Full static gate battery (the 21 `check-*.sh` gates `dev-check.sh` runs): **all OK** at the candidate (`review-evidence/task-237-static-gates.txt`).

### Residual risk assessed, not blocking

`git diff --check` also flags `space before tab in indent` — a whitespace class `rstrip()` does not remove. This is not the observed failure class (the reproduced offender was trailing whitespace, and the minified-JS log excerpt lines begin at column 0), and a probe of all generator-produced excerpts (task-231/232/237 at all refs) finds zero space-before-tab-in-indent lines. Trailing-whitespace is the class raw log tails actually produce (log lines end in whitespace, they don't start with indents). Recording it as a bounded residual, not a defect against this task's contract.

### Environment constraints

This sandbox cannot accept TCP listeners (capabilities.json: `tcp_listener_probe` errno 95), so no server/runtime probes were run — none are needed for this diff (pure Python pipeline code + markdown; no Rust or web surface touched). Full `cargo test --all`, `npm test`, and the `checks.sh` gate remain for host verification, which already ran the Rust/web surfaces unchanged from the round-1 verification (the Rust/web trees at the candidate are identical to `db37fd02` — the diff touches no Rust or web file).

Findings:

- (none)

— Reviewer, 2026-10-10
