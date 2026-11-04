# Review — task-235 (Repair verified failure on main 27bd585ca7eb)

Candidate under review: `2d40775dde7edce113af7f2dd3a5ef6839f8a15e` (merge of branch
tip `5ec1b678` into assigned base `63aa1486210dc579f2fcd4e8b5c6b91e6f66fd77`).
Task record: `specs/tasks/task-235.md` (commits: `69095ba3`, `ea760e28`).
Verdict: **needs-revision** — two blocking code defects introduced by the merge
commit itself; the task cannot "pass full verification and GitHub checks" as
required.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `bash scripts/check-task-commit-attribution.sh` at candidate → **exit 0**
  (`attribution-gate-candidate.txt`). The baseline gate failure (27bd585c
  task-155 squash-drift) is repaired — the SHA is present in
  `specs/tasks/task-155.md` in the assigned base and the candidate does not
  regress it.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --test explorer_ws_integration`
  → **8/8 passed**, including `explorer_ws_rate_limiting` — the durable
  verification finding 11bdc76d (`cargo test --all` exit 101) no longer
  reproduces (`explorer-ws-integration-pass.txt`). Listener probes are
  permitted in this sandbox.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib build_state` → **exit 101,
  compile error E0062** (`lib-tests-compile-failure.txt`). See F1.
- `git show 2d40775d:web/dist/assets/elk.bundled-OwdVv5y8.js | sed -n 1,5p`
  → literal merge-conflict markers (`elk-bundle-conflict-markers.txt`). See F2.
- Token-level comparison (whitespace-stripped + `rustfmt` on base versions) of
  the reflowed files (`merge_processor.rs`, `retention.rs`, `gate_executor.rs`,
  `api/audit.rs`, `api/mr_timeline.rs`, `api/recovery.rs`, `audit_simulator.rs`,
  `container_audit.rs`, `abac_middleware.rs`) → differences are rustfmt reflow
  and use-ordering only; no logic, string, or constant changes.

## Verified working (no findings)

- Task-155 attribution repair is intact at the candidate (gate exit 0; no
  exemptions added — `scripts/task-commit-attribution-exemptions.txt` untouched).
- The explorer WS integration suite passes at the candidate, including the
  previously failing `explorer_ws_rate_limiting` (per-server session registry
  landed upstream via task-239 `483bf8ad`, already in the assigned base).
- The candidate's `explorer_ws.rs` additions are sound in isolation: the
  `GYRE_EXPLORER_MAX_SESSIONS=0` clamp to 1 (avoids `Vec::remove(0)` panic in
  the eviction loop) and deterministic registry-eviction unit tests.
- No check, skip, exemption, or gate is weakened by the diff.

## Findings

- [x] **F1 (blocker, code): `cargo test -p gyre-server --lib` does not compile —
  duplicate struct field.** The merge `2d40775d` combined the branch's
  middleware.rs test-helper addition (`explorer_sessions:
  base.explorer_sessions.clone()`, line 308, authored on `5ec1b678` when the
  older base lacked the field) with the assigned base's own addition from
  task-239 (`explorer_sessions: Arc::clone(&base.explorer_sessions)`, line
  233). Both survived the merge ~75 lines apart in the same `AppState` struct
  literal → rustc E0062 "field `explorer_sessions` specified more than once",
  exit 101. Consequence: `cargo test --all` fails at compile; none of the
  candidate's new unit tests (`build_state_isolates_explorer_sessions_per_server`,
  `test_session_registry_*`, the clamp test) can even execute; full verification
  and GitHub checks cannot pass. Reproduce: `SKIP_WEB_BUILD=1 cargo test -p
  gyre-server --lib` (evidence: `lib-tests-compile-failure.txt`). Remedy: drop
  the branch-side duplicate at line 308 (base's line 233 is equivalent).
- [x] **F2 (blocker, code: corrupted product artifact): unresolved Git merge
  conflict markers committed into `web/dist/assets/elk.bundled-OwdVv5y8.js`.**
  The candidate file begins with `<<<<<<<< HEAD:web/dist/assets/elk.bundled-CZ1SLFkJ.js`
  (line 1), `========` (line 3), `>>>>>>>> 63aa1486...:web/dist/assets/elk.bundled-OwdVv5y8.js`
  (line 5) — the merge of the branch's rebuilt bundle (CZ1SLFkJ.js, importing
  `./index-DauSjDxg.js`) against the base's bundle (OwdVv5y8.js) was committed
  unresolved. The corrupted file is referenced by
  `web/dist/assets/index-D4CX8rVo.js` (the bundle loaded by `web/dist/index.html`),
  so the dashboard's elk graph view hits a JS SyntaxError at runtime; the
  HEAD-side content also imports `./index-DauSjDxg.js`, which does not exist in
  the candidate tree. Reproduce: `git show 2d40775d:web/dist/assets/elk.bundled-OwdVv5y8.js
  | sed -n 1,5p` (evidence: `elk-bundle-conflict-markers.txt`). Remedy: resolve
  the merge to the base side (the candidate changed no web source, so the
  base bundle is the correct content) and delete the stray branch-side
  bundle reference.
- [x] **F3 (process): the task record's Shipped narrative is false for this
  candidate.** `specs/tasks/task-235.md` claims "no Rust/JS source changed
  (diff vs base touches only specs/tasks/)" and "commits: [] is
  attribution-canonical for this specs-only repair round", but the diff from
  the assigned base `63aa1486` modifies 12 Rust files, `web/dist/`, and lists
  checkpoint SHAs `69095ba3`/`ea760e28` in `commits:` whose product-surface
  content (explorer_ws registry tests, the session-limit clamp, a tree-wide
  rustfmt reflow of merge_processor/retention/gate_executor and six other
  files) is entirely undescribed in the task record. The narrative was written
  for the original base `27bd585c` and never updated after the branch was
  merged onto `63aa1486`. A reviewer scoping to the Shipped section would
  believe this is a 1-line specs-only change.

## Notes for the verifier

- The two blockers were introduced by the candidate merge commit `2d40775d`
  itself (first parent `5ec1b678` has one `explorer_sessions` field at line 307
  and no `elk.bundled-OwdVv5y8.js`; base `63aa1486` has one field at line 233
  and a clean `elk.bundled-OwdVv5y8.js`). Neither defect exists in either
  parent — this is a broken merge resolution, not inherited drift.
- The rustfmt reflow noise (~460 lines across 9 files) is semantically neutral
  (verified token-identical modulo whitespace/use-order) but is attributed to
  task-235 via the checkpoint SHAs; a revision should ideally drop it to keep
  the task diff minimal.
