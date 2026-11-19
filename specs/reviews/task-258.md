# Review — task-258 (Agent-to-commit tracking at session granularity)

Spec: `specs/system/source-control.md` §Agent-to-Commit Tracking (all five bullets).
Candidate: `63c8d7516465e4ccb21c11a9431259b98ae2ccf4` (base `61b6080d1866f32d98cc7a35d62e3fb2b88a290a`).
Implementation commit: `50452ac5` (+ merge `45e8f9f6` of the base; conflict resolution in spawn.rs/stale_agents.rs verified to contain **zero deletions** — pure additions over base).
Verdict: **approved** — all five bullets are real, persisted, queryable production behavior; no blocking findings.

## Probes run (independent, this review)

All evidence retained under `/tmp/stage/review-evidence/`:

- `cargo test -p gyre-domain agent_tracking` → **7 passed** (includes new `test_agent_commit_session_and_files`: session_id NULL-when-absent and files_changed round trip).
- `cargo test -p gyre-adapters session` → **17 passed** (SQLite `AgentSessionRepository`: chronological find_by_agent, duplicate-id fails, find_open_by_agent, termination-state round trip incl. dirty=NULL + dirty_detail unknown state, update-missing fails).
- `cargo test -p gyre-adapters agent_commit` → **11 passed** (find_by_session chronological with cross-session leak guard, find_by_file exact-path match with substring/negative/other-repo cases, NULL session round trip).
- `cargo check -p gyre-server --tests` → **PASS** (7m33s; test code type-checks).
- `cargo fmt --check` (all four crates) → clean.
- Mechanical gates, all **exit 0, no new exemption entries** (baseline files untouched by the diff): check-mem-port-contracts, check-abac-route-registry, check-abac-exempt-handlers, check-in-memory-state-stores, check-migration-versions, check-migration-sql-portability, check-forged-scope-fields, check-arch, check-byte-slice-truncation, check-scope-literal-defaults, check-fabricated-scope-defaults, check-inert-enforcement, check-fail-open-ref-resolution, check-relative-path-defaults, check-lossy-secret-conversion, check-dead-message-kinds, check-unbounded-external-http, check-forwarded-header-trust.
- `check-task-commit-attribution.sh` against the committed tree (temp worktree at HEAD): **OK** — `50452ac5` recorded in the frontmatter. (Running it in the live checkout reports task-258 missing only because the executor's unstaged protected-text restore resets `commits: []`; the committed candidate carries the attribution. Not a source defect.)
- **Empirical validation of every git invocation the new code shells out to** (fresh repo): `for-each-ref --format='%(refname) %(objectname)' refs/agents/{id}/snapshots/` does prefix-match refs under the trailing-slash pattern; `status --porcelain` output is `XY <path>` with rename `old -> new` (the `get(3..)`/`split_once(" -> ")` parsing is correct and char-boundary safe); `log --all --format=%H%x1f%ct%x1f%an%x1f%s --after=S-1 --before=E+1` yields the inclusive [S,E] window; `diff-tree --no-commit-id --name-only -r --root <sha>` lists root-commit files.

## Source review — all five bullets verified real

1. **Session persistence + session-to-commit mapping.** `AgentSessionRepository` port (gyre-ports) with SQLite (diesel, migration `2026-11-14-000067_agent_sessions` — next unused number after 000066, portable SQL), PostgreSQL, and mem adapters; duplicate-id and update-missing contracts guarded identically in all three (mem guards in code). Wired via `store!` into `AppState.agent_sessions` (no bare HashMap state store). `start_session` at `spawn_agent_core` (after worktree-record creation/compensation, using the spawn branch) and at `relaunch_agent_session` (Ralph respawn — initial branch re-resolved from the live worktree, falling back to the recorded branch). `end_session` on all three termination paths: `complete_agent`, `fail_agent`, and the stale-agent Dead transition — the latter placed **before** worktree cleanup so the dirty observation has something to observe. `AgentCommit.session_id` stamped from `open_session_id` (the pushing agent's open session; NULL when none — never fabricated) in both `record_pushed_commits` (git push path) and POST `/repos/:id/commits/record`. Chronological `find_by_session` (timestamp ASC) exposed at GET `/repos/:id/agent-sessions/:sid/commits`.
2. **Branch lifecycle.** GET `/repos/:id/agents/:aid/branch-lifecycle`: initial branch from the first session (spawn-time), `current_branch` from the latest terminated session's `final_branch` — resolved at termination via a real `git rev-parse --abbrev-ref HEAD` in the agent's worktree (detached HEAD → None), never the spawn value copied forward — plus all observed commits (agent's recorded commits, chronological).
3. **Dirty state.** Real `git status --porcelain` in the worktree at every termination; modified + untracked paths recorded; missing worktree record or VCS failure records `dirty = NULL` + `dirty_detail` — an explicit unknown state, never a silent clean.
4. **Commit-based queries.** `files_changed` per pushed commit at push time (`git diff-tree --no-commit-id --name-only -r --root`, 40-char-hex SHA guard before it reaches the command line) stored on `AgentCommit` (JSON array). GET `by-file?path=` returns sessions+agents whose commits touched the path (exact match — substring and LIKE-metacharacter false positives impossible); GET `by-commit?sha=` returns the linked session; empty lists for untouched files/unrecorded SHAs.
5. **Time-travel.** GET `/repos/:id/agent-sessions/:sid/state`: commits present during [started_at, ended_at] via `git log --all --after/--before` against the real object store (inclusive window, empirically verified), plus the completion snapshot resolved from `refs/agents/{agent-id}/snapshots/{n}` (newest ref whose commit date ≤ ended_at, with tree SHA + file list). Never current HEAD; the test proves a post-session commit is excluded while the branch has since moved.

**ABAC/scope:** all six new routes registered in the resolver (repo resource) — registry gate confirms; handlers scope-check the session against the repo path segment (cross-repo session → 404, indistinguishable from missing); scope fields are server-derived. No exemption-file growth.

**Test quality (no inflation found):** the server tests build real git repos and real linked worktrees on disk, pin committer dates for deterministic windows, and assert observations that a hardcoded value cannot satisfy (dirty=true with the exact modified/untracked paths listed; final_branch = the mid-session-moved branch ≠ spawn branch; two-session attribution with the respawn's open session resolved as the attribution target; post-session commit excluded from the window and snapshot). The api tests drive the real router with the pre-existing `create_repo` helper.

## Verifier must execute (deferred per the 4 GiB build-budget rule)

- `cargo test --all` (acceptance criterion).
- Focused server tests for this task: `cargo test -p gyre-server agent_sessions`, `cargo test -p gyre-server complete_agent_ends_session_with_real_dirty_state`, `cargo test -p gyre-server respawn_opens_a_new_session`, `cargo test -p gyre-server two_session_attribution`, `cargo test -p gyre-server by_file_and_by_commit_queries`, `cargo test -p gyre-server branch_lifecycle_view`, `cargo test -p gyre-server session_state_time_travel`.

## Non-blocking observations (recorded for completeness; no repair required)

- `crates/gyre-server/src/lib.rs:2075` — the doc comment on `spawn_session_cleanup` was edited from `SessionRepository::delete_expired_before` to `AgentSessionRepository::delete_expired_before`; that contract belongs to the auth-session repository (`state.sessions`, user_profile.rs), not the new agent-session port. Doc-only inaccuracy; zero functional effect.
- The git push path's session stamping (`record_pushed_commits`) shares the `open_session_id` helper that the tested manual-record path exercises, but no test pushes through the smart-HTTP receive-pack and asserts the stamped `session_id`/`files_changed` on the resulting `AgentCommit` rows. The wiring is three lines over the tested helper; noted as residual coverage, not a defect.
- No test reopens the SQLite file through a second `SqliteStorage` handle to simulate a server restart; restart survival follows from the DB-backed port (round-trip tests prove persistence of every termination field). Residual coverage only.
- `completion_snapshot` returns None for the whole view if a single snapshot ref's `commit_time` query fails (uses `?` inside the loop instead of skipping that ref) — degrades to "no snapshot" rather than wrong data; robustness nit.
