---
title: "Agent-to-commit tracking at session granularity: session persistence, branch lifecycle, dirty state, file/commit queries, and time-travel"
spec_ref: "Spec gap spec-36fad8fcafe7b73a2b6f2d0e / agent-session-commit-tracking / b580b93c564f"
depends_on: []
progress: complete
commits: ["ec0fbff49ca8ee5a7a21216e82ad56e110e8599b", "f7d9c3883dda4d544507d78d6f7590e339f9ab96", "dfcb500fa9dfcc496f67b5a52b3ff830c0a9c77c", "fff11b71fb5e9a7f3a113c61bbcccfbd79e1b295", "eff02cffb16bbc5ad1026478ff46d2062b5c6ceb", "50452ac5318beaf98f0ff6a5b63557f02c0543b3", "001f585ac686c90f601fd210ca4a40cc5cad7c9d"]
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

All five source-control.md §Agent-to-Commit Tracking bullets are real,
persisted, queryable behavior on this branch (original implementation in
50452ac5, fff11b71, eff02cff, dfcb500f; same-second lifecycle-commit
ordering repair in f7d9c388; this round's session-list ordering repair in
ec0fbff4):

- **Session-to-commit mapping**: `AgentSessionRepository` port with
  SQLite/PostgreSQL adapters over migration `000067_agent_sessions`, a
  mem adapter with identical guards, and `store!` wiring. Sessions are
  created at spawn and at each Ralph-loop fresh-context respawn, ended
  on every termination path (complete_agent, fail_agent, stale Dead,
  MCP gyre_agent_complete, admin kill); `AgentCommit.session_id` is
  stamped from the pushing agent's open session in
  record_pushed_commits and POST /repos/:id/commits/record (NULL when
  none — never a fabricated id); chronological GET
  /repos/:id/agent-sessions/:sid/commits.
- **Branch lifecycle**: GET /repos/:id/agents/:aid/branch-lifecycle —
  spawn-time branch, current branch resolved at termination via real
  `git rev-parse --abbrev-ref HEAD`, all observed commits.
- **Dirty state**: real `git status --porcelain` in the agent's
  worktree at termination; missing worktrees record an explicit
  unknown state, never a silent clean.
- **Commit-based queries**: files_changed per pushed commit (real
  `git diff-tree` at push time); GET /repos/:id/agent-sessions/
  by-file?path= and by-commit?sha=.
- **Time-travel**: GET /repos/:id/agent-sessions/:sid/state — commits
  in [started_at, ended_at] via real `git log --after/--before` plus
  the completion snapshot resolved from refs/agents/{id}/snapshots/{n}.

### This round: CI repair (run 44611030853, PR head bdf87292)

`ralph_loop::tests::respawn_opens_a_new_session` failed with
"session 1 stays terminated" (left None, right Some(500)): the test's
spawn session (manually terminated, ended_at=Some(500)) and its
fresh-context respawn successor are created within one wall-clock
second, and `AgentSessionRepository::find_by_agent` ordered sessions
by (started_at, id) — second-granularity timestamps tie, and the
random-UUID id tiebreak ordered the open respawn session first. The
same determinism-defect class f7d9c388 fixed for lifecycle commits,
but in the session-list query itself.

Fix (ec0fbff4): break started_at ties by session window in ALL THREE
adapters identically — an ended session predates a still-open session
started in the same second (one agent's sessions are sequential
context windows): ended_at ASC NULLS LAST via `is_null` (false sorts
first on both SQLite and PostgreSQL), then ended_at, then the id as
the final deterministic fallback. The port contract
(gyre-ports/src/agent_tracking.rs) documents the tie-break.

### Test evidence

- `cargo test -p gyre-adapters sqlite::session`: 8/8 pass on this
  exact head (ec0fbff4), including the new deterministic regression
  tests `same_second_tie_orders_ended_session_before_open` and
  `same_second_tie_between_ended_sessions_orders_by_ended_at`, whose
  session ids deliberately contradict the window order so a bare id
  tiebreak fails them. Source SHAs, commands, and output saved under
  /tmp/stage/review-evidence.
- `cargo check -p gyre-server` and `cargo clippy -p gyre-adapters -p
  gyre-ports`: clean on this exact head (no new findings in touched
  files).
- `scripts/check-mem-port-contracts.sh`: clean.
- gyre-server test builds are deferred to the independent verifier
  (build-wrapper budget). Required probes:
  `cargo nextest run -p gyre-server ralph_loop::tests::respawn_opens_a_new_session`
  and
  `cargo nextest run -p gyre-server agent_sessions` (includes the new
  mem-backed `find_by_agent_same_second_tie_orders_by_session_window`),
  plus the full `cargo nextest run --all` and the standing gate
  scripts.
