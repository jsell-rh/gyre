---
title: "Repair verified failure on main b1cdb0ddd80e"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["483bf8ad72a38505d47f5a8cb47bae619a4648da"]
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3`
Environment fingerprint: `host-7e6e750d5c78b3678c60ecb74674f1c32cb1e066757845f2fe73b675591b69bf`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 261 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/tools/checks.sh
specs/tasks/task-214.md:114: new blank line at EOF.
rustfmt: changed lines clean (5 Rust files checked)
clippy: changed lines clean (5 Rust files, 343 existing warnings outside changes)
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

  b1cdb0dd  task-170  feat(task-170): View Specification Grammar — TypeScript types and server-side validation

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/3a0448a1cdfb480f81ee57cfadb6afe4/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  b1cdb0dd  task-170  feat(task-170): View Specification Grammar \u2014 TypeScript types and server-side validation\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-iz2lr8ky/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped (round 1: commit-attribution gate repair)

**Reproduction at assignment HEAD** (tree = base `b1cdb0ddd` plus this
untracked task file; `git diff b1cdb0dd -- crates/ web/ scripts/` empty):
`bash scripts/check-task-commit-attribution.sh` exited 1 listing
`b1cdb0dd task-170 feat(task-170): View Specification Grammar — TypeScript
types and server-side validation` — a product-surface commit (touches
`crates/gyre-common/src/view_spec.rs`,
`crates/gyre-server/src/api/explorer_views.rs`, `web/src/lib/**`,
`web/src/__tests__/**`) missing from `specs/tasks/task-170.md`'s
`commits:` frontmatter. Root cause is the squash-drift class recorded by
tasks 213/219/222/224/229: the squashed landing commit cannot contain its
own SHA, so the task's recorded list stayed one entry short and the
landing surface was invisible to review scoping (task-095 R3-F4 flaw
class). Evidence: `task-239-reproduction-before-repair.txt`.

**Repair**: appended the full SHA
`b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` to `specs/tasks/task-170.md`'s
`commits:` frontmatter, joining the 11 SHAs already recorded from the
task's branch (kept, not collapsed — same repair shape as task-229's
`task-196` fix and task-224's `task-068` fix that landed on main). This is
the check's own documented remedy. No exemptions added;
`scripts/task-commit-attribution-exemptions.txt` untouched at its frozen
baseline (24 lines / 3 entries);
no check, skip, or gate weakened; no Rust/JS source changed.

**Probe after repair**: exit 0 — `OK: every task-labeled product-surface
commit is recorded in its task's commits: frontmatter (or exempted legacy
drift).` Evidence: `task-239-attribution-after.txt`.

**Mutation check (test-the-repair)**: with the appended SHA removed via
sed, the gate re-fails with the identical violation (exit 1); restoring
it re-passes (exit 0). Evidence: `task-239-mutation-check.txt`. The pass
is attributable to the recorded SHA, not gate drift.

**Attribution for this task**: `python3 /tmp/stage/dev-attribution.py
task-239` produces no change (branch vs `origin/main` carries no
product-surface commits this round — only `specs/tasks/task-170.md` and
this task file), so `commits: []` is correct.

**Note on the baseline log's `task-214.md:114: new blank line at EOF`
warning**: that lint fired in the pipeline's baseline checkout state
(`tools/checks.sh` is pipeline-external; the file flagged there does not
exist at this HEAD), it is a warning not a FAIL, and the only gate FAIL in
the baseline log was the attribution check repaired above. Files touched
this round end with a single trailing newline.

Independent review, full deterministic gates, and GitHub checks on the
exact PR head remain required before merge. All evidence is under
`/tmp/stage/review-evidence/` (paths relative to that directory).

## Round 2 baseline failure (verified)

```text

$ cargo test --all --quiet            # on candidate 7cd9dce69b9765420fcd2ea7826d4b7fed091cc1
...
running 7 tests
... 3/7
explorer_ws_save_and_load_view --- FAILED

---- explorer_ws_save_and_load_view stdout ----

thread 'explorer_ws_save_and_load_view' (2954388) panicked at crates/gyre-server/tests/explorer_ws_integration.rs:233:5:
Should receive view_query on load

failures:
    explorer_ws_save_and_load_view

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.19s

error: test failed, to rerun pass `-p gyre-server --test explorer_ws_integration`
```

Attempt artifact: `68f83ea6262f4a07b7ae9a1901827150` (exit 101).

## Shipped (round 2: explorer WS session-limit scoping)

**Root cause (real production defect, not a flaky test to paper over):**
the explorer WebSocket per-user concurrent session limit (default 3,
oldest-first eviction) was tracked in a PROCESS-GLOBAL static
(`ACTIVE_SESSIONS` + `SESSION_ID_COUNTER` in `explorer_ws.rs`) keyed by
`tenant_id:agent_id`. The limit is a property of a server instance, not
of the OS process: every connection routed through a different
`AppState` in the same process competed for one shared cap. All seven
tests in `explorer_ws_integration.rs` authenticate with the same global
dev token (principal `"default"`:`"system"`) and each runs its own
`build_state()` server in one test process, so once a 4th test held a
live session the oldest mid-flight session was evicted ("Session
replaced by a newer connection.") and its socket closed; the victim
exhausted its read loop and failed. This matches the pipeline log: the
binary failed in 0.19s, far below the 5s per-read timeout, i.e. a closed
stream, not a timeout. Scheduling-dependent, hence flaky under the full
parallel suite.

**Repair (production code, commit `483bf8ad`):** replaced the statics
with `ExplorerSessionRegistry` (register/unregister, oldest-first
eviction, same `std::sync::Mutex` + `Notify` mechanics) owned by
`AppState` and wired in `build_state`, `mem.rs` test constructor, and
the `middleware.rs` test constructor. Matches the sibling ephemeral-
state precedent (`compose_sessions`, `process_registry`): the state is
connection-lifetime-only (WebSocket sessions cannot outlive their
server instance), intentionally not port-backed; `check-in-memory-state-stores.sh`
passes. Single-instance production behavior is unchanged.

**Regression test:**
`explorer_ws_session_limit_is_per_server_instance` opens 3 sessions on
server A, one same-user session on server B in the same process, and
asserts A's oldest session still answers (`list_views` → `views`); then
asserts the cap is still enforced WITHIN one server (4th connection
evicts the oldest with a "replaced" error).

**Evidence** (`/tmp/stage/review-evidence/`):
- `task-239-reproduce-prefail.txt` — regression test on UNFIXED code:
  FAILED, exit 101, "Session replaced by a newer connection." — the
  failure mechanism reproduced deterministically.
- `task-239-final-test-binary.txt` — full `explorer_ws_integration`
  binary at fix commit: 8/8 passed, exit 0 (also 5 consecutive green
  runs during development).
- `task-239-gyre-server-suite.txt` — `cargo test -p gyre-server` (lib +
  all integration binaries, incl. 1213 lib tests): all green, exit 0.
- `task-239-clippy-diff.txt` — all-target Clippy attempt was
  INTERRUPTED at the pipeline stage boundary (per operator); NOT passed,
  NOT evidence. The independent host verifier owns this mandatory gate
  on the exact candidate.
- Changed-lines rustfmt gate (`check-rustfmt-diff.py` vs pre-change
  HEAD `3c772043`): clean, exit 0.

No checks weakened, no skips or exemptions added, no test assertions
removed. The previously-failing `explorer_ws_save_and_load_view` passes
at the fix commit.
