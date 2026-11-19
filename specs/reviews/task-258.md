# Review — task-258 (Agent-to-commit tracking at session granularity)

Spec: `specs/system/source-control.md` §Agent-to-Commit Tracking (five bullets).
Candidate under review: `c1efa33a4134449fac06d5a80112824aa9df2f65` (base `bafc03531aec9c9ddd05c52497aa10d0d8856fe5`).
Product commits: `50452ac` (implementation), `eff02cff` (clippy repair); remainder process/checkpoint commits.
Verdict: **needs-revision** — one material finding (F1: MCP completion path skips session termination); one minor doc finding (F2). Everything else verified clean and real.

## Probes run (checkout at candidate `c1efa33a`)

All evidence saved under `/tmp/stage/review-evidence/`.

- `bash scripts/check-migration-versions.sh` → OK (000067 is the next unused number after 000066).
- `bash scripts/check-mem-port-contracts.sh` → OK (mem `create`/`update` guards identical to SQLite/PG).
- `bash scripts/check-abac-route-registry.sh` → OK (all six new routes resolve, repo resource).
- `bash scripts/check-abac-exempt-handlers.sh` → OK (85 handlers).
- `bash scripts/check-in-memory-state-stores.sh` → OK (AppState uses `store!`, port-backed).
- `bash scripts/check-migration-sql-portability.sh` → OK (plain CREATE/ALTER, both backends).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-scope-literal-defaults.sh`, `check-forged-scope-fields.sh`, `check-fabricated-scope-defaults.sh` → OK.
- `cargo test -p gyre-domain agent_tracking` → 7 passed.
- `cargo test -p gyre-adapters --lib session::` → 6 passed (chronological ordering, duplicate-id failure, open-session resolution, termination-state round-trip incl. dirty=NULL+detail, missing-session update failure).
- `cargo test -p gyre-adapters --lib agent_commit` → 11 passed (session_id/files_changed round-trip incl. NULL preservation, `find_by_session` chronological + no cross-session leak, `find_by_file` exact-path matching with substring/other-repo negatives).
- `cargo check -p gyre-server --tests` → PASS, exit 0 (7m39s) — lib + all test targets type-check at the candidate.
- `cargo clippy -p gyre-ports --lib` → only pre-existing `WorktreeRepository` double_must_use (line 63, unchanged); the two new/extended traits carry the allow, matching the shipped notes.
- gyre-server test execution is blocked by the executor's 4 GiB build wrapper (recorded in review-evidence); deferred to the verifier — see below.

## Verified working (no findings)

- **Session persistence is real and port-backed.** `AgentSessionRepository` port (ports/agent_tracking.rs) with create/find_by_agent/find_by_repo/find_by_id/find_open_by_agent/update; SQLite adapter (portable migration `000067_agent_sessions`, `treat_none_as_null` so termination NULLs persist), PostgreSQL adapter (identical guards), mem adapter (identical duplicate-id / not-exists guards — mem-port-contract gate clean). Wired via `store!` into `AppState.agent_sessions`.
- **Session lifecycle on the three assigned termination paths.** Spawn starts a session (`spawn_agent_core`, initial branch = `req.branch`); Ralph-loop fresh-context respawn starts a new one (`relaunch_agent_session`, initial branch resolved from the worktree's real current branch via `git rev-parse`, falling back to the recorded branch); `complete_agent`, `fail_agent`, and the stale-agent Dead transition all call `end_session` (stale path before worktree cleanup, so the dirty-state observation has something to observe). The duplicate-completion early return in `complete_agent` is idempotency-only (first call already ended the session). The Ralph production flow is coherent: complete (ends session 1) → gate failure → respawn (opens session 2).
- **Session-to-commit mapping.** `AgentCommit.session_id` stamped in `record_pushed_commits` (resolved once per push from the pushing agent's open session) and in POST `/repos/:id/commits/record`; NULL when no open session or lookup failure — never a fabricated id (adapter round-trip test asserts NULL preservation). Chronological `find_by_session` (timestamp ASC, id tiebreak) in all three adapters; mem sorts in code with the same key.
- **Dirty state is a real VCS query.** `end_session` runs `git status --porcelain` in the agent's worktree (recorded worktree path); porcelain parsing uses `line.get(3..)` (char-boundary safe) and handles rename arrows. Missing worktree record or git failure records `dirty = NULL` + `dirty_detail` — never a silent clean (unit-tested both ways against a real linked git worktree). Current branch at termination is resolved with `git rev-parse --abbrev-ref HEAD` in the worktree — not the spawn value copied forward (tested with a mid-session `git switch -c`).
- **Commit-based queries.** `files_changed` recorded per pushed commit at push time (`git diff-tree --no-commit-id --name-only -r --root`, SHA validated 40-hex before reaching the command line); stored on AgentCommit as JSON; `find_by_file` matches exact paths in Rust (no LIKE-substring false positives — tested); by-file and by-commit endpoints group commits into session hits with repo-scope filtering.
- **Time-travel.** `commits_in_window` is a real `git log --all --after/--before` against the repo's object store (inclusive window via ±1s); `completion_snapshot` resolves `refs/agents/{agent-id}/snapshots/{n}` via for-each-ref and picks the newest whose commit date ≤ ended_at, with tree sha (`^{tree}`) and `ls-tree -r` files. Tests use real bare repos with pinned GIT_*_DATE epochs and assert a post-session commit is excluded from both the window and the snapshot, and that the snapshot differs from current HEAD.
- **ABAC / scope handling.** All six new routes have `RouteResourceMapping` entries (repo resource); handlers additionally scope sessions/commits to the path repo (session in another repo → NotFound/empty, tested). No forged scope fields: session_id is always server-resolved.
- **Tests are hard, not self-confirming:** real git repos and linked worktrees on disk, deterministic commit dates, negative cases (orphan commit NULL session, substring path, other repo, unknown agent 404, unrecorded SHA, missing worktree unknown-state).

## Findings

### F1 (material — missing enforcement on a production termination path)

`crates/gyre-server/src/mcp.rs` — `handle_agent_complete` (the `gyre_agent_complete` MCP tool, dispatched at mcp.rs:5041) terminates the agent (Idle transition, token revocation, inbox ack, secret revocation, MR creation, Ralph-loop hook — the code itself claims "MCP parity with the REST POST /agents/:id/complete surface") **without calling `agent_sessions::end_session`**. The agent-runtime Phase-5 spec names the MCP tool as the completion path (the in-code comment at mcp.rs:2323 says so explicitly), and docs/agent-protocol.md documents agents calling `gyre_agent_complete`. For an agent completing via this path:

- the session never gets `ended_at`, `final_branch`, or dirty state — the spec bullet "did the agent leave uncommitted work when it terminated?" is unanswerable on this path;
- on the subsequent Ralph-loop respawn a second session opens while the first stays open forever (a later `end_session` ends only the newest open session), so the session list lacks end times and the two-session acceptance criterion's "start/end times" fails on this flow.

Reproduce: complete an agent via MCP `gyre_agent_complete` with an uncommitted edit in its worktree, then GET `/repos/:id/agent-sessions` — the session shows `ended_at: null`, `dirty: null`. Fix is small: call `crate::agent_sessions::end_session(state, &agent).await` in `handle_agent_complete` (the worktree is already resolved there for `complete_via_mr`).

Same class, secondary: admin `kill_agent` (api/admin.rs:369) also transitions to Dead without ending the session (task named only the stale-agent Dead transition, but an admin-killed agent's session likewise stays open indefinitely).

The task text enumerated three termination paths and all three are implemented; F1 is about the spec-level "every termination path" contract (goal rule: specs are the contract) being hollow on the spec-named MCP completion path, which the candidate's own code treats as the REST-completion sibling for every other teardown concern.

### F2 (minor — doc defect introduced by the diff)

`crates/gyre-server/src/lib.rs:2078` — the comment was renamed from `SessionRepository::delete_expired_before` to `AgentSessionRepository::delete_expired_before`, but `delete_expired_before` belongs to the auth-session `SessionRepository` trait (gyre-ports/src/user_profile.rs:73, invoked as `state.sessions.delete_expired_before` at lib.rs:2093). `AgentSessionRepository` has no such method; the renamed comment misattributes the never-delete-revoked-rows port contract to the wrong trait. Restore the original trait name in the comment.

## Observations (non-blocking)

- Working tree has an uncommitted revert of `specs/tasks/task-258.md` (progress/commits/Shipped section) — controller/stage artifact, not a source defect; the candidate commit itself contains the ready-for-review state.
- `find_by_file` (SQLite/PG) loads all repo commits and filters in Rust — correct but O(repo commits); fine at current scale.
- `record_commit`'s caller-asserted `files_changed` fallback only applies when the sha does not resolve in the repo's object store; the native push path always derives from `git diff-tree`. Documented in the API reference.

## Verifier must run (deferred — blocked by the 4 GiB build wrapper here)

- `cargo test --all` (full suite).
- Focused task-258 server tests: `cargo test -p gyre-server --lib agent_sessions`, `cargo test -p gyre-server --lib agent_tracking::tests` (two_session_attribution, by_file_and_by_commit_queries, branch_lifecycle_view, session_state_time_travel), `cargo test -p gyre-server --lib respawn_opens_a_new_session`, `cargo test -p gyre-server --lib complete_agent_ends_session_with_real_dirty_state`.
- `python3 scripts/check-clippy-diff.py bafc03531aec9c9ddd05c52497aa10d0d8856fe5` (workspace clippy on changed lines).
- `bash scripts/check-task-commit-attribution.sh` (needs full history).
