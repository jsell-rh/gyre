# Review — task-233 (Repair verified failure on main 27bd585ca7eb)

Candidate: `b91e5cbbdb4f03850d115615e5c338ac8b4d5fb5`
Base: `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77` (= origin/main)
Verdict: **approved**

## Scope inspected

`git diff 63aa1486 b91e5cbb` is exactly one file: `specs/tasks/task-233.md`
(the task record, 116 lines). The product-surface delta vs main
(`crates/ web/src web/tests scripts/`) is empty (verified: 0-line diff).
The candidate is main plus the task record, exactly as the "Shipped" section
claims.

## Independent checks (evidence in the stage's review-evidence dir)

1. **Baseline failure repaired.** `bash scripts/check-task-commit-attribution.sh`
   on candidate HEAD: `OK: every task-labeled product-surface commit is recorded
   ...`, exit 0. The substantive repair — appending `27bd585c...` to
   `specs/tasks/task-155.md` `commits:` — is commit `7c6ac232` (task-231),
   an ancestor of the candidate; that commit touched only `specs/tasks/`
   (no exemption file). The exemption file `scripts/task-c....txt` does not
   exist in the tree, so nothing was grandfathered.

2. **Explorer WS fix is real production code.** Commit `483bf8ad` (task-239)
   is an ancestor. The process-global `ACTIVE_SESSIONS`/`SESSION_ID_COUNTER`
   statics are gone from `crates/gyre-server/src/explorer_ws.rs`; the limit
   lives in `ExplorerSessionRegistry` owned by `AppState.explorer_sessions`,
   wired in `build_state` (lib.rs:900), the test constructor (mem.rs:3281),
   and the middleware override clones the base instance (middleware.rs:233).
   Single-instance behavior (cap + oldest-eviction) is preserved.

3. **Focused probe.** `cargo test -p gyre-server --test explorer_ws_integration`:
   8 passed / 0 failed, exit 0, on two consecutive complete runs (~3–4s),
   including the previously failing `explorer_ws_delete_view` and the
   regression test `explorer_ws_session_limit_is_per_server_instance`.

4. **Mutation check (test is a genuine bug-killer).** Temporarily backed the
   registry's session map with a process-global `LazyLock` (simulating the
   pre-task-239 bug) and re-ran the regression test: it FAILED deterministically
   (exit 101, panic at the "Session on server A must survive a same-user
   connection on server B" assertion). Mutation reverted; tree verified clean;
   full binary re-verified 8/8 after restore. The test also asserts the cap
   still evicts within one server, so the fix is not "limit disabled".

5. **Merge-resolution claim.** `git diff 63aa1486 b91e5cbb -- web/` is empty —
   the candidate ships main's `web/dist` verbatim; committed `index.html`
   references the committed bundle (`index-D4CX8rVo.js`), which exists;
   no orphaned bundles remain.

6. **Attribution canonicality.** `dev-attribution.py task-233` derives an
   empty commit list (branch commits vs origin/main touch only `specs/tasks/`
   and `web/dist/`), so `commits: []` in the frontmatter is correct.

7. **No weakened checks.** The candidate diff contains no scripts/, exemption,
   skip, or test-filtering changes.

## Notes

- A `cargo test` run in this sandbox rebuilds `web/dist` via build.rs with
  environment-dependent content hashes; that working-tree side effect was
  restored — it is not a candidate defect (the committed dist is byte-identical
  to main's, which is the claimed resolution).
- Full workspace suites, all-target Clippy, and GitHub checks are owned by the
  verification stage, per the task record.
