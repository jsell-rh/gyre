---
title: "Repair verified failure on main 6bf777a6a44f"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `6bf777a6a44f28052ed5af28bf6fb013fde6df48`
Environment fingerprint: `host-076b557dafaf763281cc2c8e33a955d7b7961e79630fba73e186761a001ca796`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 4598 files, 16.2GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/tools/checks.sh
rustfmt: changed lines clean (2 Rust files checked)
clippy: changed lines clean (2 Rust files, 347 existing warnings outside changes)
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

  6bf777a6  task-200  feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/d3765144aa824f43bedaf67ac48496db/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "6bf777a6a44f28052ed5af28bf6fb013fde6df48", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  6bf777a6  task-200  feat(task-200): Message bus \u2014 per-kind payload schema validation (reject invalid payloads with 400)\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-07dzv48c/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

- Reproduced the verified baseline failure at assignment HEAD (`6bf777a6`, tree equal to base plus only the untracked `specs/tasks/task-221.md`): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `6bf777a6  task-200  feat(task-200): Message bus - per-kind payload schema validation (reject invalid payloads with 400)` (evidence: `/tmp/stage/review-evidence/attribution-before.txt`, HEAD recorded in `head-before.txt`). The ship commit `6bf777a6` is task-200's product-surface commit on main (touches `crates/gyre-common/src/message.rs` +600, `crates/gyre-server/src/api/messages.rs` +116, `crates/gyre-server/src/mcp.rs` +107, `docs/api-reference.md`) but task-200's `commits:` frontmatter recorded only its 13 pre-ship lineage SHAs - a commit cannot contain its own hash (chicken-and-egg), so the shipped surface was invisible to review scoping (task-095 R3-F4 flaw class). Same drift class as tasks 211/216/218 repaired before.
- Repair (the check's documented remedy): appended the full SHA `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to `specs/tasks/task-200.md`'s `commits:` frontmatter list, joining the 13 SHAs already recorded - append-only, review scope grows, never shrinks. All 14 frontmatter SHAs resolve in this history (`git cat-file -e` verified, 14/14). No exemptions added - `scripts/task-commit-attribution-exemptions.txt` untouched, still frozen at its 3-entry baseline; no check, skip, or gate weakened.
- Probe after fix: `bash scripts/check-task-commit-attribution.sh` -> `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` exit 0 (evidence: `attribution-after.txt`).
- Other baseline-log probes verified clean at this HEAD: `python3 scripts/check-rustfmt-diff.py 6bf777a6a44f28052ed5af28bf6fb013fde6df48` -> `changed lines clean (0 Rust files checked)` exit 0 (repair touches `specs/` only); `git diff --check 6bf777a6a44f...` clean exit 0. Clippy unchanged from baseline (no Rust files touched). No cargo build or suite run - no Rust source changed; per the smallest-relevant-probe constraint and this sandbox's unsupported listener probe (`capabilities.json` tcp errno 95), HTTP-surface verification is deferred to host verification and required GitHub CI.
- Attribution for this task: `commits: []` is correct - the only branch commit touches `specs/` only, no product surface. Verified with `python3 /tmp/stage/dev-attribution.py task-221` (no change produced to the list).
- Independent review, full deterministic gates, and GitHub checks remain required before merge (per assignment).
