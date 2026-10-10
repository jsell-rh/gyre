# Review — task-218 (Repair verified failure on main f4acb4ebcaf9)

Assignment: repair the verified upstream failure at base `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` — `scripts/check-task-commit-attribution.sh` fails because commit `f4acb4eb` (feat(task-189), touches `crates/gyre-server/src/api/personas.rs`) is absent from task-189's `commits:` frontmatter, making it invisible to review scoping (task-095 R3-F4 flaw class).

Candidate: `49b3c6389eac39b834a9a9f580127c35b11c21d9` (base + `273f1b62` repair + `49b3c638` process note). Cumulative diff: exactly two files, both under `specs/tasks/` — `task-189.md` (+1 SHA appended to `commits:`), `task-218.md` (new task contract + shipped notes, 81 lines). No production code, scripts, exemption files, or coverage matrices touched.

Verdict: **complete**.

## Round 1

Independent probes (evidence under `/tmp/stage/review-evidence/`):

- **Baseline reproduction (fresh detached worktree at base):** `bash scripts/check-task-commit-attribution.sh` → exit 1, failing output matches the assignment's baseline log verbatim (`f4acb4eb task-189 ... Fix persona scope resolution to walk the real parent chain`). Confirmed `f4acb4eb` touches product surface (`crates/gyre-server/src/api/personas.rs`, +280) and is task-labeled — squarely inside the check's contract.
- **Candidate passes:** same check at `49b3c638` → exit 0, `OK: every task-labeled product-surface commit is recorded ...`.
- **Counter-probe (fix is load-bearing):** the added SHA was removed in-place at the candidate and the check re-run → exit 1 with the identical baseline failure; then restored (`git checkout --`), tree verified clean, check re-passes. The pass is caused by the repair, not by coincidence, exemption growth, or check weakening.
- **No exemption route taken:** `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen 3-entry baseline (01493c88 task-097, 17c81d5a task-072, a8d036f4 task-091); `FROZEN_EXEMPTION_COUNT=3` unchanged in the script. The repair used the check's documented remedy (record the SHA in the task file's frontmatter) — the same remedy already applied for 5aaded21 task-095 on 2026-09-30.
- **The other two baseline failures were stale-checkout artifacts, correctly not "fixed":** at both base (fresh worktree) and candidate, `python3 scripts/check-rustfmt-diff.py f4acb4eb` → exit 0 ("0 Rust files checked" — no Rust differs from base) and `bash scripts/check-relative-path-defaults.sh` → exit 0. The baseline log's `main.rs:3521`/`main.rs:1850` line references cannot exist in this history (`main.rs` is 3501 lines; the dynamic `PathBuf::from(&repo_name)` fallback sits at :1737, covered by the committed task-099 exemption at base). The log came from the superseded attempt checkout `.gyre-pipeline/attempts/8cb5bacc.../checkout`. The implementer's diagnosis is corroborated, and appropriately nothing was changed for these.
- **Task's own attribution is correct:** `python3 scripts/dev-attribution.py task-218` → exit 0, no change. The only task-218-labeled commit anywhere is the process note (specs-only, `process:` subject — excluded by both dev-attribution.py and check-task-commit-attribution.sh), so `commits: []` is accurate, not a dodge. Both branch commits are `process(...)` machinery commits — the type the pipeline itself creates with `core.hooksPath=/dev/null` (stages.py:184-185); 17 `process:` commits already exist on origin/main. Not a candidate defect.
- **Diff hygiene:** `git diff --check base..candidate` clean; whitespace clean; the task-189 frontmatter edit is a minimal one-token append joining the 5 pre-existing full SHAs from the task's branch.

Verified working (no findings):

- The repair is the canonical, non-weakening remedy prescribed by the failing check itself, its exemption-file header, and AGENTS.md: record the commit in the task's `commits:` frontmatter rather than growing the frozen exemption file. `f4acb4eb` is now review-scoped for task-189's history, restoring the invariant the check guards (every task-labeled product-surface commit visible to review rounds).
- Task-218's contract body reproduces the assignment's required behavior verbatim (required behavior, base, environment fingerprint) and its Shipped section's claims were each independently re-verified during this review (reproduction, repair mechanics, exemption freeze, stale-artifact diagnosis) — no claim in the notes contradicts observed evidence.
- Scope discipline: the candidate changed nothing outside `specs/tasks/`. In particular it did not touch `specs/coverage/` (the check that failed is history-scoped, not coverage-scoped), did not edit any script or verifier, and did not modify task-189's coverage rows.

Notes (no action required for this task):

- `specs/coverage/SUMMARY.md` is stale relative to the coverage matrices (business-continuity shows 1 assigned/4 implemented but its matrix says 0/5). The drift was introduced by task-151's `1e8141f4` (2026-10-06) and exists identically at base `f4acb4eb` — pre-existing, untouched by this candidate, and outside this task's assignment (which is the attribution failure only). Flagged for the PM/backlog, not a finding against this candidate.

Recommendation: **approve**. The verified upstream failure is repaired with the minimal, check-prescribed, non-weakening change; reproduction and the load-bearing counter-probe are on record under `/tmp/stage/review-evidence/`. Verdict written to `/tmp/stage/verdict.json` with `approved: true` and no findings.
