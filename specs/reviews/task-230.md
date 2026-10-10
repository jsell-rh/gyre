# Review — task-230 (Repair verified failure on main 05709c24)

Assignment: reproduce and repair the verified upstream failure of `bash scripts/check-task-commit-attribution.sh` on main `05709c242509b89214876339b3c463ede3a31b60` (task-labeled product-surface commit `05709c24 task-196` absent from `specs/tasks/task-196.md`'s `commits:` frontmatter), without weakening checks, adding exemptions, or implementing the blocked feature.

Commit under review: `14cc74cd` (single commit on the assigned base). Verdict: **approved**.

## Diff inspected

`git diff 05709c24..14cc74cd` touches exactly two files, both under `specs/tasks/`:

- `specs/tasks/task-196.md` (+1/-1): appends `05709c242509b89214876339b3c463ede3a31b60` to the existing three-SHA `commits:` list — the check's own documented remedy.
- `specs/tasks/task-230.md` (new, 72 lines): the task record (required behavior, baseline failure log, shipped notes).

No `scripts/` changes (diff over `scripts/` is empty), no Rust/JS production code, no CI workflow edits. `scripts/task-commit-attribution-exemptions.txt` is untouched at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`; `FROZEN_EXEMPTION_COUNT=3` unchanged). Nothing was weakened, skipped, or exempted.

## Independent verification (evidence under /tmp/stage/review-evidence)

All commands run in the candidate checkout at `14cc74cd` with a clean tree.

1. **Gate at candidate** (`attribution-gate-at-candidate.txt`): `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded...`, exit 0.
2. **Mutation check — repair is load-bearing** (`attribution-mutation-check.txt`): `git checkout <base> -- specs/tasks/task-196.md` (repair reverted, check unchanged) → gate FAILS with the identical violation (`05709c24 task-196`, exit 1); restoring the candidate's file re-passes (exit 0). The pass is attributable to the recorded SHA, not gate drift or an exemption. Tree restored to candidate exactly afterward (`git status` clean, HEAD `14cc74cd`).
3. **Repair is minimal and correctly scoped** (`attribution-scope-analysis.txt`):
   - `05709c24` is the only product-surface commit labeled task-196 in history; the other task-196-labeled commit (`e5dbfe17`, `chore(tasks)`) touches only `specs/`, out of the gate's scope by its own rule.
   - The three pre-squash SHAs (`3b90956c`, `abcfff04`, `88b57180`) exist as objects; they are kept as historical review-scoping records and the landing SHA is **appended**, matching the task-224 precedent (`a11ba8d3` in task-068's list).
   - `dev-attribution.py task-230` produced no change — `commits: []` is correct for a specs-only round. (Note for the record: my read-only-intended run of `dev-attribution.py task-196` would rewrite the list to only the landing SHA, since the pre-squash SHAs are unreachable from HEAD; the resulting working-tree change was reverted and the tree restored to candidate. Either form passes the gate; the candidate's append form is the established convention.)
4. **ABAC WARN from the baseline log investigated** (`abac-warn-investigation.txt`): `bash scripts/check-abac-route-registry.sh` → OK, exit 0, no WARN in this tree. The baseline runner's WARN came from a different checkout; advisory-only and not the assigned probe.
5. **Commit message note (not a defect)**: the candidate checkpoint commit's subject `implement(task-230): pipeline checkpoint` does not match `scripts/check-commit-msg.sh`'s allowed types, but that check is a pre-commit commit-msg hook on new commits only — it does not scan history, and the pipeline lands candidates as `feat(task-NNN): ...` per the task-213/219/222/224 precedent. Recorded for the verifier, not a finding against the diff.

## Checks for the verifier to re-run

- `bash scripts/check-task-commit-attribution.sh` → exit 0.
- `git diff <base>..<candidate> -- scripts/` → empty; exemption file at 3 entries.
- Full `scripts/checks.sh` suite and GitHub CI on the landed `feat(task-230)` commit (not run here; this review used focused probes only, per review policy — no product code changed, so no build/test surface is implicated).

## Verdict

The repair is real, minimal, and load-bearing: the reproduced failure is eliminated by recording the missing landing SHA exactly as the gate's failure message prescribes, with no exemption growth or check weakening. Approved.
