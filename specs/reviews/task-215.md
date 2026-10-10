# Task-215 Review — Repair verified failure on main f4acb4eb

- **Candidate:** `dd211a01d8a2260ad99c62cccedddab1e4ea0df4`
- **Base:** `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`
- **Reviewer:** independent (task-215 review round)
- **Date:** 2026-10-10
- **Verdict:** APPROVED

## Scope of the diff

Candidate vs base touches exactly two files (`git diff --name-only base..candidate`):

- `specs/tasks/task-189.md` — appends the full ship SHA `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` to the `commits:` frontmatter list (6th entry).
- `specs/tasks/task-215.md` — new task file carrying the assignment contract, baseline failure log, and shipped notes.

Zero changes to `crates/`, `web/`, `docs/`, or `scripts/` (base→candidate `scripts/` diff is 0 lines). No exemptions added; `scripts/task-commit-attribution-exemptions.txt` is byte-identical to base and still at its frozen 3-entry baseline. This is the check's documented remedy and matches the task-211 precedent (`ce96eb64`) for the same flaw class.

## Contract reproduction

Baseline failure reproduced at the base commit in a temporary worktree (`/tmp/gyre-base-215`, removed after review):

```
$ bash scripts/check-task-commit-attribution.sh
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:

  f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain
exit=1
```

This is exactly the verified baseline JSON's failing line.

## Causality — the repair is load-bearing

Mutation probe: in a second worktree at the candidate, reverting only `specs/tasks/task-189.md` to its base content makes the check fail again with the identical line:

```
$ bash scripts/check-task-commit-attribution.sh
FAIL: ... f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain
exit=1
```

Restoring the candidate file passes. The frontmatter `commits:` extraction (`frontmatter_commits()`) lowercases and cuts `[0-9a-f]{7,40}` tokens to 8 chars, so the appended 40-char SHA matches the 8-char short SHA `f4acb4eb` that the history scan derives via `git rev-parse --short=8`. Without the appended entry there is no match — proven by the probe above, not by reading the diff.

## Repair verification at the candidate

- `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` exit 0.
- Frontmatter integrity: `commits` is valid JSON, 6 entries, all 40-hex, all resolve to commit objects; `title`/`progress`/`spec_ref` and the document body are byte-identical to base (only the `commits:` line changed). All five pre-existing lineage SHAs remain reachable from published `origin/pipeline/task-189/*` and `origin/devloop/task-189/*` refs — the review-scoping record of the examined candidate evolution is preserved, not discarded.
- Squash-merge provenance confirmed: `git diff f38abb7e 5ada6357` (PR branch net diff) equals the ship commit's patch `git diff f38abb7e f4acb4eb` modulo the publish flow's progress bump to `complete` (`scripts/pipeline/stages.py:177`) and merge-base exclusion of a stale task-210 bookkeeping hunk. `f4acb4eb` touches `crates/gyre-server/src/api/personas.rs` — genuine product surface, genuinely invisible to task-189's review scoping before this repair.
- Attribution for task-215 itself: `python3 scripts/dev-attribution.py task-215` produces no change — correct, since the branch changes no product surface (`specs/` only), so no task-215-labeled surface commit exists to record.
- Contract unmodified: `requirement_parts()` (scripts/dev-contract.py) of task-215 at the candidate equals the assigned contract exactly (title, spec_ref, depends_on, Required behavior, base, fingerprint); `progress: ready-for-review`, `commits: []` — same shape as the approved task-211 candidate.
- `git diff --check HEAD^1 HEAD` clean.

## Test evidence (independent reruns, logs in /tmp/stage/review-evidence/)

- `python3 -m unittest discover -s scripts -p 'test_dev*.py'` → 40 tests OK.
- `python3 -m unittest discover -s scripts -p 'test_pipeline*.py'` → 57 tests OK.
- `node --test scripts/dashboard/test/*.test.mjs` → 6 pass, 0 fail, exit 0.
- CI note: the attribution check is wired as a pre-commit/dev-check gate (`scripts/dev-check.sh:56`) rather than a `.github/workflows/ci.yml` step; the GitHub-side recheck for host verification is `bash scripts/check-task-commit-attribution.sh` on a full-history checkout (`fetch-depth: 0`).

## Durable-finding spot check

The shipped durable finding about `publish()` is accurate against the source: the merge command carries `--subject feat(task-NNN)` (`stages.py:367`), so the task-labeled ship commit is created by the merge itself after the PR body is frozen, and the MERGED path (`stages.py:281-292`) records `delivered.sha` only in the store — never in the task file's `commits:` frontmatter. The two prior drift instances cited (`a781ede2`/task-210 repaired by task-211's `ce96eb64`; `103364e6` attributing `1e8141f4` for task-151) all exist with the claimed subjects. The recurrence cause needs its own task; deferring unverifiable orchestration changes to a dedicated task is the right call, and in-scope repair here is complete.

## Notes

- The implementer's claimed evidence filenames (`attribution-before.txt`, `attribution-after.txt`) were not present in the stage directory at review time; this review's independent probes (`attribution-base.txt`, `attribution-candidate.txt`, `attribution-mutation-reverted-fix.txt`) reproduce both claimed states and the causal link, so nothing rests on the implementer's logs.
- `specs/reviews/task-211.md` does not exist on main (only on the review checkpoint `408642a2`) — the task-211 merge message links a review record via PR description blob URL, not a main-path file. Same pattern applies here; not a defect of this candidate.
