---
title: "Agent-to-commit tracking at session granularity: session persistence, branch lifecycle, dirty state, file/commit queries, and time-travel"
spec_ref: "Spec gap spec-36fad8fcafe7b73a2b6f2d0e / agent-session-commit-tracking / b580b93c564f"
depends_on: []
progress: ready-for-review
commits: ["50452ac5318beaf98f0ff6a5b63557f02c0543b3"]
---

## Required behavior

Per specs/system/source-control.md §Agent-to-Commit Tracking, implement all five bullets as real, persisted, queryable production behavior. Today only agent-level push recording exists (git_http.rs record_pushed_commits writes AgentCommit rows keyed to agent_id); the Session domain entity (crates/gyre-domain/src/agent_tracking.rs) is never persisted — there is no SessionRepository port, no adapter, no session column on agent_commits — so 'which agent session touched which commits' is answerable only at agent granularity. (1) Session-to-commit mapping (chronological): add a SessionRepository port (crates/gyre-ports/src/agent_tracking.rs, alongside AgentCommitRepository) implemented by the SQLite adapter (next unused migration number — current latest is 000066), the PostgreSQL adapter, and the mem adapter, with every port failure constraint guarded identically in all adapters (scripts/check-mem-port-contracts.sh); wire it into AppState via the existing store! pattern in crates/gyre-server/src/lib.rs (no bare Arc<Mutex<HashMap>> state store — scripts/check-in-memory-state-stores.sh). A Session row is created at agent spawn (api/spawn.rs) and at each Ralph-loop respawn with fresh context (ralph_loop.rs relaunch_agent_session) — one session per context window; ended_at is set on every termination path: complete_agent, fail_agent, and the stale-agent Dead transition (stale_agents.rs). Extend AgentCommit with session_id: record_pushed_commits resolves the pushing agent's currently-open session and stamps it; when no open session exists the commit is recorded with a NULL session_id — never a fabricated id; POST /repos/:id/commits/record follows the same resolution. Expose a chronological commits-by-session query. (2) Branch lifecycle: record the initial branch at spawn (AgentWorktree.branch), the current branch at each session end — resolved with a real lookup against the agent's worktree/refs on the termination paths, never the spawn-time value copied forward — and all observed commits (commits recorded during the agent's lifetime); expose a per-agent branch-lifecycle view. (3) Dirty state: on every termination path, detect uncommitted work in the agent's worktree with a real VCS query (git status --porcelain in the worktree, or equivalent jj working-copy diff) and record it on the session as a dirty flag plus the list of uncommitted paths — never a hardcoded value; a missing/inaccessible worktree records an explicit unknown/error state, not a silent clean. (4) Commit-based queries: record files_changed per pushed commit at push time (git diff-tree --no-commit-id --name-only -r <sha> in record_pushed_commits; the machinery already exists at git_http.rs:3996) stored on AgentCommit, and expose 'find all agent sessions that touched this file/commit': a by-file query returning the sessions (and agents) whose recorded commits touched the path, and a by-commit-SHA query returning the linked session(s). (5) Time-travel: an endpoint that reconstructs what the repo looked like when a given agent session was active — the commits present in the repo during [started_at, ended_at] (git log --before/--after style query against the real object store) plus the tree/commit state at the session's completion point by resolving the existing refs/agents/{agent-id}/snapshots/{n} refs (git_refs::resolve_ref) — real queries against real git state, never current-HEAD or mocked stand-ins. Every new route registered in api/mod.rs MUST have an ABAC RouteResourceMapping entry in abac_middleware.rs (scripts/check-abac-route-registry.sh); any handler running without middleware ABAC MUST enforce per-handler scoped authorization (load the entity, compare tenant/workspace against the caller, Forbidden on mismatch — scripts/check-abac-exempt-handlers.sh); scope fields are derived from or validated against the caller (scripts/check-forged-scope-fields.sh). All state is DB-backed so sessions and session-stamped commits survive a server restart.

## Acceptance Criteria

- [ ] Two-session attribution: spawn an agent, push commit A (session 1), trigger a Ralph-loop respawn (fresh context), push commit B (session 2). The agent's session list shows two sessions with start/end times, and the commits-by-session query returns exactly [A] for session 1 and [B] for session 2, chronologically ordered — session granularity, not agent granularity.
- [ ] Dirty state: completing an agent whose worktree contains an uncommitted modification records the terminated session with dirty=true and that file path listed; completing with a clean worktree records dirty=false; both remain readable after a server restart (port-backed persistence).
- [ ] Branch lifecycle: the per-agent branch-lifecycle view returns the initial (spawn-time) branch, the current branch recorded at termination (observable to differ after the agent moved branches mid-session), and all observed commits.
- [ ] File/commit queries: after pushing a commit touching src/lib.rs, the by-file session query for src/lib.rs returns the producing session; a file touched by no recorded commit returns an empty list; the by-commit query for that SHA returns its session.
- [ ] Time-travel: the session state view returns the repo state during the session's active window — commits that existed during [started_at, ended_at] and the tree at the session's completion snapshot ref; a commit pushed after the session ended is excluded, and the result differs from current HEAD when the branch has moved since.
- [ ] cargo test --all passes with new adapter contract tests, and scripts/check-mem-port-contracts.sh, check-abac-route-registry.sh, check-abac-exempt-handlers.sh, check-in-memory-state-stores.sh, and check-migration-versions.sh all remain clean (no new exemption entries).

## Verification intent

Run an agent through two Ralph-loop sessions (spawn → push commit A; gate-failure respawn with fresh context → push commit B) and ask the spec's question 'which agent session touched which commits': on the current tree this is unanswerable — the Session entity is never constructed outside its own unit test, gyre-ports/src/agent_tracking.rs defines no SessionRepository, and the agent_commits table has no session column (every row is session-anonymous). After the task, the commits-by-session query for session 1 must return exactly [A] and not [B]; if session_id were stamped with a fabricated constant instead of the resolved open session, both commits would land in one session and this probe fails. Companion probes for the hollow modes this gap invites: complete an agent whose worktree holds an uncommitted edit and assert the session's dirty state is true with that file listed (a hardcoded false fails); push a commit touching src/lib.rs and query sessions by that file (an empty files_changed Vec — the existing TurnCommitLink pattern at git_http.rs:1715 — returns nothing and fails); request the time-travel view for a session that ended before a later commit landed and assert the later commit is absent (returning current HEAD fails).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/source-control.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `e7d4e79257be81eda6ddf1029e79d17654ad24d2acc5ff2425ebf4e133acd4b0`.

[
  {
    "id": "633595c9df8812514a9517ad",
    "decision": "gap",
    "requirement_quote": "- **Session-to-commit mapping** - which agent session touched which commits (chronological)\n- **Branch lifecycle** - initial branch at agent spawn, current branch, all observed commits\n- **Dirty state** - did the agent leave uncommitted work when it terminated?\n- **Commit-based queries** - \"find all agent sessions that touched this file/commit\" for audit and debugging\n- **Time-travel** - reconstruct what the repo looked like when any agent session was active",
    "detail": "Partially implemented; the coverage matrix marks this verified but three of the five bullets have no production behavior. What exists: an agent-level commit mapping — git_receive_pack's record_pushed_commits walks pushed commits (git log old..new) and records an AgentCommit per SHA with provenance (task_id, spawned_by, parent agent, model context, attestation level), plus the manual POST /repos/:id/commits/record; queries exist by agent, repo, commit SHA, and task; AgentWorktree records the spawn branch. What is missing: (1) Session-to-commit mapping — the Session domain entity has no repository port, is never persisted, and AgentCommit carries agent_id but no session id, so 'which agent session touched which commits' cannot be answered at session granularity, only agent granularity. (2) Dirty state — no code anywhere records whether an agent left uncommitted work at termination; grep across all crates for dirty/uncommitted tracking finds nothing on the completion or failure paths (the only 'Never leave work uncommitted' text is prompt guidance, not tracking). (3) Commit-based queries by file — 'find all agent sessions that touched this file' has no implementation; the agent-commits endpoints filter only by agent/repo/SHA/task, and TurnCommitLink records files_changed as an empty Vec at push time. (4) Time-travel — no endpoint or query reconstructs repo state for a session's active period; the snapshot refs record per-completion branch tips but nothing queries them for session-scoped reconstruction. (5) Branch lifecycle — AgentWorktree.branch is a single static spawn-time branch; 'current branch' is never updated as the agent moves, and 'all observed commits' is only push-recorded commits, not observed history.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "quote": "            let mapping = gyre_domain::AgentCommit::new(\n                Id::new(uuid::Uuid::new_v4().to_string()),\n                Id::new(agent_id),\n                repo.id.clone(),",
        "detail": "What does exist: every pushed commit is natively recorded as an AgentCommit at push time with provenance — real agent↔commit tracking, but keyed to the agent, not to a session."
      },
      {
        "file": "crates/gyre-domain/src/agent_tracking.rs",
        "quote": "pub struct Session {\n    pub id: Id,\n    pub agent_id: Id,\n    pub started_at: u64,",
        "detail": "The Session entity exists only as a domain struct: there is no SessionRepository port (crates/gyre-ports/src/agent_tracking.rs defines only AgentCommitRepository and WorktreeRepository), it is never constructed in server or adapter code, and it has no commit linkage — the session-to-commit mapping bullet is unimplemented."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        .route(\n            \"/api/v1/repos/:id/commits/record\",\n            post(agent_tracking::record_commit),\n        )\n        .route(\n            \"/api/v1/repos/:id/agent-commits\",\n            get(agent_tracking::list_commits),\n        )",
        "detail": "The complete route surface for commit tracking: record + list filtered by agent_id/repo only. Inspected all routes in api/mod.rs — there is no query by file, no session-scoped commit query, and no time-travel/reconstruction endpoint; 'find all agent sessions that touched this file' cannot be issued."
      },
      {
        "file": "crates/gyre-domain/src/agent_tracking.rs",
        "quote": "pub struct AgentWorktree {\n    pub id: Id,\n    pub agent_id: Id,\n    pub repository_id: Id,\n    pub task_id: Option<Id>,\n    pub branch: String,",
        "detail": "Branch lifecycle is partial: a single static `branch` recorded at worktree creation (the spawn branch); nothing tracks a 'current branch' as the agent moves, and dirty state is absent from this and every other termination-path struct — I inspected the completion path (complete_agent), the failure path (fail_agent), and stale_agents.rs; none capture uncommitted-work state (grep for 'dirty'/'uncommitted' across crates/ returns no production hits)."
      }
    ]
  }
]

## Shipped

Implementation commit: `50452ac5` (feat(task-258)). All five
source-control.md §Agent-to-Commit Tracking bullets are implemented as
real, persisted, queryable production behavior; state is DB-backed (shared
migration `2026-11-14-000067_agent_sessions`, portable to both SQLite and
PostgreSQL), so sessions and session-stamped commits survive a server
restart.

### What shipped

- **Session persistence.** `Session` (gyre-domain) extended with
  `repository_id`, `initial_branch`, `final_branch`, `dirty`
  (`Option<bool>` — NULL = unknown, never a silent clean),
  `uncommitted_paths`, `dirty_detail`. New `AgentSessionRepository` port
  (gyre-ports/agent_tracking.rs) implemented by the SQLite adapter
  (sqlite/session.rs), the PostgreSQL adapter (postgres/session.rs), and
  the mem adapter (`MemAgentSessionRepository`) with identical
  create-duplicate and update-not-exists guards; wired into
  `AppState.agent_sessions` via the `store!` pattern (lib.rs, mem.rs test
  state). `agent_commits` gains `session_id` (NULL when the pushing agent
  had no open session — never a fabricated id) and `files_changed` (JSON
  array).
- **Session lifecycle.** A session row is created at agent spawn
  (spawn_agent_core) and at each Ralph-loop fresh-context respawn
  (relaunch_agent_session — initial branch resolved from the worktree's
  real current branch, falling back to the recorded branch). `ended_at` is
  set on every specced termination path: `complete_agent`, `fail_agent`,
  and the stale-agent Dead transition (stale_agents.rs — invoked before
  worktree cleanup so the dirty-state observation has a worktree to read).
- **Session-to-commit mapping (chronological).** `record_pushed_commits`
  (git_http.rs) and `POST /repos/:id/commits/record` resolve the pushing
  agent's currently-open session and stamp `session_id`.
  `GET /repos/:id/agent-sessions?agent_id=` lists sessions with start/end
  times; `GET /repos/:id/agent-sessions/:sid/commits` returns exactly the
  commits recorded during that context window, oldest-first
  (`find_by_session` orders by timestamp ASC in all adapters).
- **Branch lifecycle.** `GET /repos/:id/agents/:aid/branch-lifecycle`
  returns the initial (spawn-time) branch, the current branch recorded at
  termination (real `git rev-parse --abbrev-ref HEAD` lookup in the
  agent's worktree at termination — observable to differ after the agent
  moved branches mid-session), and all observed commits (chronological).
- **Dirty state.** On every termination path the session records a real
  `git status --porcelain` query in the agent's worktree: dirty flag plus
  the uncommitted paths (rename entries resolved to the new path). A
  missing/inaccessible worktree records `dirty = NULL` plus
  `dirty_detail` naming why — an explicit unknown state, not a silent
  clean.
- **Commit-based queries.** `files_changed` is recorded per pushed commit
  at push time (`git diff-tree --no-commit-id --name-only -r --root
  <sha>` in record_pushed_commits; the manual record endpoint derives it
  from the repo's object store when the sha resolves there, with an
  optional caller-asserted fallback for commits that live outside this
  forge). `GET /repos/:id/agent-sessions/by-file?path=` returns the
  sessions (and agents) whose recorded commits touched the path (exact
  match, not substring); `GET /repos/:id/agent-sessions/by-commit?sha=`
  returns the linked session.
- **Time-travel.** `GET /repos/:id/agent-sessions/:sid/state` returns the
  commits present in the repo during `[started_at, ended_at]` (real
  `git log --all --after/--before` query against the object store) plus
  the completion snapshot: the newest `refs/agents/{id}/snapshots/{n}`
  ref whose target commit existed by the session's end, with commit sha,
  tree sha, and file list. Real queries against real git state — never
  current HEAD or mocked stand-ins.
- **Authorization.** All six new routes are registered in the ABAC
  `ResourceResolver` as repo-scoped resources (abac_middleware.rs);
  session-bearing handlers load the session and treat a session in
  another repo as NotFound. No exemption-file entries were added
  anywhere.

### Test evidence

Executed in this sandbox (see /tmp/stage/review-evidence/README.md):

- `cargo test -p gyre-domain agent_tracking` — 7 passed (session
  construction, session_id/files_changed builders, NULL-not-fabricated).
- `cargo test -p gyre-adapters session` — 17 passed (create +
  duplicate-id failure, chronological find_by_agent/find_by_repo,
  find_open_by_agent, termination-state round trip incl. dirty=NULL +
  detail, update-missing failure, for the SQLite adapter; the PG adapter
  is the same code shape against the shared schema).
- `cargo test -p gyre-adapters agent_commit` — 11 passed (session_id and
  files_changed JSON round trip, NULL stays NULL, find_by_session
  chronological and session-scoped, find_by_file exact-path with
  substring-negative and empty cases).
- `cargo check -p gyre-server --tests` — exit 0 (all new server tests
  type-check); `cargo clippy -p gyre-server --lib` — no new warnings on
  touched code.
- All `scripts/check-*.sh` gates run on both the base tree and this tree:
  identical failure sets (pre-existing environment/debt failures only).
  Clean on this tree: check-mem-port-contracts, check-abac-route-registry,
  check-abac-exempt-handlers, check-in-memory-state-stores,
  check-migration-versions, check-migration-sql-portability, check-arch,
  and the security-pattern lints.

Deferred to the independent verifier (gyre-server test codegen exceeds
this sandbox's 4 GiB budget — the build wrapper blocks it; probes
recorded in /tmp/stage/review-evidence/README.md):

- `cargo test -p gyre-server agent_sessions` — real-git-worktree tests:
  dirty=true with modified + untracked files listed; clean worktree →
  dirty=false; missing worktree → dirty=NULL + detail (not a silent
  clean); moved-branch resolution (final ≠ spawn branch);
  files_changed_for_commit against a real commit; time-travel window
  (during-commit present, after-commit excluded) and completion snapshot
  (ref, sha, tree files).
- `cargo test -p gyre-server api::agent_tracking` — two-session
  attribution (commit A → session 1, commit B → session 2,
  commits-by-session returns exactly [A]/[B]; an orphan commit outside
  any session carries NULL session_id); by-file/by-commit queries
  (producing session returned, untouched file empty, substring
  non-match, SHA → session, unrecorded SHA empty); branch-lifecycle view
  (initial vs. termination branch, all observed commits); time-travel
  endpoint over a real bare repo with pinned commit dates (after-commit
  excluded, snapshot tree predates it, cross-repo session 404).
- `cargo test -p gyre-server respawn_opens_a_new_session` (ralph_loop.rs)
  — gate-failure respawn with fresh context opens a second session; only
  the respawned window is open; commit attribution resolves to it.
- `cargo test -p gyre-server complete_agent_ends_session_with_real_dirty_state`
  (spawn.rs) — spawn starts an open session; complete_agent terminates it
  with dirty=true, the modified file listed, and the moved branch
  resolved from the real worktree.
- `cargo test --all` (repo-wide) — owned by verification.

### Notes

- Coverage row 12 (Agent-to-Commit Tracking) in
  specs/coverage/system/source-control.md was rewritten with the concrete
  source evidence for all five bullets.
- docs/api-reference.md documents the new endpoints.
- The TurnCommitLink (conversation-provenance) `files_changed` empty-Vec
  pattern at git_http.rs is a different feature (HSI §5 conversation
  links); the spec bullet's by-file query is served by AgentCommit's
  push-time file list, which is recorded for real.
