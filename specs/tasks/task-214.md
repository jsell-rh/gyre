---
title: "Repair verified failure on main f4acb4ebcaf9"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: ready-for-review
commits: ["52a7d4afb5c5ecccaf548409c9478817c410fd02", "8e5852493dd00fae67c93cc1f74679eea9b7c43a", "96d2a28c0e5068d00924db884877712e1cbd4dee"]
---

## Required behavior

Reproduce and repair this verified upstream failure. Implement real production fixes or correct a genuinely broken test setup. Do not weaken checks, add skips or exemptions, or implement the blocked feature. Obtain independent review and pass full verification and GitHub checks.

Base: `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f`
Environment fingerprint: `host-ccef3b35e63712dd367c12c4ca37cae27eced4e1f8b1172298247a7af662ea42`

## Baseline failure

```text

$ python3 /home/jsell/code/gyre/scripts/dev-cargo-clean.py
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-server#0.1.0` is ignored, cleaning all versions of `gyre-server` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-server#0.1.0` ignored, cleaning all versions of `gyre-server` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-adapters#0.1.0` is ignored, cleaning all versions of `gyre-adapters` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-adapters#0.1.0` ignored, cleaning all versions of `gyre-adapters` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-common#0.1.0` is ignored, cleaning all versions of `gyre-common` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-common#0.1.0` ignored, cleaning all versions of `gyre-common` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-domain#0.1.0` is ignored, cleaning all versions of `gyre-domain` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-domain#0.1.0` ignored, cleaning all versions of `gyre-domain` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-ports#0.1.0` is ignored, cleaning all versions of `gyre-ports` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-ports#0.1.0` ignored, cleaning all versions of `gyre-ports` found
warning: version qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-cli#0.1.0` is ignored, cleaning all versions of `gyre-cli` found
warning: url qualifier in `-p path+file:///home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-cli#0.1.0` ignored, cleaning all versions of `gyre-cli` found
     Removed 257 files, 1.5GiB total

$ bash /home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/tools/checks.sh
rustfmt: crates/gyre-server/src/api/specs_assist.rs: changed lines need formatting: 695, 696, 697, 698, 699, 700, 701, 702, 1109
crates/gyre-server/src/api/specs_assist.rs:1049: clippy::needless_borrows_for_generic_args: the borrowed expression implements the required traits
    Checking gyre-common v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-common)
   Compiling gyre-server v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-server)
warning: gyre-server@0.1.0: SKIP_WEB_BUILD=1 set, skipping web build
    Checking gyre-domain v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-domain)
    Checking gyre-cli v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-cli)
    Checking gyre-ports v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-ports)
    Checking gyre-adapters v0.1.0 (/home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/crates/gyre-adapters)
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

  f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain

