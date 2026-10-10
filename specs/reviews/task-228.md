# Review — task-228 (Repair verified failure on main 05709c242509)

Spec: GOAL.md — real implementations and meaningful verification. Assigned task: reproduce and repair the verified upstream failure of `scripts/check-task-commit-attribution.sh` at base `05709c242509b89214876339b3c463ede3a31b60`, without weakening any check, adding exemptions, or implementing the blocked feature.
Candidate under review: `1e5400c7952bafa1d88b654650177cab81816d45` (branch `pipeline/task-228/636cdd2a4ee54d0d96140d977650d686-1`, tree clean). Review scoping per `commits:` frontmatter: `commits: []` — verified attribution-canonical (below), and the diff vs base confirms the entire branch is specs-only.
Verdict: **complete**.

## Diff surface vs base `05709c24`

`git diff --stat 05709c242509b89214876339b3c463ede3a31b60 1e5400c7` = exactly two files:

- `specs/tasks/task-196.md` — 1 line: `commits:` frontmatter gains the 4th entry `"05709c242509b89214876339b3c463ede3a31b60"` (the three branch SHAs `3b90956c`/`abcfff04`/`88b57180` preserved).
- `specs/tasks/task-228.md` — the task file itself (contract + operational history).

`git diff <base> <candidate> -- scripts/ crates/ web/src/ web/dist/ web/tests/` is empty: zero production source, zero gate, zero exemption-file changes. All five branch commits (`edb132c3`, `d3fff96d`, `dc5c49d7`, `0e8e1e68`, `1e5400c7`) are pipeline checkpoints touching only `specs/tasks/` — none is product-surface, none needs frontmatter attribution beyond `commits: []`.

## The failure and root cause (independently confirmed)

`05709c24` ("feat(task-196): Ground Briefing Q&A in real briefing data…") touches `crates/` and `web/src` (product surface) and carries the `task-196` label, but task-196's frontmatter at base listed only the 3 branch SHAs — the squash-landing commit cannot contain its own SHA, so the recorded list stayed one entry short and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class). Verified: `git show 05709c242509…:specs/tasks/task-196.md` has the 3-entry `commits:` list.

The repair is the check's own documented remedy (check header lines 28–30: "Fix drift by adding the SHA to the task's frontmatter … not by growing the file") and the same repair shape as the two recorded precedents: task-224's `770785f7` appending `a11ba8d3…` to task-068's list, and task-219's `e96d25ab` resolving the `6bf777a6`/task-200 drift (task-200's list now ends with the full landing SHA). `frontmatter_commits()` truncates recorded SHAs to 8 chars (`cut -c1-8`), so the 40-char entry matches the `^05709c24$` scan key.

## Probes (all this checkout, evidence under /tmp/stage/review-evidence/)

- **Check at candidate**: `bash scripts/check-task-commit-attribution.sh` → exit 0, `OK: every task-labeled product-surface commit is recorded…` (`task-228-r4-attribution-candidate.txt/.exitcode`).
- **Reproduction (mutation, test-the-repair)**: repair SHA stripped from task-196's `commits:` via sed → exit 1 with the identical baseline violation `05709c24 task-196 feat(task-196): Ground Briefing Q&A…` (`task-228-r4-attribution-repro.txt/.exitcode`); restored → exit 0 (`task-228-r4-attribution-restored.txt/.exitcode`). Tree confirmed clean after restoration (`git status --porcelain` empty; task-196.md byte-identical to the candidate). The pass is attributable to the recorded SHA, not gate drift.
- **No weakening**: `scripts/task-commit-attribution-exemptions.txt` at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`, `a8d036f4 task-091`); diff vs base empty.
- **Full static gate battery** (the same 20-check `tools/checks.sh` set that produced the baseline failure; the two diff-scoped lints are trivially clean — 0 Rust files changed): all 19 runnable scripts exit 0 at the candidate — arch, ABAC route registry, ABAC exempt handlers, MCP write tools, migration versions, migration SQL portability, dead message kinds, byte-slice truncation, relative path defaults, fail-open ref resolution, mem port contracts, fabricated scope defaults, lossy secret conversion, scope literal defaults, inert enforcement, forwarded-header trust, in-memory state stores, unbounded external HTTP, forged scope fields, plus the repaired attribution check (`task-228-r4-full-gate-battery.txt`).
- **Contract integrity**: `dev-contract.py requirement_parts()` on the candidate task file yields frontmatter `title: "Repair verified failure on main 05709c242509"` / `spec_ref: "GOAL.md — real implementations and meaningful verification"` / `depends_on: []` and prose containing exactly the Required behavior + Base + fingerprint; `Baseline failure` and `Shipped` sections are correctly classified operational and excluded from the generation (`task-228-r4-contract-hash-proof.txt`). This repairs the prior round's contract finding (dropped `spec_ref`).
- **`commits: []` is attribution-canonical**: `python3 scripts/dev-attribution.py task-228` derives an empty list — `origin/main` is exactly the base `05709c24`, so the branch scope `origin/main..HEAD` is the 5 specs-only checkpoint commits, none touching `crates/|web/src|web/tests` (`task-228-r4-attribution.txt`).

No Rust/JS behavioral surface changed on this branch, so no runtime probes (server/browser/cargo) apply; the sandbox's unsupported TCP listener is not relevant to a specs-only repair. Verification beyond the focused probes above belongs to full verification on the merging host.

Findings: none. The failure is genuinely repaired by the smallest correct production bookkeeping change, with reproduction and mutation evidence, no gate weakened, and no scope creep.

— Reviewer, 2026-10-10
