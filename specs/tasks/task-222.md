---
title: "Repair verified failure on main 6bf777a6a44f"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `6bf777a6a44f28052ed5af28bf6fb013fde6df48`
Environment fingerprint: `host-c94ece55c13b0dae3c1370132077f913beaf67036e4aed42e9b0fd4a3c1276f9`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 261 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/tools/checks.sh
rustfmt: changed lines clean (6 Rust files checked)
crates/gyre-domain/src/call_graph_resolve.rs:306: clippy::unnecessary_map_or: this `map_or` can be simplified
crates/gyre-domain/src/call_graph_resolve.rs:306: clippy::unnecessary_map_or: this `map_or` can be simplified
crates/gyre-adapters/src/call_graph.rs:233: clippy::await_holding_lock: this `MutexGuard` is held across an await point
crates/gyre-adapters/src/call_graph.rs:269: clippy::await_holding_lock: this `MutexGuard` is held across an await point
    Checking gyre-common v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-common)
   Compiling gyre-server v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-server)
warning: gyre-server@0.1.0: SKIP_WEB_BUILD=1 set, skipping web build
    Checking gyre-domain v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-domain)
    Checking gyre-cli v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-cli)
    Checking gyre-ports v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-ports)
    Checking gyre-adapters v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/crates/gyre-adapters)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 14s

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
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/e50a8815cb564b458bab22fc5e11b2f9/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "6bf777a6a44f28052ed5af28bf6fb013fde6df48", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  6bf777a6  task-200  feat(task-200): Message bus \u2014 per-kind payload schema validation (reject invalid payloads with 400)\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-7wb09y5_/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

**Status: the verified failure is already repaired on this assignment's base.** The repair landed as task-219 (ship commit `e96d25abcdbb51f8890ea36d11541bfa9f80a2b8`), which is exactly this task's assignment base and this branch's starting HEAD — the pipeline re-dispatched the same verified failure after its repair had merged to main (same duplicate-dispatch pattern as tasks 215/216/218 for the `f4acb4eb` drift). No new code change is needed or possible without fabricating work: the failure's only root cause on main was `6bf777a6` missing from `specs/tasks/task-200.md`'s `commits:` frontmatter, and that record exists at base.

What this round did — verified the repair holds and left evidence:

- **Reproduction at the exact failing commit.** Detached worktree at `6bf777a6a44f28052ed5af28bf6fb013fde6df48` (`git worktree add --detach`): `bash scripts/check-task-commit-attribution.sh` exited 1 with the identical violation — `6bf777a6 task-200 feat(task-200): Message bus — per-kind payload schema validation (reject invalid payloads with 400)` — a product-surface commit (touches `crates/gyre-common/src/message.rs`, `crates/gyre-server/src/api/messages.rs`, `crates/gyre-server/src/mcp.rs`) absent from task-200's `commits:` frontmatter. Root cause of the original drift: the squash landing commit cannot contain its own SHA, so the task's recorded list stayed one entry short and the landing surface was invisible to review scoping (task-095 R3-F4 flaw class). Evidence: `attribution-before-task222.txt`, worktree removed after use.
- **Probe at assignment HEAD** (`e96d25ab` == `origin/main`, branch carries only this task file): exit 0, `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` Evidence: `attribution-after-task222.txt`, `head-before.txt`.
- **Mutation check (test-the-repair)**: with task-219's repair present, removing `6bf777a6a44f28052ed5af28bf6fb013fde6df48` from `specs/tasks/task-200.md`'s `commits:` frontmatter re-fails the gate with the identical violation (exit 1), and restoring it re-passes (exit 0) — the current pass is attributable to the recorded SHA, not gate drift. Evidence: `mutation-check-task222.txt`, `mutation-restore-check-task222.txt`; frontmatter restored via `git checkout --`.
- **No gate weakened.** `scripts/` untouched: `git diff 6bf777a6..HEAD -- scripts/` is empty; `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen 3-entry baseline (`exemption-count-task222.txt`); `git diff 6bf777a6..HEAD --name-only` shows only `specs/tasks/task-200.md` (task-219's repair) and `specs/tasks/task-219.md` (its record). No Rust/JS product surface touched: diff on `crates/`/`web/` is empty.
- **Baseline-log clippy lines dispositioned, not acted on.** The log's rustfmt/clippy lines (`call_graph_resolve.rs:306 unnecessary_map_or`, `call_graph.rs:233/269 await_holding_lock`) print without a `GYRE_BASELINE_FAILURE_JSON` prefix and do not resolve against this history: `call_graph.rs` is 245 lines (flagged line 269 does not exist), `call_graph_resolve.rs` contains no `map_or` at all, and the referenced lines sit in `#[tokio::test]` bodies. They are foreign-checkout artifacts from the superseded attempt (`attempts/e50a8815cb564b458bab22fc5e11b2f9/1`, named in its own paths), the same artifact class task-218 documented for the `f4acb4eb` baseline log. Evidence: `clippy-disposition-task222.txt`. CI's clippy gate is diff-based (`check-clippy-diff.py HEAD^1`) and this branch changes no Rust files: `check-rustfmt-diff.py 6bf777a6` → `changed lines clean (0 Rust files checked)` exit 0 (`rustfmt-diff-task222.txt`).
- **Attribution for this task**: `python3 /tmp/stage/dev-attribution.py task-222` produced no change — `commits: []` is correct: the repair itself belongs to task-219's record; this branch adds only this task record, no product surface.
- **Transport restriction**: this sandbox cannot accept TCP (`accept(): [Errno 95] Operation not supported`, recorded in `/tmp/stage/capabilities.json`). No runtime surface was touched, so no HTTP probe is applicable; exact-head GitHub checks belong to host verification and remain mandatory.
- **Durable recurrence cause** (already recorded by task-215 as needing its own task, re-confirmed here against current source): `publish()` in `scripts/pipeline/stages.py` (lines 281–292) learns the squash `mergeCommit.oid` but only stores it in delivery metadata — it never appends the landed SHA to the task's `commits:` frontmatter on main, so every future product-surface ship re-creates this drift and the pipeline will keep minting repair tasks for it. A pipeline-side fix (record the merge SHA in the task file in the merge-confirmation path) is out of scope for this repair round.

Independent review, full deterministic gates, and GitHub checks on the exact PR head remain required before merge. All evidence above is under `/tmp/stage/review-evidence/`.
