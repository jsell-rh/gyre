# Review — task-220 (Repair verified failure on main 6bf777a6a44f)

Round 3, independent review of candidate `9d01707a4a2ea0991b658a7d9a39908aae863070` against assigned base `e96d25abcdbb51f8890ea36d11541bfa9f80a2b8` (ancestor of candidate, verified). Evidence: `/tmp/stage/review-evidence/`, indexed in `task-220-r3-verification.txt`.

Verdict: **approved** (no findings).

## Scope of the diff

`git diff --name-status e96d25ab 9d01707a` is exactly one added file: `specs/tasks/task-220.md` (+68). No `scripts/`, `crates/`, `web/`, `specs/tasks/task-200.md`, or `specs/tasks/task-219.md` changes beyond the base. Branch commits `4635b533` → `42260613` → `9d01707a` (via clean two-parent merge `ce2e36ce` of the base) are all specs-only, so `commits: []` is correct for this task's own frontmatter (`dev-attribution.py task-220` → no change, exit 0).

## Baseline failure reproduced and repaired (verified, not taken on narrative)

- Reproduction at the failing base: detached worktree at `6bf777a6a44f28052ed5af28bf6fb013fde6df48`, `bash scripts/check-task-commit-attribution.sh` → **exit 1** with `6bf777a6 task-200 feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)`. Output matches the canonical `GYRE_BASELINE_FAILURE_JSON` log field line-for-line, modulo the checkout-specific exemptions-path line (`task-220-r3-canonical-compare.txt`). Canonical probe (`["bash", "scripts/check-task-commit-attribution.sh"]`) and base match.
- Repair present in the candidate tree: `6bf777a6a44f28052ed5af28bf6fb013fde6df48` recorded in `specs/tasks/task-200.md`'s `commits:` frontmatter at the assigned base and at the candidate (present); absent at the failing base (verified by `git show <ref>:specs/tasks/task-200.md`). The base `e96d25ab` is task-219's landing on main and carries the identical task-200.md tree change as checkpoint commit `5df2f9ab` ("process(task-200): record commit 6bf777a6 …") — the check's documented remedy, same precedent as task-212/task-218 drift repairs. `6bf777a6` itself is a 961-insertion product-surface commit (`gyre-common/src/message.rs`, `api/messages.rs`, `mcp.rs`), so recording it restores review scoping over real surface.
- Gate at candidate HEAD: exit 0 (`task-220-r3-gate-at-head.log`).
- **Mutation check (attributability):** removing the recorded SHA from task-200's frontmatter in the working tree re-fails the gate with the identical violation (exit 1, `task-220-r3-mutation-gate.log`); restoring it re-passes (exit 0, `task-220-r3-restore-gate.log`). The pass is attributable to the repair, not gate drift. Working tree clean after restore.
- No exemption growth: `scripts/task-commit-attribution-exemptions.txt` sha256-identical to base, still 3 entries (frozen baseline). No check, skip, or gate weakened — `scripts/` has zero diff from base to candidate.

## Contract finding `66f22838` repaired (root cause confirmed mechanically)

`requirement_parts` of the candidate file equals that of the assigned contract on both front and prose (fronts identical across round-1/round-2/candidate; prose deltas of round-1 (`4635b533`) and round-2 (`42260613`) versus the candidate are exactly the non-whitelisted `## Repair` section — the section that produced the prior generic contract findings). The round-3 file keeps only the whitelisted operational sections (`## Baseline failure` per the assigned bytes, `## Shipped`) and the `progress:` flip. Guard snapshot `review-before.json` task_text (controller-seeded before review) equals the candidate file byte-for-byte — no post-candidate tampering.

## Whitespace trim on the baseline-blob line (verified necessary and minimal)

Round-2's preserved canonical bytes carry a trailing ` \t` on line 19 (the controller-seeded blob line inside `## Baseline failure`): `git diff --check 4635b533 42260613` → exit 2, `specs/tasks/task-220.md:19: trailing whitespace` — reproducing the claim that round-2 was unverifiable against `dev-check.sh`'s first line. The round-3 file trims exactly those two bytes (line lengths 20148 → 20146, rstrip-equal otherwise), which is invisible to `requirement_parts` (inside the stripped operational section — verified: front+prose unchanged) and clears `git diff --check HEAD^1 HEAD` → exit 0, including the whole-file-added comparison against the assigned base (exit 0). This is the minimal correction that makes both the contract comparison and the whitespace gate pass; documented in the task file's `## Shipped`.

## Other checks

- `bash scripts/check-arch.sh` at candidate HEAD: OK (exit 0).
- `check-commit-msg.sh` on the candidate commit subject (`fix(task-220): …`): exit 0.
- No tests touched; no runtime surface changed (specs-only diff), so no server/browser probe is owed. TCP listeners are unsupported in this sandbox (capabilities.json, accept errno 95) — not a code defect; full verification (dev-check.sh battery, web build, test suite, GitHub CI on the exact head) belongs to the host verifier.

Findings: none.

— Reviewer, 2026-10-10
