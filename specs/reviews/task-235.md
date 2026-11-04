# Review — task-235 (Repair verified failure on main 27bd585ca7eb)

Candidate: `515d9139f743fc3a758b5ee1d9358b056db1542d` (assigned base
`58deeb75d0eb6f5b08a3baf4e7f1af67360c0f09`). Verdict: **needs-revision**.

## Round 1

Probes (all at the candidate checkout; evidence under
`/tmp/stage/review-evidence/`):

- `bash scripts/check-task-commit-attribution.sh` → exit 0 (assigned gate repaired).
- Mutation: remove `27bd585c…` from `specs/tasks/task-155.md` frontmatter → gate
  re-fails with the exact baseline violation; restored → exit 0
  (`mutation-frontmatter-check.txt`).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → 1217 passed, 0 failed
  (`lib-tests.txt`) — previous round's E0062 duplicate-field compile failure is fixed.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration` →
  8 passed, 0 failed, including `explorer_ws_rate_limiting` (durable verification
  finding 11bdc76d) and `explorer_ws_session_limit_is_per_server_instance`
  (`explorer-ws-integration.txt`).
- Mutation: remove `let max = max.max(1);` from
  `ExplorerSessionRegistry::register` → `test_session_registry_evicts_oldest_at_limit`
  panics with the predicted `removal index (is 0) should be < len (is 0)`;
  restored → passes (`mutation-clamp-check.txt`). The new tests are real.
- `git diff 58deeb75 515d9139 -- scripts/` → empty; every exemption file
  byte-identical to base (attribution exemptions still at the frozen 3 entries).
  No check weakened, no skip or exemption added.
- `bash scripts/check-fail-open-ref-resolution.sh` → exit **1** at candidate,
  exit 0 at base (`fail-open-check-candidate-vs-base.txt`).
- `bash scripts/check-inert-enforcement.sh` → exit **1** at candidate, exit 0 at
  base (`inert-enforcement-check-candidate-vs-base.txt`).
- `cargo fmt --check -p gyre-server` → clean.

Verified working (no findings):

- **Assigned repair**: the task-155 squash-drift repair (full SHA
  `27bd585ca7eb…` in `specs/tasks/task-155.md` `commits:`) is present and
  byte-identical to base (main received the same repair via task-231/`7c6ac232`;
  the candidate does not regress it). The gate passes at the candidate, and the
  pass is attributable to the recorded SHA (mutation check above).
- **Previous round's blocking defects fixed**: the cfg(test) duplicate
  `explorer_sessions` field in `middleware.rs` is gone (file byte-identical to
  base; lib suite compiles and passes), and the elk-bundle merge-conflict
  markers are gone (`web/dist` byte-identical to base, no conflict markers, no
  dangling import).
- **Net product surface fully examined**: of the 11 changed Rust files vs base,
  9 are pure rustfmt reflow — verified via `git diff -w` (zero semantic hunks in
  `merge_processor.rs`, `retention.rs`, `abac_middleware.rs`, `api/audit.rs`,
  `api/mr_timeline.rs`, `api/recovery.rs`, `audit_simulator.rs`,
  `container_audit.rs`, `gate_executor.rs`). The only functional changes are in
  `explorer_ws.rs`: (a) `max.max(1)` clamp in `register()` — a real
  `Vec::remove(0)` panic fix reachable via `GYRE_EXPLORER_MAX_SESSIONS=0`
  (the un-clamped `while slots.len() >= max` loop always enters at 0); (b)
  mid-run user `Message`s are queued and replayed through the rate limiter
  instead of being silently dropped (extends the existing unbounded
  post-run queue already used for view CRUD at base — same risk class, not a
  new one); (c) registry doc updated to justify the intentionally
  non-port-backed ephemeral state. Plus a `lib.rs` test exercising the
  production `build_state` wiring for per-server session isolation. All tests
  for these pass and the clamp test kills its bug (mutation above).

Findings:

- [-] **F1 (major): the candidate breaks `scripts/check-fail-open-ref-resolution.sh`
  (exit 1 at candidate, exit 0 at base).** The mechanical rustfmt reflow carried
  by checkpoint `69095ba3` re-wrapped `crates/gyre-server/src/api/recovery.rs`
  and shifted the two pre-existing, exemption-pinned legacy fail-open sites
  (`.unwrap_or_default()` on `resolve_ref`) from the frozen
  `scripts/fail-open-ref-resolution-exemptions.txt` lines `…/api/recovery.rs:224`
  and `:275` to lines 232 and 281. The exemptions are path:line-keyed and frozen;
  the reflow invalidated the pinning, so the check now fails on statements whose
  semantics are identical at base. Per AGENTS.md this check runs in pre-commit
  and CI (it is in the baseline probe list), so the candidate cannot pass full
  verification — the task contract requires passing verification and GitHub
  checks. Reproduce: `bash scripts/check-fail-open-ref-resolution.sh` at
  `515d9139` (FAIL) vs at `58deeb75` in a worktree (OK).
  Remedy: drop the content-free reflow of `api/recovery.rs` (restore base
  formatting for the file, or at least the two pinned statements). Do NOT
  re-pin or grow the exemption file — it is frozen; the underlying fail-open
  fix is owned by task-095's revision round.
- [-] **F2 (major): same flaw class breaks `scripts/check-inert-enforcement.sh`
  (exit 1 at candidate, exit 0 at base).** The reflow of
  `crates/gyre-server/src/merge_processor.rs` shifted the two exemption-pinned
  `evaluate_attestation_abac(…)` statement-position discard sites from frozen
  `scripts/inert-enforcement-exemptions.txt` lines
  `…/merge_processor.rs:1457` and `:1479` to lines 1455 and 1477, un-suppressing
  the legacy violations. Same consequence (mechanical gate red at candidate,
  green at base) and same remedy: revert the formatting-only change to
  `merge_processor.rs` rather than touching the frozen exemption file.
- [-] **F3 (minor, process): the `Shipped` narrative in `specs/tasks/task-235.md`
  is stale for this candidate.** It still claims "no Rust/JS source changed
  (`git diff 27bd585c` touches only specs/tasks/)" and "commits: []
  attribution-canonical", but the candidate's net diff vs the assigned base
  touches 11 Rust files, and the frontmatter (correctly) lists three
  product-carrying checkpoint commits (`37fe6100`, `69095ba3`, `ea760e28`) —
  the prose was written for the original 74bf94b1 round and never updated.
  The review-scoping requirement itself is satisfied (all task-labeled
  non-merge product commits are recorded; the gate passes and survives
  mutation), so this is documentation accuracy, not a scoping hole. Remedy:
  update the Shipped section to describe the actual shipped surface (specs
  repair + rustfmt reflow + explorer_ws hardening/tests) and the supersession
  of the original repair by task-231 on main.

Note for the revision round: F1/F2 are repairs of formatting, not of the
underlying legacy violations — the `.unwrap_or_default()` fail-open sites and
the inert `evaluate_attestation_abac` discards are pre-existing at base,
explicitly recorded as legacy debt owned by other tasks, and must not be
"fixed" by editing frozen exemption files or by weakening the checks.
