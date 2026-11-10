---
title: "Repair verified failure on main 05709c242509"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `05709c242509b89214876339b3c463ede3a31b60`
Environment fingerprint: `host-2d923684a67db7534a4efe78b8f15c3b59b03752982e1d6fd37c3e3a69d1d9ee`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 4598 files, 16.2GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/tools/checks.sh
rustfmt: changed lines clean (0 Rust files checked)
clippy: changed lines clean (0 Rust files, 343 existing warnings outside changes)
Architecture lint passed: gyre-domain has no forbidden dependencies or I/O.
Hierarchy lint passed: all hierarchy fields are non-optional.
OK: all registered /api/v1/ routes resolve in the ABAC registry (or are exempted legacy entries).
check-abac-exempt-handlers: OK (89 handler(s) checked)
check-mcp-write-tools: OK (8 write-capable tool(s) checked, all gated)
OK: no duplicate Diesel migration versions.
OK: no dialect-only SQL in shared migrations.
OK: every MessageKind variant has an emitter (or documented exemption).
check-byte-slice-truncation: OK
check-relative-path-defaults: OK
OK: no fail-open .unwrap_or_default()/.unwrap_or("") on resolve_ref() results.
FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:

  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/41507847ee164e7d81a67dc60dbc65fe/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "05709c242509b89214876339b3c463ede3a31b60", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  05709c24  task-196  feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-bdzxlyw1/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Status: repaired.** The verified failure at base `05709c242509b89214876339b3c463ede3a31b60` was live and is fixed with the check's documented remedy.

- **Reproduction at assignment HEAD** (tree = base + untracked task file): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `05709c24 task-196 feat(task-196): Ground Briefing Q&A in real briefing data with sources and history validation` — a product-surface commit (+613 lines across `crates/gyre-server/src/api/graph.rs`, `web/src/__tests__/Briefing.test.js`, `web/src/components/Briefing.svelte`, `web/src/lib/InlineChat.svelte`) missing from `specs/tasks/task-196.md`'s `commits:` frontmatter. Evidence: `task-227-attribution-before.txt` (exit 1, identical violation text).
- **Root cause — squash-drift class (tasks 213/219/222/224 precedent):** task-196's frontmatter recorded three pipeline-branch SHAs (`3b90956c` checkpoint-recover, `abcfff04` rustfmt fix, `88b57180` checkpoint-recover) — none is an ancestor of HEAD (verified with `git merge-base --is-ancestor` for each); the branch work was squashed into the landing commit `05709c24`, which cannot contain its own SHA at squash time, so the recorded list stayed one entry short and the entire landed surface was invisible to review scoping (task-095 R3-F4 flaw class). The landed tree is content-identical to the recorded branch tree `88b57180` on task-196's four product files (verified: `git diff 05709c24 88b57180 -- <the four files>` is formatting-only deltas from the `abcfff04` rustfmt pass).
- **Repair:** appended the full SHA `05709c242509b89214876339b3c463ede3a31b60` to `specs/tasks/task-196.md`'s `commits:` frontmatter, joining the 3 SHAs already recorded. This is the check's own documented remedy — same repair shape as task-224's `a11ba8d3` for task-068 and task-219's `e96d25ab` for task-200. No exemptions added; `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS source changed (`git diff 05709c24` touches only `specs/tasks/`).
- **Probe after repair:** exit 0 — `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` Evidence: `task-227-attribution-after.txt`.
- **Mutation check (test-the-repair):** with the repair present, removing the SHA from the frontmatter re-fails the gate with the identical violation (exit 1), and restoring it re-passes (exit 0) — the pass is attributable to the recorded SHA, not gate drift. Evidence: `task-227-mutation-check.txt`, `task-227-mutation-restore-check.txt`.
- **All deterministic gates re-run at the repaired tree** (the baseline's `checks.sh` set, run script-by-script): architecture, ABAC route registry, ABAC exempt handlers, MCP write tools, migration versions, migration SQL portability, dead message kinds, byte-slice truncation, relative-path defaults, fail-open ref resolution, mem port contracts, fabricated scope defaults, lossy secret conversion, scope literal defaults, inert enforcement, in-memory state stores, unbounded external HTTP, forwarded-header trust, forged scope fields, task-commit-attribution — all exit 0; `python3 scripts/check-rustfmt-diff.py 05709c24` — "changed lines clean" (0 Rust files changed); `git diff --check 05709c24` — clean. Evidence: `task-227-all-checks.txt`.
- **Attribution for this task:** this repair round touches only `specs/tasks/task-196.md` and `specs/tasks/task-227.md` — no product surface — so `commits: []` is correct (matching the task-224 precedent where `dev-attribution.py` produced no change for a specs-only repair round).
- **Transport restriction:** this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime surface was touched, so no HTTP probe is applicable; exact-head GitHub checks belong to host verification and remain mandatory.

Independent review, full deterministic gates, and GitHub checks on the exact PR head remain required before merge. All evidence above is under `/tmp/stage/review-evidence/`.
