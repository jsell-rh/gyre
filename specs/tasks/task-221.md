---
title: "Repair verified failure on main 6bf777a6a44f"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["9749ae4b74e927bed0abc32a11eef1c6a1a35fb4", "dcbd107a63440b5e9a927e1875fbae111cc8360e"]
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

Round 1 (attribution repair, at base `6bf777a6a44f28052ed5af28bf6fb013fde6df48`):

- Reproduced the verified baseline failure at assignment HEAD (`6bf777a6`, tree equal to base plus only the untracked `specs/tasks/task-221.md`): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `6bf777a6  task-200  feat(task-200): Message bus - per-kind payload schema validation (reject invalid payloads with 400)` (evidence: `/tmp/stage/review-evidence/attribution-before.txt`, HEAD recorded in `head-before.txt`). The ship commit `6bf777a6` is task-200's product-surface commit on main (touches `crates/gyre-common/src/message.rs` +600, `crates/gyre-server/src/api/messages.rs` +116, `crates/gyre-server/src/mcp.rs` +107, `docs/api-reference.md`) but task-200's `commits:` frontmatter recorded only its 13 pre-ship lineage SHAs - a commit cannot contain its own hash (chicken-and-egg), so the shipped surface was invisible to review scoping (task-095 R3-F4 flaw class). Same drift class as tasks 211/216/218 repaired before.
- Repair (the check's documented remedy): appended the full SHA `6bf777a6a44f28052ed5af28bf6fb013fde6df48` to `specs/tasks/task-200.md`'s `commits:` frontmatter list, joining the 13 SHAs already recorded - append-only, review scope grows, never shrinks. All 14 frontmatter SHAs resolve in this history (`git cat-file -e` verified, 14/14). No exemptions added - `scripts/task-commit-attribution-exemptions.txt` untouched, still frozen at its 3-entry baseline; no check, skip, or gate weakened.
- Probe after fix: `bash scripts/check-task-commit-attribution.sh` -> `OK: every task-labeled product-surface commit is recorded in its task's commits: frontmatter (or exempted legacy drift).` exit 0 (evidence: `attribution-after.txt`).
- Independent review, full deterministic gates, and GitHub checks remain required before merge (per assignment).

Round 2 (explorer WS session registry, at base `73a31e0b6c5280b0a4df563b08fa503ad476113a`). Fix commit: `dcbd107a63440b5e9a927e1875fbae111cc8360e` (`fix(task-221): scope explorer WS session registry to AppState`, 4 files, +175/-36 Rust). This round repaired the outstanding verification finding `4b1d5646` (`cargo test --all --quiet` at base `73a31e0b`: 2/7 tests in `crates/gyre-server/tests/explorer_ws_integration.rs` fail — `explorer_ws_connect_and_list_views` gets `{"type":"error"}` instead of `views`; `explorer_ws_save_and_load_view` gets its socket closed before the `view_query` response; binary finished in 0.17s, ruling out timeout paths).

Root cause: `ACTIVE_SESSIONS` was a process-global static keyed by `tenant:agent` (identity `default:system` for all dev-token connections), limit 3 via `max_sessions_per_user()` (env `GYRE_EXPLORER_MAX_SESSIONS`, default 3). The test binary runs 7 tests in parallel — each spins its own server via `build_state()` but all authenticate as the same dev token → same identity. Connections 4–7 evicted the oldest still-open sessions mid-test, killing the two slowest tests (both do view-seeding round-trips). Eviction sends `{"type":"error","message":"Session replaced by a newer connection."}` then closes the socket (explorer_ws.rs eviction select arms) — matching both failure signatures.

Repair (real ownership fix, not a test change): moved the registry from process-global static into `AppState` as `explorer_sessions: Arc<ExplorerSessionRegistry>` — per-server ownership, matching every sibling (`presence`, `ws_connections`, `ws_connection_counter`). The integration test file is untouched: the tests are correct, the handler was wrong. Eviction semantics, per-user limit, env override, tenant-scoped keying, oldest-first eviction, and the `Notify` shutdown signal are all preserved exactly. Also clamped a zero per-user limit to 1 in `register()` — `GYRE_EXPLORER_MAX_SESSIONS=0` would have panicked at `slots.remove(0)` on an empty Vec in the old code.

Unit tests pinning the observable semantics (deterministic eviction oracle — `Notify::notify_one()` stores a permit when no waiter is registered, so `timeout(50ms, notified())` observes whether eviction fired without sleeps):

- `test_session_registry_evicts_oldest_at_limit` — below-limit: nothing evicted; at limit: oldest evicted, newest survives; unregister makes room; limit 0 clamps to 1.
- `test_session_registry_users_are_independent` — cross-user and cross-tenant registrations never evict another user's session.
- `test_session_registries_are_per_server` — THE regression test: two independent registries (two AppStates), same user key; server B registering past its limit must never evict server A's sessions. This is exactly the failure that killed the parallel integration-test servers.

Round 2 local verification (this sandbox): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib explorer_ws::tests::test_session` → 3 passed, 0 failed (evidence: `/tmp/stage/review-evidence/cargo-test-unit.log`); `python3 scripts/check-rustfmt-diff.py 73a31e0b` → clean (4 Rust files); `SKIP_WEB_BUILD=1 python3 scripts/check-clippy-diff.py 73a31e0b` → clean (1145 existing warnings outside changes); `bash scripts/check-in-memory-state-stores.sh` → OK; `bash scripts/check-arch.sh` → passed; all three gyre-server `AppState` literal sites updated (`lib.rs` build_state, `mem.rs` test_state_inner, `middleware.rs` test literal).

Round 3 (this round, contract restore at assignment head `f7718d11`):

- Restored the assigned task contract verbatim: frontmatter title/spec_ref/depends_on, Required behavior, Base, environment fingerprint, and Baseline failure block are byte-identical to the assigned body; all post-contract material (this Shipped section, round records) sits under the single strippable `## Shipped` heading. Round 2's record was previously under `## Shipped (round 2 — explorer WS session registry)`, which does not match the contract hash's exact-heading strip list, so it leaked into the requirements hash and tripped the `contract` finding (`316b8da3`). No requirement text changed — the contract prose is exactly the assigned prose.
- Recorded the round-2 fix commit `dcbd107a63440b5e9a927e1875fbae111cc8360e` in this task's `commits:` frontmatter (restoring what `8d841d8b` had recorded): it is a task-221-labeled product-surface commit in this branch's history, and the attribution gate requires every such commit to be visible to review scoping. Verified `dcbd107a` and its tree merge cleanly: the merge `f7718d11` preserves both the AppState registry (no `ACTIVE_SESSIONS` static remains) and task-068's search fallback change.
- Cleared the same drift class on the assignment's main-side base: `a11ba8d32859a9018ca74f9745d6b00d4ebe1aa0` (`feat(task-068): Graph Summary & Dry-Run MCP Tools`, product surface: `crates/gyre-domain/src/view_query_resolver.rs`, `crates/gyre-server/src/explorer_ws.rs`, `crates/gyre-server/src/mcp.rs`, `crates/gyre-server/tests/graph_integration.rs`) was missing from `specs/tasks/task-068.md`'s `commits:` frontmatter — the attribution gate fails on it at the pure assignment base (`bash scripts/check-task-commit-attribution.sh` at `a11ba8d3` exits 1 with exactly that violation, evidence `attribution-before-r3-base.txt`). Appended it (the check's documented remedy), append-only, joining the 9 SHAs already recorded. No exemptions added.
- Baseline reproduction re-confirmed at the exact base this round: detached worktree at `6bf777a6a44f28052ed5af28bf6fb013fde6df48`, gate exits 1 with the identical violation (evidence: `/tmp/stage/review-evidence/attribution-before-r3.txt`).
- Mutation check (test-the-repair): removing `a11ba8d3` from task-068's frontmatter re-fails the gate with that violation (exit 1); removing `dcbd107a` from task-221's frontmatter re-fails with that violation (exit 1); restoring both re-passes (exit 0) — the pass is attributable to the recorded SHAs, not gate drift (evidence: `mutation-check-r3.txt`).
- Contract equality re-verified after the full rewrite: `requirement_parts(assigned body)` vs `requirement_parts(this file)` — frontmatter parts and prose parts both byte-identical (`front equal: True, prose equal: True`).
- Round 3 local verification on the merged tree `f7718d11` (the merge combined two independent `explorer_ws.rs` changes, so the probes were re-run, not inherited): `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib explorer_ws::tests::test_session` → **3 passed, 0 failed** (`test_session_registry_evicts_oldest_at_limit`, `test_session_registry_users_are_independent`, `test_session_registries_are_per_server`; evidence: `cargo-test-unit-r3.log`); `python3 scripts/check-rustfmt-diff.py a11ba8d3` → clean (4 Rust files); `bash scripts/check-arch.sh` → passed; `bash scripts/check-in-memory-state-stores.sh` → OK; `bash scripts/check-task-commit-attribution.sh` → exit 0 (evidence: `head-after.txt`).
- Clippy (scoped substitute; see evidence `clippy-diff-r3.log`): `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server -p gyre-adapters --all-targets --all-features -- -W clippy::all` with the gate's exact changed-lines logic → 0 errors, 0 changed-line failures, 720 pre-existing warnings outside the changed lines. The workspace-wide gate could not run here: gyre-cli's dependency `lru` (via ratatui) is absent from the offline registry and static.crates.io is unreachable from this sandbox (connection refused). No Rust file changed vs base outside gyre-server, so the changed-line set checked is identical to the gate's; the unscoped workspace clippy remains a host/CI item.
- `python3 /tmp/stage/dev-attribution.py task-221` re-run after the rewrite: no-op, `commits:` stays exactly `["dcbd107a63440b5e9a927e1875fbae111cc8360e"]` (verified via `git diff`).
- No gate weakened: `scripts/` untouched this round; `scripts/task-commit-attribution-exemptions.txt` unchanged at its frozen 3-entry baseline.
- Transport restriction: this sandbox's TCP `accept()` is errno-95-blocked (recorded in `/tmp/stage/capabilities.json`), so `cargo test -p gyre-server --test explorer_ws_integration` (the explorer WS reproduction), full `cargo test --all`, and the unscoped workspace clippy are deferred to host verification and required GitHub CI, as in round 2.

Round 4 (review-finding repair on merged tree `0eb1caf3`, fix commit `9749ae4b74e927bed0abc32a11eef1c6a1a35fb4`):

- Repaired review finding `646d6f6b` (category code, file `scripts/byte-slice-truncation-exemptions.txt`): candidate broke `check-byte-slice-truncation.sh` — `dcbd107a` drifted the pre-existing `&raw_preview[..500]` site from explorer_ws.rs:2882 to 2912 while the exemption stayed anchored at 2882. Reproduced on the merged tree: gate exit 1, `ERROR: constant byte-index slice at crates/gyre-server/src/explorer_ws.rs:2912` (evidence: `byte-slice-mutation-r4.txt`, which also serves as the mutation check below).
- Chose the reviewer's preferred remedy and the exemption file's own policy (list should SHRINK; entries are real F4-class panic hazards): real char-boundary fix + entry deletion, not line re-anchoring. The invalid-view-query warning path now truncates via `gate_executor::truncate_bytes` (made `pub(crate)`), the same unit-tested helper the gate executor already uses for multibyte process output — one convention, not a second. No new exemption, no inline `// slice:ok`, no gate edit; the exemption list went 3 -> 2 entries and both surviving entries re-verified at their exact lines (specs.rs:282, pre_accept.rs:81).
- Mutation check (test-the-repair): `git stash` of the fix re-fails the gate with the identical violation (exit 1); restore re-passes (exit 0). The pass is attributable to the fix, not gate drift (evidence: `byte-slice-mutation-r4.txt`, `byte-slice-after-restore-r4.txt`, `byte-slice-after-r4.txt`).
- Focused verification: `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` clean; `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib explorer_ws::tests::test_session` -> 3 passed (re-run after import reorder); `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib truncate_bytes` -> 3 passed (the helper's char-boundary semantics: ASCII untouched, multibyte boundary backs off, leading multibyte backs to zero — these are the behavior tests for the truncation now applied to the preview path); `SKIP_WEB_BUILD=1 cargo clippy -p gyre-server --all-targets --all-features` -> 0 errors, 0 unused warnings; rustfmt clean on the changed file; `bash scripts/check-arch.sh` -> passed; `bash scripts/check-in-memory-state-stores.sh` -> OK.
- `9749ae4b` is a task-221-labeled product-surface commit (touches `crates/gyre-server/src/explorer_ws.rs`, `crates/gyre-server/src/gate_executor.rs`), so it is recorded in this task's `commits:` frontmatter, append-only, joining `dcbd107a`.
- Cleared the same drift class on this round's base: `27bd585ca7eb429905ccbded1f48b4d0167c0c20` (`feat(task-155): Implement gyre search CLI command`, product surface: `crates/gyre-cli/src/client.rs`, `crates/gyre-cli/src/main.rs`, `docs/cli.md`) was missing from `specs/tasks/task-155.md`'s `commits:` frontmatter — the gate failed on it at the pre-fix HEAD `0eb1caf3` (evidence: `attribution-r4-base.txt`), i.e. it arrived with the assignment's base merge, not this round's work. Appended it (the check's documented remedy), append-only, joining the 7 SHAs already recorded. No exemptions added; the exemption file stays frozen at its 3-entry baseline. Gate re-run: exit 0 (`attribution-r4-after.txt`).
- Full workspace suite, workspace-wide clippy, and GitHub checks remain host/CI items (TCP `accept()` errno-95 restriction unchanged, `/tmp/stage/capabilities.json`).

## Review

Round 4 repaired review finding `646d6f6b` (byte-slice gate failure on the candidate). Round 4 verification on tree `9749ae4b` is required: byte-slice gate, attribution gate, static gates, and the host-side suite runs listed above (the transport restriction still defers the explorer WS integration test, full `cargo test --all`, and workspace-wide clippy to host verification and required GitHub CI). Round 2's fix (`dcbd107a`) was reviewed within this task's round-2 record; its regression tests are the three `explorer_ws.rs::tests::test_session*` tests. The round-4 truncation fix's behavior tests are the three `gate_executor::tests::truncate_bytes_*` tests plus the byte-slice gate itself.
