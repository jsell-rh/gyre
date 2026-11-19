# Review — task-258 (Agent-to-commit tracking at session granularity)

Spec: `specs/system/source-control.md` §Agent-to-Commit Tracking (spec gap
spec-36fad8fcafe7b73a2b6f2d0e).
Candidate: `c3d44af2ec7ed220c5f336f590cfad39a47b0e4b` (base `25497b9e`);
implementation commit `50452ac5`.
Verdict: **approved** — all five bullets are real, persisted, queryable
production behavior; no hollow modes found.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → exit 0.
- `cargo check -p gyre-server --tests` → exit 0 (test code type-checks;
  server test *execution* is deferred to the verifier per the review-stage
  build wrapper — required probes listed below and in review-summary.md).
- `cargo test -p gyre-adapters session` → 17 passed (SQLite session
  adapter contract tests: duplicate-id create fails, update-missing fails,
  open-session resolution, termination round-trip incl. dirty=NULL +
  dirty_detail unknown state).
- `cargo test -p gyre-adapters agent_commit` → 11 passed (incl.
  find_by_session_chronological, find_by_file_exact_path_match,
  session_and_files_round_trip with NULL-session preservation).
- `cargo test -p gyre-domain agent_tracking` → 7 passed.
- Mechanical checks all exit 0 with no new exemption entries:
  check-mem-port-contracts, check-abac-route-registry,
  check-abac-exempt-handlers, check-in-memory-state-stores,
  check-migration-versions, check-migration-sql-portability, check-arch,
  check-byte-slice-truncation, check-lossy-secret-conversion,
  check-forged-scope-fields, check-inert-enforcement,
  check-dead-message-kinds, check-task-commit-attribution (on the
  candidate tree; see note on the arriving unstaged frontmatter revert in
  review-summary.md).

## Verified working

- **Session persistence (bullet 1).** `AgentSessionRepository` port
  (gyre-ports/agent_tracking.rs) implemented by SQLite
  (sqlite/session.rs), PostgreSQL (postgres/session.rs), and mem
  (MemAgentSessionRepository) with identical create-duplicate and
  update-not-exists guards; shared migration `2026-11-14-000067_agent_sessions`
  (next unused number after 000066, portable SQL — both DB runners embed
  the same dir); wired into `AppState.agent_sessions` via the `store!`
  pattern (DB-backed in production, mem only for pure-mem mode). Sessions
  created at `spawn_agent_core` and at each Ralph-loop fresh-context
  respawn (`relaunch_agent_session`, initial branch resolved from the
  worktree's real current branch); `ended_at` set on `complete_agent`,
  `fail_agent`, and the stale-agent Dead transition (stale_agents.rs,
  deliberately before worktree cleanup so the dirty observation has a
  worktree to read).
- **Session-to-commit mapping.** `AgentCommit.session_id` stamped in
  `git_http.rs record_pushed_commits` and `POST /repos/:id/commits/record`
  from the pushing agent's resolved open session
  (`find_open_by_agent`); NULL when none — never a fabricated id (tested:
  the orphan-commit case asserts `session_id` is null).
  `GET /repos/:id/agent-sessions/:sid/commits` returns the session's
  commits chronologically (timestamp ASC in SQL; sort in mem).
- **Branch lifecycle (bullet 2).** `GET /repos/:id/agents/:aid/branch-lifecycle`
  returns the spawn-time branch (first session's `initial_branch`), the
  current branch recorded at termination via a real
  `git rev-parse --abbrev-ref HEAD` in the agent's worktree (tested to
  differ after a mid-session branch move; spawn value preserved), and all
  observed commits (agent's recorded commits, chronological).
- **Dirty state (bullet 3).** Real `git status --porcelain` in the
  worktree on every termination path; modified + untracked paths listed;
  missing/inaccessible worktree records `dirty = NULL` plus
  `dirty_detail` naming why — never a silent clean (each case has a test
  against a real git worktree on disk).
- **Commit-based queries (bullet 4).** `files_changed` recorded per pushed
  commit at push time (`git diff-tree --no-commit-id --name-only -r
  --root <sha>` with a 40-hex SHA guard before the command line; the
  manual record endpoint derives from the same query, with a documented
  caller-asserted fallback only when the SHA does not resolve in the
  object store). `GET .../agent-sessions/by-file?path=` (exact path match
  in Rust — substring/LIKE-injection-safe) and `by-commit?sha=` return the
  producing sessions; untouched file / unrecorded SHA return empty lists
  (tested).
- **Time-travel (bullet 5).** `GET .../agent-sessions/:sid/state` runs a
  real `git log --all --after/--before` against the repo's object store
  for [started_at, ended_at] and resolves the completion snapshot from the
  `refs/agents/{agent-id}/snapshots/{n}` refs (newest ref whose commit
  date ≤ ended_at, with tree SHA + tree files). Tested with deterministic
  commit dates against a real bare repo: the during-session commit is
  present, the after-end commit is excluded, and the snapshot pre-dates
  the later commit — i.e. the view is not current HEAD.
- **ABAC / scope.** All six new routes have `RouteResourceMapping` entries
  (repo resource, `:id` path param — the middleware resolves
  repo→workspace→tenant and evaluates policy, same as the sibling
  agent-commits route); no exemption-file growth; handlers additionally
  scope-check sessions against the repo path segment (cross-repo session
  → 404). check-abac-route-registry / check-abac-exempt-handlers /
  check-forged-scope-fields all clean.
- **Persistence across restart.** Production state is DB-backed (SQLite/PG
  adapters + migration); the adapter round-trip tests prove the
  termination fields survive storage.

## Test-inflation check

None found. The key tests drive real git repos/worktrees on disk with
pinned author/committer dates (window membership is deterministic), and
each would fail in the hollow mode the spec gap invited: a hardcoded
dirty=false fails the uncommitted-edit test; a fabricated constant
session id fails two-session attribution (both commits would land in one
session); an empty files_changed Vec fails the by-file query; returning
current HEAD fails the time-travel exclusion assertion. The
`respawn_opens_a_new_session` test ends session 1 via direct state setup
before driving `on_gates_failed` — acceptable because in the production
flow the previous context window is ended by `complete_agent` (MR
creation precedes gate failure), which is separately tested end-to-end in
`complete_agent_ends_session_with_real_dirty_state`.

## Deferred to the verifier (recorded in review-evidence)

gyre-server test execution (build-wrapper budget): `cargo test --all`,
specifically the 13 tests listed in
/tmp/stage/review-evidence/review-summary.md (agent_sessions::*,
spawn::complete_agent_ends_session_with_real_dirty_state,
ralph_loop::respawn_opens_a_new_session, and the four
api::agent_tracking::tests session endpoints tests).

## Minor non-blocking observations (no action required)

- lib.rs doc comment now cites
  `AgentSessionRepository::delete_expired_before`, but that contract
  belongs to the auth `SessionRepository` (user_profile.rs) — comment-only
  inaccuracy introduced by this diff.
- Commit bfc625ad's message prose cites a stale pre-rebase SHA
  (04dc4acb); the task frontmatter correctly records 50452ac5.
- The arriving working tree had an unstaged revert of task-258.md
  frontmatter (executor-side, not part of the candidate); the attribution
  check passes on the committed candidate state.
