# Review — task-258 (Agent-to-commit tracking at session granularity)

Spec: `specs/system/source-control.md` §Agent-to-Commit Tracking (gap
spec-36fad8fcafe7b73a2b6f2d0e / agent-session-commit-tracking / b580b93c564f).
Candidate: `c2352d0e551a6d927a2d75b9c4be4505780c5178` (base
`05024b3eb99d474cc523e0a365f14e543641e8f7`). Product commits:
`50452ac5` (implementation), `eff02cff` (clippy repair), `fff11b71`
(review repair: MCP-complete + admin-kill session termination).
Verdict: **approved** — all five spec bullets are real, persisted,
queryable production behavior; no findings.

## What was reviewed

Full diff of the three task-258 product commits (the base→candidate diff
additionally shows task-254 content as "removed" because the candidate
forked from the pre-task-254 task-251 chain and merged the task-249
branch; no task-258 commit touches auth.rs/oidc.rs/message-size code, so
integration's 3-way merge retains task-254 — merge-lineage artifact, not
a finding).

## Per-bullet verification

1. **Session-to-commit mapping (chronological)** — `AgentSessionRepository`
   port (ports/agent_tracking.rs) implemented by SQLite
   (sqlite/session.rs), PostgreSQL (postgres/session.rs), and mem
   (MemAgentSessionRepository) with identical port-contract guards
   (duplicate-id create fails; update of a missing session fails —
   check-mem-port-contracts clean). Shared portable migration
   `2026-11-14-000067_agent_sessions` (next unused number;
   check-migration-versions and check-migration-sql-portability clean).
   Wired via `store!` into `AppState.agent_sessions` (no in-memory state
   store; script clean). Sessions are created at spawn
   (`spawn_agent_core`, initial branch = the worktree record's branch)
   and at each Ralph-loop fresh-context respawn
   (`relaunch_agent_session`, initial branch resolved via a real
   `git rev-parse --abbrev-ref HEAD` in the worktree). `ended_at` is
   stamped on every termination path: REST `complete_agent`, MCP
   `gyre_agent_complete`, `fail_agent`, the stale-agent Dead transition,
   and the admin kill (which runs `end_session` BEFORE worktree cleanup
   so the dirty observation has a worktree to query). `AgentCommit`
   gained `session_id`: `record_pushed_commits` resolves the pushing
   agent's currently-open session (newest open) once per push and stamps
   it; no open session → NULL, never a fabricated id; the manual
   `POST /repos/:id/commits/record` follows the same resolution.
   Chronological query: `GET /repos/:id/agent-sessions/:sid/commits`
   (`find_by_session`, timestamp ASC; adapter test proves ordering).
2. **Branch lifecycle** — `GET /repos/:id/agents/:aid/branch-lifecycle`
   returns the spawn-time branch (first session's `initial_branch`,
   worktree-record fallback), the current branch recorded at the latest
   termination (`final_branch`, resolved by a real worktree lookup at
   termination — never the spawn value copied forward; test
   `end_session_resolves_moved_branch_not_spawn_branch` switches branches
   mid-session and asserts the moved branch), and all observed commits
   (agent's recorded commits, chronological).
3. **Dirty state** — termination runs a real `git status --porcelain` in
   the agent's worktree (char-boundary-safe `line.get(3..)` parsing,
   rename `old -> new` handling) and records `dirty` +
   `uncommitted_paths`; a missing/inaccessible worktree records
   `dirty = NULL` plus `dirty_detail` naming why — explicit unknown,
   never a silent clean. Both directions proven against real git
   worktrees (dirty=true with the modified path listed; clean → false;
   missing worktree → unknown).
4. **Commit-based queries** — `files_changed` recorded per pushed commit
   at push time (`git diff-tree --no-commit-id --name-only -r --root`,
   with a 40-hex SHA guard before the command line) and stored on
   `AgentCommit`. `GET /repos/:id/agent-sessions/by-file?path=` returns
   the sessions (and agents) whose recorded commits touched the path —
   exact path match (substring paths do not match; adapter test
   `find_by_file_exact_path_match`); `by-commit?sha=` returns the linked
   session. Empty results for untouched files / unrecorded SHAs.
5. **Time-travel** — `GET /repos/:id/agent-sessions/:sid/state` returns
   the commits present during `[started_at, ended_at]` via a real
   `git log --all --after/--before` against the repo's object store,
   plus the completion-point state resolved from the existing
   `refs/agents/{agent-id}/snapshots/{n}` refs (newest ref whose commit
   date ≤ ended_at, with tree sha and `ls-tree -r --name-only` files).
   Tests pin commit dates for window determinism and assert a commit
   pushed after the session's end is excluded while the snapshot
   predates it — the view differs from current HEAD by construction.

## Security / invariants

All six new routes (`agent-sessions`, `by-file`, `by-commit`,
`:session_id/commits`, `:session_id/state`, `agents/:agent_id/branch-lifecycle`)
have ABAC `RouteResourceMapping` entries (repo resource);
check-abac-route-registry and check-abac-exempt-handlers clean. Handlers
additionally scope sessions to the repo path segment
(`session_in_repo` → NotFound on cross-repo access). No forged scope
fields, no relative-path defaults, no byte-slice truncation, no
fabricated scope defaults — all corresponding scripts clean.

## Probes (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain agent_tracking` → exit 0 (7 passed).
- `cargo test -p gyre-adapters --lib session::` → exit 0 (6 passed,
  real SQLite).
- `cargo test -p gyre-adapters --lib agent_commit::` → exit 0 (11
  passed, real SQLite; chronological by-session, exact-path by-file,
  session/files round-trip, NULL-stays-NULL).
- `cargo check -p gyre-server --tests` → exit 0 (all new server tests
  type-check).
- 19 mechanical gate scripts → all exit 0.
  `check-task-commit-attribution` exits 1 only against the review stage's
  own uncommitted reset of task-258.md; the committed candidate
  frontmatter lists all three product commits.

gyre-server test execution is deferred to the independent verifier per
the 4 GiB build-budget rule. Verifier must run: `cargo test -p gyre-server
--lib agent_sessions::`, `--lib api::agent_tracking`,
`--lib mcp_agent_complete_terminates_open_session_with_dirty_state`,
`--lib kill_agent_terminates_open_session_before_worktree_cleanup`,
`--lib respawn_opens_a_new_session`,
`--lib complete_agent_ends_session_with_real_dirty_state`, then
`cargo test --all`.

## Test-inflation check

None found. The behavioral tests construct real git repositories and
linked worktrees on disk, pin author/committer dates for deterministic
session windows, and assert observable outcomes (dirty flag + listed
path, moved branch at termination, exclusion of post-window commits,
snapshot tree contents). The two repair-round regression tests
(MCP-complete, admin-kill) assert exactly the behavior that was missing
before `fff11b71` (session stayed open, dirty state unobserved after
worktree cleanup). The two-session attribution test would fail if
session_id were a fabricated constant (both commits would land in one
session).

## Non-blocking observations (recorded for completeness)

- If a respawn ever occurs without a prior termination (or a spawn fails
  after `start_session` — currently no failure point exists after it),
  an older open session could linger without `ended_at`; attribution
  remains correct because `find_open_by_agent` resolves the newest open
  session. Data-quality edge only.
- `end_session` observes the agent's first worktree record; production
  flows hold one worktree per agent.
