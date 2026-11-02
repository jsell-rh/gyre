---
title: "Repair verified failure on main b1cdb0ddd80e"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["3e62cdbb5aa0881246ceb3170461e02eda7bea6b", "483bf8ad72a38505d47f5a8cb47bae619a4648da"]
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

## Shipped

### Verification failure repaired (round 4)

The verified upstream failure (checks.sh exit 1 at candidate
3a878cf3) reproduces as `scripts/check-byte-slice-truncation.sh`
exit 1: the F4-class `&raw_preview[..500]` slice in
`crates/gyre-server/src/explorer_ws.rs` (invalid view-query preview
truncation) was exempted at line 2882, but this branch's product
commit 483bf8ad added lines above the site and shifted it to line
2921, so the line-pinned exemption no longer matched. Re-pinning the
line number would create a new exemption entry, which the
exemption-growth gate forbids; per the exemption file's own policy
("when you fix one, DELETE its exemption line") the hazard was fixed
for real instead, in commit 3e62cdbb:

- `explorer_ws.rs` truncates the preview through
  `crate::gate_executor::truncate_bytes` - UTF-8 char-boundary safe.
  The site previously panicked whenever byte 500 of the
  pretty-printed query JSON fell inside a multibyte character.
- `truncate_bytes` is now `pub(crate)`: the same helper, already
  unit-tested for exactly that panic case, is reused rather than
  duplicated.
- The `explorer_ws.rs:2882` exemption entry is deleted. The list
  shrinks; no new entries; no check, skip, or gate weakened.

Evidence (`/tmp/stage/review-evidence/`): gate exit 1 at assignment
HEAD before the fix (flags `explorer_ws.rs:2921`, identical violation
to the verified failure); exit 0 after; mutation check re-fails with
the fix reverted and re-passes restored; explorer WS integration
suite 8/8 (including `explorer_ws_save_and_load_view` and
`explorer_ws_session_limit_is_per_server_instance`); `truncate_bytes`
unit tests 3/3; rustfmt and clippy clean on the changed lines (the
sandbox clippy 1.99 `never_loop` error in `gyre-domain` is a plain
invocation artifact - under the gate's exact `-W clippy::all`
invocation it is a pre-existing warning on lines unchanged vs base,
which the changed-lines gate ignores).

### Prior rounds (carried in the branch)

- Baseline attribution failure repaired: full SHA
  `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` appended to
  `specs/tasks/task-170.md`'s `commits:` frontmatter (squash-drift
  class; the check's own documented remedy). Mutation-checked in
  round 1.
- Product fix 483bf8ad (durable finding 68f83ea6): the explorer
  WebSocket per-user concurrent session limit moved from a
  process-global static to an `ExplorerSessionRegistry` owned by
  `AppState`, stopping parallel test servers (and embedders) from
  evicting each other's sessions. Recorded in this task's `commits:`
  frontmatter, as is 3e62cdbb.
- Round 3 restored this task file's contract (frontmatter, Required
  behavior, Baseline failure) to the round-1 pipeline checkpoint
  after round 2 appended normative sections; those contract sections
  remain byte-identical.

Independent review, full deterministic gates, and GitHub checks on
the exact PR head remain required before merge.
