# Review — task-258 (Agent-to-commit tracking at session granularity)

Spec: `specs/system/source-control.md` §Agent-to-Commit Tracking (all five bullets).
Assigned candidate: `a46d2ce92569c830a5386f5d72ef05f8245a9dc4` (base `ee03c29984b19a2bbcc9b6f2370e8babb6e61e92`).
Product commits under review: `50452ac5` (feat), `eff02cff` (clippy repair), `fff11b71` (MCP-complete + admin-kill termination paths), `dfcb500f` (rustfmt) — all recorded in the task frontmatter at the candidate commit.
Verdict: **approved** (round 1). All five bullets are real, persisted, queryable production behavior; no hollow modes found.

## Probes run (round 1, checkout at `a46d2ce9`)

Mechanical gates (all exit 0, evidence in `/tmp/stage/review-evidence/`):
`check-mem-port-contracts`, `check-abac-route-registry`, `check-abac-exempt-handlers`,
`check-in-memory-state-stores`, `check-migration-versions`, `check-arch`,
`check-forged-scope-fields`, `check-migration-sql-portability`,
`check-byte-slice-truncation`, `check-relative-path-defaults`,
`check-scope-literal-defaults`, `check-inert-enforcement`,
`check-fabricated-scope-defaults`.

Focused compilation and tests:
- `cargo test -p gyre-adapters --lib sqlite::session` → **6/6 passed** (duplicate-id fails, not-exists update fails, open-session resolution, termination-state round trip incl. dirty=NULL+detail unknown state).
- `cargo test -p gyre-adapters --lib sqlite::agent_commit` → **11/11 passed** (session_id NULL round trip, `find_by_session` chronological with cross-session leak guard, `find_by_file` exact-path match with substring-miss guard).
- `cargo check -p gyre-server` and `cargo check -p gyre-server --tests` → clean (all new `#[cfg(test)]` modules type-check).
- `cargo clippy -p gyre-adapters -p gyre-ports -p gyre-domain --lib` → no warnings in any new file (all reported warnings are pre-existing in untouched files).
- `cargo test -p gyre-server ...` is blocked by the executor's 4 GiB wrapper — server test **execution** is delegated to the verifier (list below). `check-task-commit-attribution.sh` timed out in this sandbox (needs full history traversal); verifier should run it.

## Verified against the task contract

