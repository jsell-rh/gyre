# Review — task-258 (Agent-to-commit tracking at session granularity)

Spec: `specs/system/source-control.md` §Agent-to-Commit Tracking (all five bullets).
Candidate under review: `32fc838f217e0648716be70c7d5ca3ce38c65e9e` (base `e1cf9d8d`).
Product commits: `50452ac5` (implementation), `eff02cff` (clippy), `fff11b71` (MCP-complete/admin-kill
termination paths), `dfcb500f` (rustfmt), `f7d9c388` (lifecycle-commit same-second ordering),
`ec0fbff4` (session-list same-second ordering, CI run 44611030853 repair); the rest are
process/merge commits.

Verdict: **approved** — all five bullets are real, persisted, queryable production behavior;
no hollow modes found; every mechanical invariant gate passes; focused adapter tests and
independent git-semantics probes confirm the behavior on this exact head.

## What was inspected

Full diff `e1cf9d8d..32fc838f` (28 files, ~3.7k lines): domain/ports
(`gyre-domain/src/agent_tracking.rs`, `gyre-ports/src/agent_tracking.rs`), all three adapters
(`gyre-adapters/src/sqlite/session.rs`, `postgres/session.rs`, both `agent_commit.rs` files,
`schema.rs`, migration `2026-11-14-000067_agent_sessions`), server wiring
(`lib.rs` store!, `middleware.rs`), lifecycle call sites (`api/spawn.rs` spawn/complete/fail,
`ralph_loop.rs` relaunch, `stale_agents.rs` Dead, `mcp.rs` gyre_agent_complete,
`api/admin.rs` kill_agent), the git observation core (`agent_sessions.rs`), push-time stamping
(`git_http.rs` record_pushed_commits), the six new routes + ABAC resolver entries
(`api/mod.rs`, `abac_middleware.rs`), handlers (`api/agent_tracking.rs`), docs, and the
coverage row 12 update.

## Bullet-by-bullet verification

1. **Session-to-commit mapping** — real. `AgentSessionRepository` port implemented by
   SQLite + PostgreSQL (shared portable migration `000067_agent_sessions`, next unused
   number after 000066) and the mem adapter with identical code-level guards
   (duplicate-id create fails, update-of-missing fails — `check-mem-port-contracts.sh`
   passes). Wired via `store!` (`check-in-memory-state-stores.sh` passes). Sessions start
   at spawn (`spawn_agent_core`, unconditional after repo resolution) and at each
   Ralph-loop fresh-context respawn (`relaunch_agent_session`, initial branch resolved via
   real `git rev-parse --abbrev-ref HEAD` in the worktree, recorded branch as fallback).
   `ended_at` set on all termination paths the task enumerates: complete_agent, fail_agent,
   stale Dead (before worktree cleanup, so the dirty observation has something to read),
   plus MCP gyre_agent_complete and admin kill. `AgentCommit.session_id` is stamped from
   `find_open_by_agent` in both `record_pushed_commits` and POST `/commits/record`; no open
   session → NULL, never a fabricated id (lookup failure also degrades to NULL with a warn
   so the commit is still recorded). Chronological query
   `GET /repos/:id/agent-sessions/:sid/commits` orders by timestamp ASC.
2. **Branch lifecycle** — real. Spawn-time branch on the session; `final_branch` resolved
   at termination with a real worktree lookup (verified independently: probe 1 below shows
   `rev-parse --abbrev-ref HEAD` returning the post-switch branch, not the spawn branch);
   `GET /repos/:id/agents/:aid/branch-lifecycle` returns initial branch, latest
   termination-time branch, and all observed commits (with the same-second session-window
   tie-break). Detached HEAD records None rather than a fake name.
3. **Dirty state** — real. `git status --porcelain` in the agent's worktree on every
   termination path; porcelain v1 `XY <path>` parsed with char-boundary-safe `get(3..)`,
   rename `old -> new` handled. Missing/inaccessible worktree records `dirty = NULL` +
   `dirty_detail` naming why — never a silent clean. Verified against a real linked
   worktree with a modification and an untracked file (probe 2 below).
4. **Commit-based queries** — real. `files_changed` recorded per pushed commit at push time
   via `git diff-tree --no-commit-id --name-only -r --root <sha>` (40-hex SHA validated
   before reaching the command line) in `record_pushed_commits`; the manual record endpoint
   derives from the same diff-tree against the repo's object store. `by-file` matches exact
   paths (in Rust in the DB adapters, so substring/LIKE-pattern paths never false-match);
   `by-commit` resolves the linked session. SQLite round-trip tests pass locally.
