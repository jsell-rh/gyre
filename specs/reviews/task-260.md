# Review — task-260 (Publish reviewed spec backlog for specs/system/source-control.md)

Candidate: `f0179d525c317445fd8f2ea6b0a539ae258444f0` (single commit on assigned base `321615eced877c4839ccb98b78fae10778559933`).
Plan: `e7d4e79257be81eda6ddf1029e79d17654ad24d2acc5ff2425ebf4e133acd4b0`. Verdict: **approved** — publication-only change, published exactly the reviewed plan, no product surface touched.

## Scope of the diff (additive, 4 files, 634 insertions)

- `specs/tasks/task-258.md` — agent-session-commit-tracking, `progress: not-started`, `depends_on: []`
- `specs/tasks/task-259.md` — mirror-namespace-isolation, `progress: not-started`, `depends_on: []`
- `specs/tasks/task-260.md` — the publication task itself, frontmatter matching the assignment exactly (`ready-for-review`), plan JSON embedded
- `specs/task-spec-inputs.json` — two `backlog/{section-id}` section entries + two `Spec gap spec-36fad8fcafe7b73a2b6f2d0e / {key} / b580b93c564f` bindings

No Rust, web, script, spec, coverage-matrix, or gate file was modified. Coverage rows for the two gap sections deliberately remain as-is (per "Task creation is not implementation" and the standard appended guard paragraph).

## Probes run (evidence: /tmp/stage/review-evidence/)

- **Plan fidelity (programmatic):** task-260's embedded JSON is byte-identical to the assigned plan; task-258/259 carry the plan's title, required_behavior, acceptance criteria, negative_probe (verification intent), and empty dependencies verbatim. The only delta is the standard "Update the relevant coverage rows only after implementing…" paragraph — the same convention used by every task published under the approved task-255 (message-bus) precedent (tasks 247–254).
- **Pipeline identity derivation:** recomputed with the repo's own `scripts/pipeline/requirements.py` digest — `spec-36fad8fcafe7b73a2b6f2d0e` = `digest("specs/system/source-control.md")[:24]`; all 14 audit section IDs (including both gap IDs `633595c9df8812514a9517ad` → Agent-to-Commit Tracking and `09c28182c53ccd934aa3b93a` → External Repository Mirroring) match their canonical headings; the spec_ref suffix `b580b93c564f` matches the plan's audit requirements hash. The `Spec gap {spec-id} / {key} / {hash12}` binding keys follow the message-bus precedent exactly.
- **Audit challenged against the current tree:** every requirement_quote was located in its assigned spec section (14/14), and every evidence quote exists verbatim in its cited production file (0 misses). The two gap claims were re-verified directly in code: `create_mirror_repo` (repos.rs:639) uses `create_repo`'s exact path formula (repos.rs:176) — no `mirrors/` namespace anywhere; `gyre-ports/src/agent_tracking.rs` defines only `AgentCommitRepository`/`WorktreeRepository`; `AgentCommit` has no session_id; the route surface is only `/commits/record` + `/agent-commits` (no by-file, session, or time-travel queries). Task-258's factual anchors are accurate: latest migration is 000066; the diff-tree machinery exists at git_http.rs:3996; `TurnCommitLink.files_changed` is an empty Vec at :1715; `relaunch_agent_session` exists in ralph_loop.rs.
- **Duplicate work:** no other task binds `spec-36fad8…` or either backlog section; no existing task covers session-granularity commit tracking or mirror namespace isolation.
- **Registry validity:** `task-spec-inputs.json` parses; new entries structurally match the existing backlog-entry and binding shapes; `python3 scripts/test_pipeline_requirements.py` → 23/23 OK.

## Decomposition / dependency assessment

Two tasks, each owning exactly one gap, no overlap, no dependencies — correct: agent-session tracking (server tracking surface) and mirror namespace isolation (repos.rs storage paths) are independent work streams. Both stay `not-started`, satisfying "new tasks remain not-started until this backlog publication merges."

## Observations (non-blocking, recorded for task-258's implementer)

- The plan directs adding a `SessionRepository` in `gyre-ports/src/agent_tracking.rs`; `gyre-ports` already exports an auth-session `SessionRepository` (mem.rs `MemSessionRepository`, `AppState.sessions`). The implementer must namespace the new trait (module-qualified path or re-export alias) — an implementation detail, not a publication defect; the cited file placement is unambiguous.
- The audit's §6 speculative-merge wrinkle (speculation targets hardcoded `main` rather than each MR's target branch) is disclosed in the audit itself and the classification (satisfied) is defensible since that section's text does not pin the speculation target; no missing task results.
- task-256/257 do not exist as files — names 258/259/260 were assigned by the controller's ledger; no collision exists in the repo.

## Verifier notes

No Rust/web runtime probes were warranted: the candidate changes no compiled or executed surface. Standard gates (including `check-task-commit-attribution.sh`, which needs full git history) should run as usual; the publication commit is `chore:`-labeled docs-only, consistent with the task-255 precedent (`commits: []` on the publication task).