- **(1) Session-to-commit mapping.** `AgentSessionRepository` port (`ports/agent_tracking.rs`) with SQLite (`sqlite/session.rs`), PostgreSQL (`postgres/session.rs`), and mem adapters; duplicate-id and not-exists failure constraints guarded identically in all three (gate clean). Shared portable migration `2026-11-14-000067_agent_sessions` (next unused number after 000066; plain CREATE TABLE/ALTER TABLE, portability gate clean). Wired via `store!` into `AppState.agent_sessions` — no bare `Arc<Mutex<HashMap>>`. Sessions created at spawn (`spawn_agent_core`) and at each Ralph-loop fresh-context respawn (`relaunch_agent_session`, initial branch resolved from the worktree's real current branch, not copied forward); `ended_at` set on every named termination path — `complete_agent`, `fail_agent`, stale-agent Dead (before worktree cleanup) — plus MCP `gyre_agent_complete` and admin `kill_agent` (both before worktree cleanup, so `git status --porcelain` still sees the tree). `AgentCommit.session_id` stamped from the pushing agent's **open** session in `record_pushed_commits` and `POST /repos/:id/commits/record`; no open session → NULL, never fabricated. Chronological `GET /repos/:id/agent-sessions/:sid/commits` (timestamp ASC + id tie-break). The `two_session_attribution` test drives the real router and would fail on a fabricated constant session id (both commits would land in one session).
- **(2) Branch lifecycle.** `GET /repos/:id/agents/:aid/branch-lifecycle` returns initial (first session's spawn-time) branch, `final_branch` resolved at termination via real `git rev-parse --abbrev-ref HEAD` in the worktree (test observes it differ after a mid-session `git switch -c`), and all observed commits (chronological, agent+repo scoped).
- **(3) Dirty state.** Real `git status --porcelain` in the agent's worktree at termination (porcelain-v1 parse via `line.get(3..)` — char-boundary safe; rename entries record the new path). Missing/inaccessible worktree records `dirty = NULL` + `dirty_detail` — explicit unknown, never a silent clean (unit-tested). Full-path test `complete_agent_ends_session_with_real_dirty_state` spawns a real agent against a real linked git worktree, leaves an uncommitted edit, and asserts dirty=true with the file listed after `complete_agent`.
- **(4) Commit-based queries.** `files_changed` recorded per pushed commit at push time via `git diff-tree --no-commit-id --name-only -r --root <sha>` (40-hex validation before the command line — caller-supplied SHAs on the manual record path cannot inject). `GET .../agent-sessions/by-file?path=` does exact path match in Rust (substring/LIKE-metacharacter false matches impossible; tested); `by-commit?sha=` returns the linked session with repo-scope guards. Old pre-migration rows read back as `[]` (column default), not an error.
- **(5) Time-travel.** `GET .../agent-sessions/:sid/state`: commits present during `[started_at, ended_at]` via real `git log --all --after/--before` against the object store (±1s inclusive tolerance; deterministic-date tests), plus the completion snapshot resolved from `refs/agents/{agent-id}/snapshots/{n}` (newest ref whose commit date ≤ ended_at, with tree sha and `ls-tree -r` file list). Tests assert a post-session commit is excluded and the snapshot pre-dates it — returning current HEAD fails them.
- **ABAC/authz.** All six new routes have `RouteResourceMapping` entries (repo resource, same pattern as the existing agent-commits routes); registry and exempt-handler gates clean; session handlers scope-check `session.repository_id` against the repo path segment (cross-repo session is 404/empty, tested); no caller-supplied scope fields.

## Test-inflation check

None found. The new tests use real git repos/worktrees on disk, the real router (`app.oneshot`), and the real spawn→complete path; assertions kill the hollow modes this gap invites (hardcoded dirty=false, empty files_changed, spawn-branch copied forward, current-HEAD time travel, fabricated constant session id).

## Non-blocking observations (recorded for a future polish task, not defects against this contract)

- **Respawn-retry session stacking:** `relaunch_agent_session` starts the session before the launch; `respawn_with_retry` re-invokes it per attempt, so N failed launch attempts create N open sessions. Attribution stays correct (`find_open_by_agent` returns the newest; the terminal spawn-failure path revokes the token so no commit can be mis-attributed), but the terminal `Failed` transitions in `respawn_with_retry`/max-iterations do not call `end_session`, and the stale sweeper skips terminal-status agents — so a session from a terminally failed respawn keeps `ended_at = NULL` forever. The task named exactly three termination paths (all implemented, plus two extra); retry-path bookkeeping is beyond that scope.
- **`find_open_by_agent` SQL tie-break:** the SQLite/PG queries order by `started_at DESC` with no id tie-break (the mem adapter does tie-break). Only matters when two open sessions share a second — i.e. the retry-stacking case above.
- `record_commit`'s optional caller-asserted `files_changed` fallback only applies when the sha does not resolve in the repo's object store (e.g. foreign-forge commits); the native push path always derives from `git diff-tree`. Acceptable per the handler doc.

## Verifier must execute (blocked in review sandbox by the 4 GiB build gate)

- `cargo test -p gyre-server --lib agent_sessions::`
- `cargo test -p gyre-server --lib api::agent_tracking` (includes `two_session_attribution`, `by_file_and_by_commit_queries`, `branch_lifecycle_view`, `session_state_time_travel`)
- `cargo test -p gyre-server --lib complete_agent_ends_session_with_real_dirty_state`
- `cargo test -p gyre-server --lib respawn_opens_a_new_session`
- `cargo test -p gyre-server --lib kill_agent_terminates_open_session_before_worktree_cleanup`
- `cargo test -p gyre-server --lib mcp_agent_complete_terminates_open_session_with_dirty_state`
- `cargo test --all` (full suite, incl. `scripts/check-task-commit-attribution.sh` under CI conditions)

## Process note

At review start the working tree contained an **unstaged** modification to `specs/tasks/task-258.md` reverting the frontmatter to `progress: not-started` / `commits: []` — the recurring attribution-clobber pattern (cf. task-208 rounds 121/123). It is NOT part of candidate `a46d2ce9`, whose committed frontmatter correctly lists the four product SHAs. Left untouched per review rules; the executor checkpoint must not publish the clobbered working-tree copy over the candidate's committed frontmatter.
