# Review — task-234 (Repair verified failure on main 27bd585ca7eb)

Candidate: `21c2650e69f0780c0e41fa3b15ff24adc19e2d60` on
`pipeline/task-234/de19197e2f714f74bdea74bdce9eebbf-3`.
Assigned base: `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77`.
Verdict: **needs-revision** (not approved).

## What the candidate actually is (diff vs assigned base)

`git diff 63aa1486 21c2650e` touches exactly:

- `crates/gyre-server/src/explorer_ws.rs` — +110 lines, all inside the `tests`
  module (three `ExplorerSessionRegistry` unit tests + two assert helpers);
  **no production line changed**.
- `specs/tasks/task-234.md` — new task file (bookkeeping).
- `web/dist/` — bundle churn (`elk.bundled-OwdVv5y8.js`→`CZ1SLFkJ.js`,
  `index-D4CX8rVo.js`→`index-DauSjDxg.js`, `index.html` reference).

The task's nominal subject — the `check-task-commit-attribution.sh` failure on
`27bd585c`/task-155 — is **already repaired at the assigned base**:
`specs/tasks/task-155.md` at `63aa1486` already lists
`27bd585ca7eb…c20` in its `commits:` frontmatter (sibling repair tasks landed
the same one-line fix on main before this base). The candidate's copy of that
fix (commit `2c4a1708`, made against the original base `27bd585c`) is redundant
relative to the assigned base: the candidate contributes no change to
`specs/tasks/task-155.md` at all.

## Probes (evidence in /tmp/stage/review-evidence)

- `bash scripts/check-task-commit-attribution.sh` → **exit 0** at the candidate
  (`attribution-candidate.txt`) and **exit 0 at the assigned base**
  (`attribution-base.txt`, run in a detached worktree) — the assigned baseline
  failure does not reproduce at the base, with or without this candidate.
- Mutation: removing the SHA from `task-155.md` frontmatter → **exit 1** with
  the original violation (`attribution-mutation.txt`) — the gate is live; the
  pass is attributable to the recorded SHA.
- New unit tests: `cargo test -p gyre-server --lib registry_` → **8 passed**
  (`registry-unit-tests.txt`).
- Test-quality mutation: reverting `ExplorerSessionRegistry` to a
  process-global backing (the exact pre-task-239 defect shape) →
  `registry_sessions_are_isolated_per_instance` **FAILS**
  (`registry-mutation.txt`). The tests are meaningful, not self-confirming.
- Durable-finding probe: `cargo test -p gyre-server --test explorer_ws_integration`
  → **FAILED intermittently**: `explorer_ws_rate_limiting` panicked at
  `tests/explorer_ws_integration.rs:390` ("Second rapid message should be rate
  limited") in 2 of 5 full-binary runs (`explorer-ws-integration.txt` and
  follow-up runs logged in this review); passes 3/3 in isolation. The second
  durable failure, `explorer_ws_save_and_load_view`, now passes consistently —
  that one was fixed on main by task-239's `483bf8ad` ("scope explorer WS
  session limit to the server instance"), an ancestor of the assigned base.
- Web bundle integrity: `npm ci` + `npm run build` from the candidate's
  `web/src` reproduces the committed `web/dist` **byte-for-byte** (git status
  clean after rebuild) — the dist churn is benign build-env nondeterminism
  (minifier variable-renaming vs base's bundle), not a source/bundle mismatch.
- No checks weakened: `scripts/`, `.github/workflows/`, and all exemption
  files are identical to base; `ci.yml` at candidate == base.
- Sandbox listeners: `/tmp/stage/capabilities.json` reports
  `tcp_listener_probe: supported` — the WebSocket integration tests bind and
  run fine in this environment.

## Findings

- **F1 (major, verification): the durable verification failure is not
  repaired.** Durable finding `b5ed01bf` records `cargo test --all` → exit 101
  with `explorer_ws_rate_limiting` and `explorer_ws_save_and_load_view`
  failing. At the candidate, `explorer_ws_rate_limiting` still fails
  intermittently under the full integration binary (2/5 runs here; timing
  flake under parallel load — the server reads the second message more than
  `min_message_interval_ms` after the first, or the error arrives outside the
  test's read window). The candidate changed neither the production
  rate-limit path (`explorer_ws.rs` message loop, unchanged vs base) nor the
  test. The added registry unit tests do not touch this path. The task
  contract requires "pass full verification"; a verifier re-running
  `cargo test --all` can hit the identical failure again. Remedy: fix the
  flake for real — either the test setup (deterministic ordering/timeout
  budget for the second message and its error frame) or the production
  rate-limit semantics — not by adding unrelated green tests.
- **F2 (process/narrative): the `task-234.md` "Shipped" section
  misrepresents the candidate.** It claims "no Rust/JS source changed", but
  the diff vs the assigned base changes `crates/gyre-server/src/explorer_ws.rs`
  (+110 lines). It presents the task-155 frontmatter append as this round's
  repair, but that line is already present at the assigned base — the
  candidate contains no change to it. It claims "this sandbox cannot run TCP
  accept() (errno 95)" to justify skipping the integration tests, but this
  environment's measured capabilities report listener support and the
  integration tests run here — and they are exactly where the outstanding
  failure lives. A reviewer scoping to this narrative would miss both the
  actual diff and the actual open defect.
- **F3 (scope): the only substantive change is out-of-scope regression
  testing.** The +110 lines test `ExplorerSessionRegistry` per-instance
  isolation — production behavior already fixed on main by task-239
  (`483bf8ad`, ancestor of the assigned base; at `27bd585c` the registry was
  the `static ACTIVE_SESSIONS: LazyLock<…>` process global). The task rules
  explicitly exclude "New tests for already-working behavior — not needed, not
  wanted." The tests are individually sound (mutation-verified above) but
  close no gap and do not address F1.

## Not findings (checked, clean)

- `check-task-commit-attribution.sh` passes and is mutation-sensitive; no
  exemption-file growth; no gate/skip/check weakened anywhere in the diff.
- Committed `web/dist` exactly matches a fresh locked build of the candidate's
  `web/src`; the bundle-hash churn vs base is functionally equivalent
  minifier renaming, not stale or mismatched UI code.
- Task frontmatter attribution for task-234's own commits is consistent (the
  attribution check passes with the branch's task-labeled commits reachable).

## Required before approval

1. Repair the `explorer_ws_rate_limiting` intermittent failure (test setup or
   production semantics — real fix, no skip/ignore/timeout inflation), with a
   probe showing the full `explorer_ws_integration` binary passing repeatedly
   under load.
2. Correct the `task-234.md` Shipped narrative to describe the candidate
   relative to its assigned base (including the registry tests and the dist
   rebuild), and drop the unsupported no-TCP claim.
3. Drop or justify the out-of-scope registry unit tests against the task
   rules (they are redundant with task-239's coverage on main).
