# Review — task-213 (Repair verified failure on main 8c2d17750585 / round-2 base 19d65446)

Spec: `GOAL.md` — real implementations and meaningful verification.
Candidate under review: `ae451a27fdf29baced87a03b851ec72e9dbd66e0` (base `19d65446917e983dd8f85f5e6f0d681999cd3012`).
Verdict: **approved**.

## What the task required

Reproduce and repair the verified upstream `check-task-commit-attribution.sh` failure without weakening any check, adding exemptions, or shipping unneeded code. The failure class is squash-drift: a task-labeled product-surface ship commit whose SHA post-dates the task file the PR carries, making the commit invisible to review scoping (task-095 R3-F4 flaw class). Round 2's drift instance: `f4acb4eb task-189` at base `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`.

## Independent verification (logs under `/tmp/stage/review-evidence/`)

- **Baseline reproduction.** Gate at pristine `f4acb4eb` in a detached worktree → FAIL exit 1, naming exactly `f4acb4eb task-189 feat(task-189): Fix persona scope resolution to walk the real parent chain` (`task-213-reviewer-round2-before-repair.log`). Round-1 baseline independently reproduced too: gate at pristine `8c2d1775` → FAIL exit 1 naming `a781ede2 task-210` (`task-213-reviewer-round1-before.log`).
- **Repair present and causal.** Gate at candidate `ae451a27` → OK exit 0 (`task-213-reviewer-attribution-after.txt`). Mutation check (test-the-test): in a detached worktree at the candidate, removing `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` from task-189's frontmatter re-fails the gate with the identical violation, exit 1 (`task-213-reviewer-mutation-check.log`). The pass is attributable to the recorded SHA, not gate drift.
- **The repair is genuine, not narrative.** The task-215 PR branch tip `dd211a01` (origin/pipeline/task-215/c376722fbfd44a90b4eaa01afe76837f-1) carries the one-line append of `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` to task-189's `commits:` list (5→6 entries). On main it landed via `a1751da1` (squash attribution subject `19d65446`); both heads pass the gate independently. The branch merge `81f8c910` (parents `fdb86859` + `19d65446`) carries it into this candidate. Round-1's `a781ede2`/task-210 repair (`f38abb7e`, task-211) is likewise an ancestor of the candidate; none of the 14 checkpoint-branch repair commits that never merged is an ancestor.
- **No weakening.** `git diff 19d65446..ae451a27 -- scripts/` is empty — the gate script is byte-identical to base. `scripts/task-commit-attribution-exemptions.txt` still has exactly its 3 frozen entries; `FROZEN_EXEMPTION_COUNT=3` unchanged. No skips, no exemptions, no check edits.
- **Scope discipline.** `git diff 19d65446..ae451a27` touches only `specs/tasks/task-213.md` (+85, the task record). Zero delta under `crates/`, `web/`, `scripts/`. No product-surface commit exists since base (`git log 19d65446..HEAD --no-merges -- crates web` empty), so `commits: []` is correct — confirmed by running `dev-attribution.py task-213` (no change). `git diff --check` clean.
- **Frontmatter integrity.** task-189's `commits` is a valid 6-entry JSON list (unique, all SHAs resolve to existing objects) containing the ship SHA; task-210's is a valid 9-entry list containing `a781ede2ea21a5153dbcb49c65771f990344a093`.
- **All script-based deterministic gates at the candidate pass** (`task-213-reviewer-gate-suite.txt`): arch, ABAC route registry, ABAC exempt handlers, MCP write tools, migration versions, migration SQL portability, dead message kinds, byte-slice truncation, relative path defaults, fail-open ref resolution, fabricated scope defaults, lossy secret conversion, scope literal defaults, inert enforcement, forged scope fields, forwarded header trust, in-memory state stores, unbounded external HTTP, task-commit attribution — all exit 0.
- **Transport probe** is unsupported in this sandbox (`accept` errno 95, `/tmp/stage/capabilities.json`); no server/browser probe applies to this specs-only change. Full workspace suite and exact-head GitHub CI belong to verification.

## Findings

None blocking. One prose nit, recorded for the record, not a defect: the task-213 "Shipped" section attributes the task-189 frontmatter append to "commit `19d65446`" — strictly, `19d65446` is the specs-only squash-attribution commit for task-215; the append itself landed on main in `a1751da1` (task-212's ship, which carried task-215's branch content) after originating on the task-215 branch (`dd211a01`). The operative claim — the repair is in this candidate's lineage and clears the gate causally — is correct and independently verified. All 85 added lines are a task record; the record's evidence-file references (e.g. `task-213-round2-*.log`) point at the implementation sandbox's `/tmp/stage/review-evidence/`, which is standard for these records; the reviewer re-derived every operative claim from fresh probes rather than trusting those files.

Commit-message lint: `85432a3a` and `ae451a27` pass `scripts/check-commit-msg.sh`; `60668c82`/`fdb86859` use the historical `process:`/pipeline subjects (1481 precedents across pipeline branches; the hook runs at commit-msg stage on interactive commits only, and the gate deliberately skips `process:` subjects). Not a defect of this candidate.

## Verdict

Approved. The verified failure is reproduced and repaired in the candidate's lineage by real recorded attribution (both drift instances), no check is weakened, the exemption file is untouched, and the mutation check proves the gate pass is causal. Remaining checks for verification: full workspace suite and GitHub CI on the exact head.