A task-labeled commit absent from the task's commits: list is invisible
to review scoping — the verifier scopes each round to that list
(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by
adding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.
Do NOT add entries to /home/jsell/code/gyre/.gyre-pipeline/attempts/11d68489bcea44b383f7ded8a0e374b3/1/checkout/scripts/task-commit-attribution-exemptions.txt.
GYRE_BASELINE_FAILURE_JSON {"base": "f4acb4ebcaf930ada2f1318b8aa2adbf244e720f", "environment": "61f092e025ec4bba7cdcf4797b454fd3b7bf86825ab1ddd7d2084bd9a163061e", "probe": ["bash", "scripts/check-task-commit-attribution.sh"], "log": "FAIL: task-labeled product-surface commits missing from their task's commits: frontmatter:\n\n  f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain\n\nA task-labeled commit absent from the task's commits: list is invisible\nto review scoping \u2014 the verifier scopes each round to that list\n(task-095 R3-F4: 5aaded21, +880 lines, was never examined). Fix by\nadding the short SHA to specs/tasks/task-NNN.md's commits: frontmatter.\nDo NOT add entries to /tmp/gyre-gate-baseline-qe03bc1k/scripts/task-commit-attribution-exemptions.txt.\n"}

```

## Shipped

Two repairs shipped on this branch (commits `96d2a28c` + the task-record commits; assignment base `f4acb4eb` → current base merge `653a696f`):

### 1. Baseline failure — task-189 attribution drift (the verified upstream failure)

- Reproduced at assignment HEAD (`f4acb4eb`, tree equal to base plus only `specs/tasks/task-214.md`): `bash scripts/check-task-commit-attribution.sh` exited 1 listing `f4acb4eb  task-189  feat(task-189): Fix persona scope resolution to walk the real parent chain` — the task-189 ship commit (GitHub squash-merge onto main, touches `crates/gyre-server/src/api/personas.rs` + `specs/`) absent from task-189's `commits:` frontmatter, i.e. invisible to review scoping (task-095 R3-F4 flaw class). Evidence: `/tmp/stage/review-evidence/attribution-before.txt` (exit 1, prior round), `head-commit.txt`.
- Repair (the check's documented remedy): appended the full SHA `f4acb4ebcaf930ada2f1318b8aa2adbf244e720f` to `specs/tasks/task-189.md`'s `commits:` frontmatter list, preserving the five existing pipeline-branch SHAs (same append-not-replace form as the reviewed task-211 repair of the identical failure class). No exemptions added — `scripts/task-commit-attribution-exemptions.txt` untouched, still frozen at its 3-entry baseline; no check, skip, or gate weakened.
- Probe at current HEAD: `bash scripts/check-task-commit-attribution.sh` → `OK: ... (or exempted legacy drift).` exit 0. Evidence: `/tmp/stage/review-evidence/attribution-after.txt`, `gates-at-head.txt`.
- The other baseline-log failures (rustfmt/clippy on `specs_assist.rs`) were artifacts of the superseded attempt checkout (`11d68489...`), not base state: `f4acb4eb`'s diff touches no `.rs` file other than `personas.rs`, and `check-rustfmt-diff.py` at this HEAD reports the changed lines clean.

### 2. Durable verification finding — explorer WS per-user session cap (cargo test --all exit 101)

- **Failure (attempt `6db444f0`, `cargo test --all` exit 101):** `explorer_ws_message_too_long` panicked at `explorer_ws_integration.rs:340` — `contains("too long")` failed after `type=="error"` passed: the socket received the *eviction* error ("Session replaced by a newer connection.") instead of the expected "Message too long". Race-dependent: 6/7 tests passed in the recorded run (task-217's branch observed the sibling manifestation `explorer_ws_delete_view` at :294).
- **Root cause:** the per-user concurrent-session cap was enforced through a **process-global** `ACTIVE_SESSIONS` static while every `AppState` instance is an independent server (own storage, own port). The integration binary spawns one server per test, all authenticating as the same dev user (`default:system`), so the 4th concurrent registration evicted the oldest **live** session on a *different* server.
- **Fix (`96d2a28c`, byte-identical across all 4 changed files to reviewed task-217 branch commit `c3cba0a6`, which had not landed on main):** moved the tracker into `AppState` as `ExplorerSessionRegistry` — a named struct (not a bare alias; `check-in-memory-state-stores.sh` OK because live WS sessions are ephemeral process-local state: a TCP connection cannot outlive its process, so it is deliberately not port-backed). `register()` returns `(id, shutdown Notify)`; a `SessionGuard` releases the slot on drop (panic-safe); oldest-first eviction signals exactly the evicted session; a 0 cap is treated as 1 so a misconfigured limit admits the registering session instead of spinning the eviction loop. Production semantics unchanged (one AppState per process); test servers are isolated per AppState.
- **Unit evidence:** `cargo test -p gyre-server --lib explorer_ws::` → **39/39 pass**, including the 4 registry tests: eviction beyond cap signals exactly the oldest session (never a live one), release frees a slot and is idempotent, two independent registries never evict each other's sessions (the regression, pinned), zero-cap admits the registering session. Evidence: `/tmp/stage/review-evidence/cargo-test-explorer-ws.txt` (39 passed; 0 failed; 1161 filtered).
- **Gates at HEAD `ca93dfde`:** `check-task-commit-attribution.sh` OK, `check-rustfmt-diff.py 653a696f` clean (4 files), `check-in-memory-state-stores.sh` OK. Evidence: `gates-at-head.txt`.
- **Transport restriction (recorded, not a code defect):** this sandbox cannot run the WS integration binary — `accept(2)` returns `ENOTSUP` (errno 95; `/tmp/stage/capabilities.json` `tcp_listener_probe`, and the integration harness binds a real `TcpListener` in `WsCtx::new`). Reproduction here is the durable CI log plus code-path analysis. **Host verification must run:** `cargo test -p gyre-server --test explorer_ws_integration` (expects 7/7) and `cargo test --all`.

### Attribution for this task

- Product-surface commit `96d2a28c` recorded in `commits:` frontmatter (full SHA, corrected by checkpoint commit `2bbc4f8e` after a truncated SHA was recorded in the interrupted run). Docs-only commits (`1f7876ab`, `2bbc4f8e`) touch `specs/` only, outside the check's scope.
- Independent review, full deterministic gates, and GitHub checks remain required before merge (per assignment); no full-suite rerun performed here per the smallest-relevant-probe constraint.
