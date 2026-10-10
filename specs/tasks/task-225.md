---
title: "Repair verified failure on main a11ba8d32859"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: [task-228]
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0`
Environment fingerprint: `host-a2d5a882e2530828f387b4a1b78b8f8d82090ec71dd1ef552437e858bcfb9074`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/tools/checks.sh
rustfmt: changed lines clean (2 Rust files checked)
crates/gyre-server/src/lib.rs:1678: clippy::needless_borrows_for_generic_args: the borrowed expression implements the required traits
    Checking gyre-common v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-common)
   Compiling gyre-server v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-server)
warning: gyre-server@0.1.0: SKIP_WEB_BUILD=1 set, skipping web build
    Checking gyre-domain v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-domain)
    Checking gyre-cli v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-cli)
    Checking gyre-ports v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-ports)
    Checking gyre-adapters v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/crates/gyre-adapters)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 19s

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

  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/90ba757ff000451cb2587c99601d0916/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  a11ba8d3  task-068  feat(task-068): Graph Summary & Dry-Run MCP Tools\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-loo_b255/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Root cause:** the verified baseline failure (probe `bash scripts/check-task-commit-attribution.sh`, exit 1) was commit-attribution drift of the task-095 R3-F4 flaw class: product-surface commit `a11ba8d3` (`feat(task-068): Graph Summary & Dry-Run MCP Tools`, touches `crates/gyre-domain/src/view_query_resolver.rs`, `crates/gyre-server/src/explorer_ws.rs`, `crates/gyre-server/src/mcp.rs`, `crates/gyre-server/tests/graph_integration.rs`) was missing from `specs/tasks/task-068.md`'s `commits:` frontmatter. The squash landing commit cannot contain its own SHA, so the task's recorded list stayed one entry short and the landing surface was invisible to review scoping — the same drift class task-219 repaired for `6bf777a6`/task-200 and task-222 verified.

**Repair** (the check's documented remedy, commit `c7a4b844`): appended the full SHA `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` to `specs/tasks/task-068.md`'s `commits:` frontmatter, joining the 9 SHAs already recorded from the task's branch. No exemptions added — `scripts/task-commit-attribution-exemptions.txt` untouched at its frozen 3-entry baseline; no check, skip, or gate weakened; no Rust/JS product source changed (diff is specs-only).

**Verification evidence** (all under `/tmp/stage/review-evidence/`, timestamps 2026-10-10):

- **Reproduced at assignment HEAD** (`a11ba8d3`, tree equal to base plus only the untracked task file): `bash scripts/check-task-commit-attribution.sh` exited 1 with the identical violation — `a11ba8d3 task-068 feat(task-068): Graph Summary & Dry-Run MCP Tools` (evidence: `task-225-attribution-before.txt`, `task-225-head-before.txt`).
- **Probe after repair**: `bash scripts/check-task-commit-attribution.sh` → OK, exit 0 (evidence: `task-225-attribution-after.txt`); re-confirmed at committed HEAD `c7a4b844` (`task-225-attribution-at-head.txt`, `task-225-head-after.txt`).
- **Mutation check (test-the-repair)**: with the repair present, removing `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` from task-068's `commits:` frontmatter re-fails the gate with the identical violation (exit 1), and restoring it re-passes (exit 0) — the current pass is attributable to the recorded SHA, not gate drift (evidence: `task-225-mutation-check.txt`, `task-225-mutation-restore-check.txt`).
- **No gate weakened**: `scripts/` untouched — the diff is `specs/tasks/task-068.md` + this task record only; exemption file at frozen 3-entry baseline (re-counted post-repair). No Rust/JS surface touched: diff on `crates/`/`web/` is empty.
- **Other gates on HEAD `c7a4b844`** (all exit 0): `check-arch.sh`, `GYRE_CHECK_HIERARCHY=1 check-hierarchy.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh` (89 handlers), `check-mcp-write-tools.sh` (8 tools), `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-relative-path-defaults.sh`, `check-fail-open-ref-resolution.sh`, `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh`, `check-forwarded-header-trust.sh`, `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`, plus the repaired `check-task-commit-attribution.sh` — all outputs preserved in `task-225-static-gates.txt` (19 static gates + hierarchy lint = 20 OK).
- **Baseline-log rustfmt/clippy lines dispositioned, not acted on.** The log's rustfmt/clippy lines (`crates/gyre-server/src/lib.rs:1678: clippy::needless_borrows_for_generic_args`) print *without* a `GYRE_BASELINE_FAILURE_JSON` prefix and do not resolve against this history: `lib.rs` is 1592 lines at assignment HEAD — line 1678 does not exist; the log's own paths (`attempts/90ba757ff000451cb2587c99601d0916/1/checkout/...`) name a superseded foreign checkout, the same artifact class task-218/task-222 documented. `python3 scripts/check-rustfmt-diff.py a11ba8d3` at HEAD reports `changed lines clean (0 Rust files checked)` — diff touches no Rust files (evidence: `task-225-clippy-disposition.txt`).
- **Attribution for this task**: `python3 /tmp/stage/dev-attribution.py task-225` produced no change — `commits: []` is correct: the repair commit `c7a4b844` is `process(task-068)`-typed (skipped by the attribution check's review/process round-trip skip) and touches `specs/` only, no product surface.
- **Transport restriction**: this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime surface was touched, so no HTTP probe is applicable; exact-head GitHub checks belong to host verification and remain mandatory.
- **Durable recurrence cause** (already recorded by task-215/222, re-confirmed against current source): `publish()` in `scripts/pipeline/stages.py` (lines ~281–292) learns the squash `mergeCommit.oid` but only stores it in delivery metadata — it never appends the landed SHA to the task's `commits:` frontmatter on main, so every future product-surface ship re-creates this drift and the pipeline will keep minting repair tasks for it. A pipeline-side fix (record the merge SHA in the task file in the merge-confirmation path) is out of scope for this repair round.


### Round 2 (finding 374e273d, base `05709c242509b89214876339b3c463ede3a31b60`, prerequisite task-228)

While round 1 awaited publication, main advanced past the assignment base:
`05709c24` (`feat(task-196)`, +613 lines across `crates/gyre-server/src/api/graph.rs`,
`web/src/__tests__/Briefing.test.js`, `web/src/components/Briefing.svelte`,
`web/src/lib/InlineChat.svelte`) landed and re-created the identical drift class;
the pipeline minted finding `374e273d` and attached it to this task as
`depends_on: [task-228]`. Task-227 (commit `18c44f1a`) repaired it on main by
appending `05709c242509b89214876339b3c463ede3a31b60` to `specs/tasks/task-196.md`'s
`commits:` frontmatter; task-228 (commit `06d70009`) recorded that repair. The
pipeline merged main into this branch at `7401f8c1` before this round began, so
the repair is present here; this round reproduced the failure at its base and
verified the repair holds at merged HEAD, without re-landing an already-landed
one-line change.

**Reproduction at the finding's base** (detached worktree at
`05709c242509b89214876339b3c463ede3a31b60`): `bash scripts/check-task-commit-attribution.sh`
exited 1 with the identical violation — `05709c24 task-196 feat(task-196): Ground
Briefing Q&A in real briefing data with sources and history validation`. At that
tree, task-196's frontmatter lists only the three branch SHAs (`3b90956c`,
`abcfff04`, `88b57180`) — the squashed landing commit cannot contain its own SHA
(task-095 R3-F4 flaw class). Evidence:
`/tmp/stage/review-evidence/task-225-r2-attribution-before.txt` (exit 1).

**Mutation check at merged HEAD `7401f8c1` (test-the-repair):** repair present →
gate exits 0; removing `05709c242509b89214876339b3c463ede3a31b60` from task-196's
`commits:` frontmatter (sed mutation) → identical FAIL exit 1; repair restored →
exit 0 again. The current pass is attributable to the recorded SHA, not gate
drift. Evidence: `task-225-r2-mutation-check.txt` (mutated state, exit 1) and
`task-225-r2-mutation-restore-check.txt` (restored state, exit 0, frontmatter
line shown with all 4 SHAs).

**No gate weakened, no product source touched:** `git diff 05709c242509b89214876339b3c463ede3a31b60 HEAD -- scripts/`
is empty; diff on `crates/`/`web/src`/`web/tests` is empty; the exemption file
sits at its frozen 3-entry baseline (`01493c88 task-097`, `17c81d5a task-072`,
`a8d036f4 task-091`) — count re-verified post-round. The branch delta vs the
failure base is exactly: `specs/tasks/task-196.md` (the landed task-227 repair),
`specs/tasks/task-227.md`, `specs/tasks/task-228.md`, and this task record.
Evidence: `task-225-r2-no-gate-weakened.txt`.

**All 21 static gates at merged HEAD `7401f8c1`** exit 0 (architecture, hierarchy,
ABAC registry + exempt handlers, MCP write tools, migration versions + SQL
portability, dead message kinds, byte-slice truncation, relative path defaults,
fail-open ref resolution, mem port contracts, fabricated scope defaults, lossy
secret conversion, scope literal defaults, inert enforcement, forged scope
fields, forwarded header trust, in-memory state stores, unbounded external HTTP,
and the repaired task-commit attribution). Evidence:
`/tmp/stage/review-evidence/task-225-r2-static-gates.txt` (21 × exit=0).

**Attribution for this task:** `python3 /tmp/stage/dev-attribution.py task-225`
derives no change — `commits: []` is attribution-canonical: the branch's own
commits are the round-1 `process(task-068)`/`process(task-225)` specs-only
commits and the main merge; nothing touches product surface.

**Transport restriction:** this sandbox cannot accept TCP (`accept(): [Errno 95]
Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime
surface was touched, so no HTTP probe is applicable; exact-head GitHub checks
belong to host verification and remain mandatory.

Independent review, full deterministic gates, and GitHub checks on the exact PR head remain required before merge.
