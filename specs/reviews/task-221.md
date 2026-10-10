# Review — task-221 (Repair verified failure on main 6bf777a6a44f)

Task: `specs/tasks/task-221.md` — reproduce and repair the verified upstream failure at base `6bf777a6a44f28052ed5af28bf6fb013fde6df48` (`check-task-commit-attribution.sh` FAIL: task-200's ship commit `6bf777a6` missing from task-200's `commits:` frontmatter).

Commits under review: `5bacbc27` (single commit; diff `6bf777a6..5bacbc27` = `specs/tasks/task-200.md` +1/−1 line, `specs/tasks/task-221.md` +69 new). Working tree clean at HEAD = candidate.

Verdict: **approved**.

## Evidence (all probes at /tmp/stage/review-evidence/)

- **Reproduction (before):** `git worktree add /tmp/stage/base-worktree 6bf777a6...` then `bash scripts/check-task-commit-attribution.sh` → FAIL, exit 1, listing exactly `6bf777a6  task-200  feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)` — byte-matching the recorded baseline failure (`attribution-before.txt`). The reproduction used an isolated worktree; the candidate tree was never checked out away from `5bacbc27`.
- **Fix probe (after):** at candidate `5bacbc27`, `bash scripts/check-task-commit-attribution.sh` → `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).`, exit 0 (`attribution-after.txt`).
- **Root cause confirmed:** `6bf777a6` is task-200's product-surface commit on main (`crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs`, `docs/api-reference.md`) but the task's frontmatter carried only the 13 pre-ship lineage SHAs — a commit cannot contain its own hash. Same chicken-and-egg drift class as the task-211/216/218 repairs.
- **Repair is the check's documented remedy, not a weakening:** the appended SHA `6bf777a6a44f28052ed5af28bf6fb013fde6df48` joins the existing 13. Append-only proven by set-diff of base vs candidate frontmatter SHAs: 13 → 14, removed = none, added = the one SHA. All 14 resolve in history (`git cat-file -e <sha>^{commit}`, 14/14; 7-char hex strings elsewhere in the file body are prose, not frontmatter). Independent re-implementation of the check's scan logic (not reusing its awk) confirms every task-200 product-surface commit is present.
- **No exemptions, no weakened gates:** `git diff 6bf777a6..5bacbc27 -- scripts/` is empty; `task-commit-attribution-exemptions.txt` still at its frozen 3-entry baseline; `dev-check.sh`'s exemption-growth guard sees no additions.
- **Attribution of the repair itself:** `5bacbc27` touches `specs/` only (0 files under `crates/|web/src|web/tests`), so `commits: []` in task-221's frontmatter is correct; `dev-attribution.py task-221` exits 0 producing no change.
- **Other gates at candidate:** rustfmt-diff (`0 Rust files checked`, exit 0), `git diff --check` clean, `check-arch.sh` OK, `GYRE_CHECK_HIERARCHY=1 check-hierarchy.sh` OK, dev-controller unit tests 40 OK. Clippy-diff structurally no-op (0 `.rs` files in the diff). No Rust/web source changed, so no cargo suite or HTTP probe is meaningful here; sandbox listener probe unsupported (errno 95) per `/tmp/stage/capabilities.json`.
- **Verifier must still run on host/CI:** the full `scripts/dev-check.sh` battery and GitHub CI (including `check-task-commit-attribution.sh` with full history, `fetch-depth: 0`) on the merge SHA.

## Non-blocking observations (recorded, not findings)

- The branch commit subject uses type `process(` … `)`, which `check-commit-msg.sh`'s strict pattern rejects. That script runs only as the pre-commit `commit-msg` stage — not in `.github/workflows/ci.yml` nor in `scripts/dev-check.sh` — and main already carries 34 bare-`process:` bookkeeping commits plus 217 total subjects failing the strict pattern. The attribution check itself treats `process:`/`review:` subjects as sanctioned bookkeeping, and the pipeline re-titles branch commits at merge (task-218's process commit merged as `feat(task-218)`). No gate evaluates this subject.
- Task numbering: `task-221` follows `task-218`; labels `task-219`/`task-220` exist in history, so no collision.