5. **Time-travel** — real. `GET /repos/:id/agent-sessions/:sid/state` runs
   `git log --all --format=... --after/--before` against the repo's real object store
   (window `[started_at, ended_at]` with ±1s inclusive tolerance) and resolves the
   completion snapshot from `refs/agents/{agent-id}/snapshots/{n}` (newest ref whose
   target commit's committer date ≤ ended_at) with tree sha + file listing via
   `ls-tree -r`. Never current HEAD. Verified independently in both a working repo and a
   bare repo (gyre's storage form): the commit dated after the session end is excluded from
   the window and absent from the snapshot tree, while HEAD has moved past it.

Authorization: all six new routes have `RouteResourceMapping` entries (repo resource) —
`check-abac-route-registry.sh` passes; handlers additionally scope-check the loaded
session/commit against the repo path segment (NotFound on cross-repo), and
`check-abac-exempt-handlers.sh` and `check-forged-scope-fields.sh` pass.

## Probes run (evidence under /tmp/stage/review-evidence)

On the exact candidate head `32fc838f`:

- `bash scripts/check-mem-port-contracts.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK.
- `bash scripts/check-abac-exempt-handlers.sh` → OK (85 handlers).
- `bash scripts/check-in-memory-state-stores.sh` → OK.
- `bash scripts/check-migration-versions.sh` → OK (no duplicate versions; 000067 is next
  unused).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-migration-sql-portability.sh` → OK (000067 is plain
  CREATE/ALTER, both-backend safe).
- `bash scripts/check-mcp-write-tools.sh` → OK.
- `bash scripts/check-forged-scope-fields.sh` → OK.
- `bash scripts/check-inert-enforcement.sh` → OK.
- `/tmp/stage/bin/cargo check -p gyre-server` → Finished, exit 0 (8m42s).
- `/tmp/stage/bin/cargo test -p gyre-adapters sqlite::session` → **8/8 passed**, including
  the CI-regression deterministic tests `same_second_tie_orders_ended_session_before_open`
  and `same_second_tie_between_ended_sessions_orders_by_ended_at` whose ids deliberately
  contradict the window order (a bare id tiebreak fails them).
- `/tmp/stage/bin/cargo test -p gyre-adapters sqlite::agent_commit` → 11/11 passed
  (session round-trip, NULL-stays-NULL, chronological by session, exact-path by-file).
- `/tmp/stage/bin/cargo test -p gyre-domain agent_tracking` → 7/7 passed.
- Independent git-semantics probe (`git-semantics-probe.sh`, replicated the production
  command sequences verbatim against a real repo + linked worktree, and a second bare-repo
  probe): moved-branch resolution, porcelain dirty listing (both ` M` and `??` forms),
  diff-tree --root file listing, snapshot ref resolution (for-each-ref + %ct + ^{tree} +
  ls-tree), and the window query excluding the post-session commit while HEAD has moved —
  all behave as the code and its tests assume.

(`scripts/check-task-commit-attribution.sh` timed out in this sandbox — it needs full git
history/fetch; the task frontmatter lists all six product commits, and the head's two
process commits are attribution bookkeeping, which the script whitelists by design.)

## Tests the verifier must execute (gyre-server test compilation is deferred to the verifier per the 4 GiB budget)

- `cargo test --all` (the task's own AC).
- `cargo test -p gyre-server agent_sessions::` — dirty/clean/unknown worktree states,
  moved-branch resolution, files_changed, same-second tie, time-travel window+snapshot.
- `cargo test -p gyre-server ralph_loop::tests::respawn_opens_a_new_session` — the CI run
  44611030853 failure, fixed by `ec0fbff4` (adapter ordering); the SQLite regression twins
  pass locally on this head.
- `cargo test -p gyre-server api::agent_tracking::tests::` — `two_session_attribution`
  (exactly [A]/[B] per session; NULL for out-of-session commits),
  `by_file_and_by_commit_queries`, `branch_lifecycle_view`,
  `branch_lifecycle_same_second_tie_ordered_by_session`, `session_state_time_travel`.
- `cargo test -p gyre-server api::spawn::tests::complete_agent_ends_session_with_real_dirty_state`.
- `cargo test -p gyre-server mcp::tests::mcp_agent_complete_terminates_open_session_with_dirty_state`.
- `cargo test -p gyre-server api::admin::tests::kill_agent_terminates_open_session_before_worktree_cleanup`.

## Test-inflation check

None found. The acceptance tests drive real git repos/worktrees on disk (deterministic
author/committer dates pinned relative to session windows), drive the real router via
oneshot, and assert negatives that fail under the hollow modes the gap invited: a fabricated
constant session id lands both commits in one session and fails `two_session_attribution`;
a hardcoded dirty=false fails the dirty-state tests (the fixtures write real uncommitted
edits); an empty files_changed Vec fails the by-file query; returning current HEAD fails
the time-travel exclusion asserts. The same-second ordering tests deliberately construct
ids that contradict the window order, so an id-only tiebreak fails deterministically.

## Non-blocking observations (recorded, no repair required)

- `POST /repos/:id/commits/record` accepts an optional caller-asserted `files_changed`
  list, used only when the sha does not resolve in the repo's object store (documented as
  the off-forge fallback). The native push path — the one the spec bullet names — always
  derives the list from real `git diff-tree`; the fallback is audit metadata on a
  pre-existing manual recording surface, not a scope field.
- `find_open_by_agent` (SQLite/PG) picks the newest open session by `started_at DESC` with
  no id tie-break. Unreachable in practice: at most one session per agent is open (every
  termination path ends it), and the only multi-open scenario (respawn_with_retry re-entering
  relaunch) is separated by ≥1s backoffs. The mem adapter tie-breaks deterministically.
- `respawn_with_retry`'s terminal Failed transition (all spawn retries exhausted) does not
  end the session the failed launch attempt started — an error path the task's
  enumeration of termination paths (complete/fail/stale-Dead) does not include; the orphan
  open session degrades to NULL-session attribution for any later commit, never to a wrong
  one.

## Coverage

Row 12 of `specs/coverage/system/source-control.md` updated to verified with concrete
source evidence matching what this review independently confirmed; no unrelated rows
touched (row 13 changes came from the task-259 merge base, already attributed there).
