---
title: "Repair verified failure on main b1cdb0ddd80e"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: ["7a820241adafdaee2218339f38eb4a29d4f6851f", "3e62cdbb5aa0881246ceb3170461e02eda7bea6b", "483bf8ad72a38505d47f5a8cb47bae619a4648da"]
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

### Verification failure repaired (round 5)

The verified upstream failure (checks.sh exit 1 on merge base
121fba26, candidate c0cef9de, category `verification`) reproduces
as the clippy changed-lines gate failing on
`crates/gyre-server/tests/explorer_ws_integration.rs:448:
clippy::useless_conversion: useless conversion to the same type:
std::string::String`. The trailing vite output in the failure log
("Removing web/dist/assets/...") is dev-check.sh's normal
`git clean` cleanup after a successful web build - the failing step
is clippy, not the build. The violation was in the new test added by
product commit 483bf8ad:
`Message::Text(json!({"type": "list_views"}).to_string().into())` -
`json!(...).to_string()` already yields `String`, and
tokio-tungstenite 0.24's `Message::Text` takes `String`, so `.into()`
was a no-op conversion on a changed line. Fixed in commit 7a820241 by
dropping the useless `.into()`; no check, skip, exemption, or gate was
weakened, and no assertion changed.

Evidence (`/tmp/stage/review-evidence/`): clippy gate reproduction
with the exact dev-check.sh invocation (`cargo clippy --all-targets
--all-features --message-format=json -- -W clippy::all`, changed lines
derived from `git diff 121fba26..HEAD`) fails with exactly the
upstream finding before the fix and reports zero changed-line
failures after (1141 pre-existing warnings outside changes, same
class as the 343 the baseline reported); rustfmt changed-lines gate
clean (6 files); explorer WS integration suite passes in full;
mutation check re-introduces the failure with `.into()` restored and
it disappears again with the fix re-applied.

### Prior rounds (carried in the branch)

- Baseline attribution failure repaired: full SHA
  `b1cdb0ddd80ecd37f9de7c29559562f8a86dccf3` appended to
  `specs/tasks/task-170.md`'s `commits:` frontmatter (squash-drift
  class; the check's own documented remedy).
- Product fix 483bf8ad (durable finding 68f83ea6): the explorer
  WebSocket per-user concurrent session limit moved from a
  process-global static to an `ExplorerSessionRegistry` owned by
  `AppState`, stopping parallel test servers (and embedders) from
  evicting each other's sessions.
- 3e62cdbb: the F4-class `&raw_preview[..500]` byte-slice in
  `explorer_ws.rs` now truncates through the char-boundary-safe
  `gate_executor::truncate_bytes`, and its line-pinned exemption
  entry was deleted rather than re-pinned.

All three commits are recorded in this task's `commits:` frontmatter,
as is 7a820241. Independent review, full deterministic gates, and
GitHub checks on the exact PR head remain required before merge.
