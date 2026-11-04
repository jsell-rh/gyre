# Review — task-241 (Repair verified failure on main b1cdb0ddd80e)

Candidate: `9d7e56acc2e9742d6dec3943297f579d609c08b5` (base `1a00087c5bd7dcd8703c7506086a1c5f089f3b13`).
Verdict: **approved**.

## Scope and resolution

The diff from the assigned base to the candidate is docs-only: it adds
`specs/tasks/task-241.md` (104 lines). No product code, scripts, or exemption
files change on this branch. That is the correct resolution, not a gap: both
recorded baseline failures were raised against main `b1cdb0dd`, and that
commit is an ancestor of the assigned base `1a00087c` — the failures were
already repaired on main by task-239 (commits `3e62cdbb`, `483bf8ad`,
`7a820241`, all ancestors of the base and all recorded in
`specs/tasks/task-239.md`'s `commits:` frontmatter). Shipping new product
changes here would have been fabrication; the assignment's real work is
verifying the repair holds at the assigned head, which the candidate does.

## Independent verification at the candidate (evidence in /tmp/stage/review-evidence/)

Baseline failure 1 — `check-byte-slice-truncation` (F4-class
`&raw_preview[..500]` at `explorer_ws.rs:2937`):

- `bash scripts/check-byte-slice-truncation.sh` → OK, exit 0.
- The fixed site (`crates/gyre-server/src/explorer_ws.rs:2918-2930`) now
  routes the invalid-view-query preview through
  `crate::gate_executor::truncate_bytes`, whose implementation
  (`gate_executor.rs:844-856`) is genuinely char-boundary-safe
  (`char_indices` + `take_while(i <= limit)`, backing off to the previous
  boundary; empty-string result when no boundary fits).
- The repair was done the hard way, not the exempt way: commit `3e62cdbb`
  deleted the line-pinned exemption entry from
  `scripts/byte-slice-truncation-exemptions.txt` rather than re-pinning it,
  and no exemption file has grown since (base→candidate exemption diff is
  empty).
- The check script itself is byte-identical between `b1cdb0dd` and the
  candidate — no weakening.
- Kill-test quality: `truncate_bytes_multibyte_boundary_does_not_panic`
  (2049 × 2-byte `é`, cut lands mid-character) and
  `truncate_bytes_leading_multibyte_backs_to_zero` (4-byte emoji, limit 2 →
  empty) both panic or fail under a fixed byte-index slice;
  `cargo test -p gyre-server --lib truncate_bytes` → 3/3 passed.
- `cargo test -p gyre-server --lib explorer_ws::` → 35/35 passed.

Baseline failure 2 — `check-task-commit-attribution` (task-170 commit
`b1cdb0dd` missing from its task's `commits:` frontmatter):

- `bash scripts/check-task-commit-attribution.sh` → OK, exit 0 (full
  history).
- Full SHA `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` is recorded in
  `specs/tasks/task-170.md`'s `commits:` frontmatter (line 15).
- The check script is byte-identical between `b1cdb0dd` and the candidate.

## Contract conformance

- No check weakened, skipped, or exempted; no exemption file grew
  (verified by diffing every `scripts/*exemption*` path base→candidate:
  empty).
- Task-241's own `commits: []` frontmatter is accurate: the only candidate
  commit `9d7e56ac` is the docs commit adding the task file, which is not
  product-surface; the attribution gate confirms.
- Minor note, not a finding: the Shipped section cites probe results "at
  HEAD `f2f8ad7a`" (the merge commit) while the final candidate is
  `9d7e56ac`; since `9d7e56ac` differs from `f2f8ad7a` only by the task doc,
  those results carry, and this review re-ran every probe independently at
  `9d7e56ac` with the same outcomes.

Findings: none.
