---
title: "Repair verified failure on main 27bd585ca7eb"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: []
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20`
Environment fingerprint: `host-88e5982a8b65dfab050a3fd4ab0c79a6c71c3e6a87bc3e5a5b877d2e2419b262`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/tools/checks.sh
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

  27bd585c  task-155  feat(task-155): Implement gyre search CLI command

A task-labeled commit absent from its task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/5e6b3bb0db3c40b9b920d43ae1605c15/1/checkout/scripts/task-c....txt.
GYRE_BASELINE_FAILURE_JSON {"base": "27bd585ca7eb429905ccbded1f48b4d0167c0c20", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  27bd585c  task-155  feat(task-155): Implement gyre search CLI command\n\nA task-labeled commit absent from its task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-xplcd8ns/scripts/task-c....txt.\n"}

```

## Shipped

This task repaired the verified failure on main `27bd585c` and, across its
rounds, absorbed two failure classes. Final state: branch = main `63aa1486`
plus this task record; `git diff origin/main HEAD -- crates/ web/src web/
tests scripts/` is empty.

### Round 1 — attribution drift (baseline probe)

`scripts/check-task-commit-attribution.sh` failed because the task-155
squash-landing commit `27bd585c` (product surface: `crates/gyre-cli/src/
client.rs`, `main.rs`, `docs/cli.md`) was missing from `specs/tasks/
task-155.md`'s `commits:` frontmatter. The repair — appending the full SHA
as the 8th entry of that list — landed on main via task-231 (`7c6ac232`).
On this branch's merged HEAD the probe exits 0. No exemptions were added;
the exemption file stays at its frozen baseline.

### Round 2 — `explorer_ws_delete_view` failure (verified `cargo test --all` finding)

Root cause: the explorer WebSocket per-user concurrent session limit
(default 3, oldest-eviction) was tracked in a process-global static keyed
`tenant_id:agent_id`. All integration tests authenticate as the same
principal (`default:system`, global token), so concurrent same-process test
connections evicted each other mid-test; the victim connection closed
before receiving its post-delete `views` frame. The production fix scopes
the registry to the server instance: `AppState.explorer_sessions`
(`explorer_ws::ExplorerSessionRegistry`, wired in `build_state`,
`test_state`, and the middleware override), exactly like sibling
`presence`/`ws_connections` state. It landed on main as task-239
(`483bf8ad`, "scope explorer WS session limit to the server instance")
together with regression test `explorer_ws_session_limit_is_per_server_instance`.

### This round — merge resolution + verification

Resolved the pending merge of main `63aa1486` into this branch: conflicts
were confined to generated `web/dist/` bundles; our side had zero `web/src`
changes since merge-base `34248324` while main changed `web/src` (+752
lines) and rebuilt dist, so the resolution takes main's `web/dist`
verbatim (orphaned `index-DauSjDxg.js` removed).

Evidence (`/tmp/stage/review-evidence/`):
- `task-233-final-state.txt` — HEAD, empty product-surface delta vs main,
  attribution probe exit 0.
- `task-233-explorer-ws-repeat.txt` — focused probe
  `cargo test -p gyre-server --test explorer_ws_integration`:
  8 passed / 0 failed on 4 consecutive runs (~4s each), including the
  previously failing `explorer_ws_delete_view` and the per-server-instance
  regression test.

Attribution: `python3 /tmp/stage/dev-attribution.py task-233` derives an
empty list — branch commits touch only `specs/tasks/` and `web/dist/`, no
product surface — so `commits: []` is attribution-canonical.

Full workspace suites, architecture checks, all-target Clippy, and GitHub
checks remain owned by the verification and publication stages.
