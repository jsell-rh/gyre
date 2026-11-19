---
title: "Publish reviewed spec backlog for specs/system/source-control.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/source-control.md",
  "plan_id": "e7d4e79257be81eda6ddf1029e79d17654ad24d2acc5ff2425ebf4e133acd4b0",
  "audit": {
    "base": "fb1424fbac55cd23cb08606f4cd667a96f4dbc43",
    "requirements": "b580b93c564ff1f8230dd6ee2fa075f869897f82962869d505c0f0b25d1fb5b5",
    "sections": [
      {
        "id": "4bd22d2e68aefe0e2d3a177b",
        "decision": "context",
        "requirement_quote": "# Source Control",
        "detail": "Document title heading only. It carries no requirement text of its own; the normative content lives in the child sections (Core Decision through External Repository Mirroring), each of which is accounted for individually below. No separate implementable requirement exists at this level.",
        "evidence": []
      },
      {
        "id": "ab8bb93c3bfaa783f9beb62f",
        "decision": "satisfied",
        "requirement_quote": "Gyre **is the source control host** - not an integration with an external forge.",
        "detail": "The spec requires: (1) git as storage format and wire protocol with the forge speaking git smart HTTP; (2) jj as the agent's version-control interface; (3) jj translating to git on push so the forge sees standard git commits; (4) owned SCM enabling protocol-level interaction, auditing of every push/merge, and inline policy enforcement. All four are real production behavior. (1) git_http.rs implements the full smart HTTP surface: git_info_refs, git_upload_pack, and git_receive_pack are registered handlers on /git/:ws/:repo/*; receive-pack enforces mirror read-only, archived read-only, repo Write permission, and repo-level ABAC before processing the packfile, and records GitPushAccepted/GitPushRejected audit events. (2) Every spawned agent is provisioned a per-agent jj workspace of the repo's shared jj checkout (spawn.rs), and jj operations are exposed through api/jj.rs over the JjOpsPort. (3) On completion, jj_bookmark_set points the source-branch bookmark at '@' and jj_git_export pushes the jj work into the backing bare git repo, after which the MR's diff/conflict checks run against plain git refs. (4) Push-time auditing and inline enforcement are real (audit events and check_pre_accept_gates which undoes ref updates on gate failure); merge-time policy enforcement runs in the merge processor (spec policies, ABAC attestation decisions, supervised-trust holds).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "pub async fn git_receive_pack(",
            "detail": "The forge's own git smart HTTP receive-pack handler (alongside git_info_refs at line 266 and git_upload_pack at line 537) — Gyre hosts the git wire protocol itself rather than delegating to an external forge."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "    let event = AuditEvent::new(\n        Id::new(uuid::Uuid::new_v4().to_string()),\n        AuditEventType::GitPushAccepted,",
            "detail": "Every accepted push is recorded as a GitPushAccepted audit event (GitPushRejected on the rejection path at line 437), satisfying 'audit every push/merge'."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "    let jj_main_checkout_path = format!(\"{}/jj-main\", repo_root);\n    let jj_workspace_path = format!(\"{}/workspaces/{}\", repo_root, agent.id);",
            "detail": "Agent spawn provisions a shared jj main checkout plus a per-agent jj workspace backed by the repo's bare git repo — jj is the agent's interface to version control, as the spec requires."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "                .jj_bookmark_set(&wt.path, &mr.source_branch, \"@\")",
            "detail": "On agent completion the source-branch bookmark is moved onto the agent's working change and (next statement) jj_git_export pushes it into the bare git repo — jj translates to git on push; the forge only ever sees conventional git refs."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "async fn check_pre_accept_gates(",
            "detail": "Inline policy enforcement on the push path: after git accepts the packfile, configured push gates (including conventional-commit) are evaluated; on failure ref updates are undone and the push is rejected — enforcement, not audit-only."
          }
        ]
      },
      {
        "id": "07fe6015ea2e18a7cea6d287",
        "decision": "context",
        "requirement_quote": "Git was designed for humans who commit intentionally, name branches deliberately, and resolve conflicts interactively. Agents don't work like that. jj solves the specific problems agents face:",
        "detail": "Pure rationale introduction. It motivates the seven numbered sub-sections that follow and states no requirement of its own; each numbered sub-section is audited individually below. Classified context, not a normative heading.",
        "evidence": []
      },
      {
        "id": "e057e49fc78aa86d91da73fb",
        "decision": "satisfied",
        "requirement_quote": "When an agent runs a tool (edit file, run command), jj automatically creates a new change. No explicit `git add && git commit` needed.",
        "detail": "The platform-side requirement is that agents work in jj where every working-copy mutation is captured as an atomic change without explicit git add/commit. Spawn provisions a real per-agent jj workspace and immediately creates a described change on it (jj_new), so the agent's working copy IS a jj change that jj auto-snapshots on every invocation — the atomic-change granularity is jj's native behavior in the provisioned workspace, which the platform wires into the spawn path (with a plain-git worktree only as a degraded fallback). The JjOpsAdapter shells out to the real jj binary (jj 0.39.0), verified by recorded-fixture tests that assert the created change id and history.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "                        match state.jj_ops.jj_new(&jj_workspace_path, &description).await {",
            "detail": "Agent spawn creates the initial described change in the agent's jj workspace — the working copy the agent edits is a jj change from the start, so subsequent tool executions are captured as jj working-copy snapshots."
          },
          {
            "file": "crates/gyre-adapters/src/jj_ops.rs",
            "quote": "    async fn jj_new(&self, repo_path: &str, description: &str) -> Result<String> {\n        self.run_jj(repo_path, &[\"new\", \"-m\", description]).await?;",
            "detail": "The adapter runs the real jj CLI (path configurable via GYRE_JJ_PATH); jj's auto-snapshotting of the working copy on every command is what makes each tool execution an atomic change — the platform provides the real jj substrate rather than a stand-in."
          }
        ]
      },
      {
        "id": "f8efc9c9e8081ace9bec3ad5",
        "decision": "satisfied",
        "requirement_quote": "jj records every operation (commit, rebase, squash) in an undo log. If an agent crashes mid-task, the next session can `jj undo` back to a known-good state.",
        "detail": "The operation log lives in the provisioned jj workspace (each agent's workspace at {repo}/workspaces/{agent-id} is a real jj workspace whose .jj directory holds the operation log), so a crashed agent's next session — which the platform re-points at the same workspace — can undo via the real jj CLI. The platform also exposes jj undo over the API: the JjOpsPort has jj_undo and jj_log (operation/change history), the adapter runs the real top-level `jj undo`, and POST /api/v1/repos/:id/jj/undo is a registered route. Additionally, session completion points are recorded as git refs (refs/agents/{id}/snapshots/{n}), giving crash recovery a known-good ref target. (Note: the HTTP jj endpoints operate on the repo's root path, which serves the colocated layout; the workspace-layout agent recovers locally in its own workspace where the operation log actually resides.)",
        "evidence": [
          {
            "file": "crates/gyre-adapters/src/jj_ops.rs",
            "quote": "        self.run_jj(repo_path, &[\"undo\"]).await?;",
            "detail": "Real jj undo against the operation log (the adapter documents that top-level `jj undo` is the correct jj 0.39.0 invocation); a recorded-fixture test asserts @ returns to the pre-operation change."
          },
          {
            "file": "crates/gyre-server/src/api/jj.rs",
            "quote": "pub async fn jj_undo(",
            "detail": "POST /api/v1/repos/:id/jj/undo is a registered production route wired to state.jj_ops.jj_undo — the undo capability is exposed on the platform surface, not just in theory."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "        let snap_ref = format!(\"refs/agents/{}/snapshots/{}\", agent.id, n);",
            "detail": "Each session's completion point is persisted as a git ref (refs/agents/{id}/snapshots/{n}), giving the 'known-good state' the spec's crash-recovery story relies on in addition to jj's own operation log."
          }
        ]
      },
      {
        "id": "3d9a1d447ae8bf6219383af3",
        "decision": "satisfied",
        "requirement_quote": "Agents work without branch-naming overhead, then create a bookmark (jj's branch pointer) only when ready to push.",
        "detail": "Agents work on anonymous jj changes in their workspace (spawn creates a described change with no branch; the working-copy change '@' is a plain DAG node), and the bookmark is created only at completion/MR time: the completion path calls jj_bookmark_set(source_branch, \"@\") and jj_git_export — the branch pointer is materialized exactly when the agent's work is ready to become an MR. The JjOpsPort provides jj_bookmark_create/jj_bookmark_set and the adapter shells to the real `jj bookmark` subcommands.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "                .jj_bookmark_set(&wt.path, &mr.source_branch, \"@\")",
            "detail": "The bookmark on the MR's source branch is pointed at the agent's anonymous working change only at completion (MR creation) time — matching 'create a bookmark only when ready to push'. Before this, the agent's work lives in unbookmarked jj changes."
          },
          {
            "file": "crates/gyre-adapters/src/jj_ops.rs",
            "quote": "    async fn jj_new(&self, repo_path: &str, description: &str) -> Result<String> {\n        self.run_jj(repo_path, &[\"new\", \"-m\", description]).await?;",
            "detail": "jj new creates a new working-copy change with no branch — the agent's day-to-day changes are anonymous DAG nodes (bookmarks: vec![] in JjChange), eliminating branch-naming overhead during work."
          }
        ]
      },
      {
        "id": "26d2cabbf85d373e0cbe8798",
        "decision": "satisfied",
        "requirement_quote": "When the target branch moves (because another agent's MR merged), jj automatically rebases the agent's in-progress work.",
        "detail": "Implemented as the last step of every successful merge (task-106): rebase_inflight_agents identifies in-flight agents — authors of other Open/Approved MRs targeting the moved branch in the same repo, still Active, with an existing worktree in that repo — and runs jj rebase -b @ -d <target_branch> on each agent's worktree, rebasing the whole in-flight stack. The adapter imports git refs first (the moved branch tip is invisible to jj otherwise), and the outcome is surfaced with backoff/rate-limiting (one rebase per agent per 60s window, deferred movements batched into a pending marker replayed by the processor), audit events for every rebase, and a baseline_moved notification to the rebased agent. Infrastructure failures do not consume the backoff window or a pending deferral.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "            rebase_inflight_agents(state, &repo, target_branch, last_sha, &merged_authors).await;",
            "detail": "Called as the last step after a successful merge lands on target_branch (and again on the single-entry merge path at line 2937) — the trigger is exactly the spec's 'target branch moves because another agent's MR merged'."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "    let outcome = state\n        .jj_ops\n        .jj_rebase(&agent.worktree_path, \"@\", target_branch)\n        .await;",
            "detail": "The rebase runs on the in-flight agent's own worktree, rebasing the branch containing the working copy (the whole in-flight stack) onto the branch that just moved — not onto an unrelated default branch."
          },
          {
            "file": "crates/gyre-adapters/src/jj_ops.rs",
            "quote": "                &[\"rebase\", \"-b\", revision, \"-d\", destination],",
            "detail": "Real `jj rebase -b <rev> -d <dest>` via the jj CLI, preceded by `jj git import` so the moved git ref is visible — a real rebase of the agent's in-flight work, not a notification-only stand-in."
          }
        ]
      },
      {
        "id": "0760b508156dff592561efe6",
        "decision": "satisfied",
        "requirement_quote": "In jj, conflicts are materialized in the working copy as regular content - the agent can keep working on other files and resolve conflicts when ready.",
        "detail": "Conflicts are treated as state, not errors, on the automatic-rebase path: jj_rebase returns JjRebaseOutcome::Conflict (exit-0 rebase with conflicts materialized as jj conflict state, detected via `jj resolve --list` exit-code contract) carrying the conflicted file list; the run_rebase handler flags the author's MR (has_conflicts = Some(true)), records an audit event, and emits SpeculativeConflict + Escalation events telling the agent which files conflict and the new base — the workspace is left usable so the agent keeps working on non-conflicting files. A later clean rebase clears the flag. The merge processor likewise treats MergeResult::Conflict as a queue-entry failure state rather than crashing, and the speculative merge job classifies conflicts (order-dependent vs order-independent) as stored state.",
        "evidence": [
          {
            "file": "crates/gyre-ports/src/jj_ops.rs",
            "quote": "    /// Rebase completed with conflicts materialized as jj conflict state.\n    /// `files` lists the conflicted paths.\n    Conflict {",
            "detail": "The port models a conflicted rebase as a successful outcome carrying conflicted paths — jj's conflict-as-state model, not an error that blocks work."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                mr.has_conflicts = Some(true);",
            "detail": "On a conflicted rebase the MR is flagged and SpeculativeConflict/Escalation events are emitted (lines 375-401) with the conflicting files and new base — the agent is told about the conflict and left to keep working, exactly the spec's non-blocking conflict state."
          }
        ]
      },
      {
        "id": "2230f5490145f7800c08c02b",
        "decision": "satisfied",
        "requirement_quote": "The forge's speculative merge feature (try-merge all in-flight branches) maps naturally to jj's model.",
        "detail": "The speculative merge feature is real production code: a background job (registered in the job framework, 60s interval) collects every active agent's in-flight branch, builds the dependency graph from their MRs (speculating in dependency order, atomic groups as a unit), and try-merges each branch via git_ops.can_merge — a real in-memory git merge (git2 merge_commits, no working tree modified). Clean results emit SpeculativeMergeClean; conflicts emit SpeculativeConflict and are classified order-dependent vs order-independent by prior speculated-clean branches. Results are queryable via GET /api/v1/repos/:id/speculative. One fidelity wrinkle, not a contract violation: the job speculates against a hardcoded \"main\" rather than each MR's target branch, so predictions are wrong for MRs targeting non-default branches (the rebase path in merge_processor.rs explicitly handles non-default targets). The spec text here does not pin the speculation target, and the try-merge-all-in-flight-branches behavior itself is implemented.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/speculative_merge.rs",
            "quote": "    let can_merge = state.git_ops.can_merge(repo_path, branch, \"main\").await;",
            "detail": "Each in-flight branch is try-merged via the real git merge check — the speculative merge feature the section is about exists and runs over all active agents' branches every 60 seconds."
          },
          {
            "file": "crates/gyre-adapters/src/git2_ops.rs",
            "quote": "            // merge_commits performs an in-memory merge; no working tree is modified\n            let merge_index = repo\n                .merge_commits(&target_commit, &source_commit, None)",
            "detail": "The speculation is a real three-way merge against actual commit trees (git2), not a structural or heuristic stand-in — a genuine try-merge."
          },
          {
            "file": "crates/gyre-server/src/speculative_merge.rs",
            "quote": "                    gyre_common::message::MessageKind::SpeculativeMergeClean,",
            "detail": "Clean speculations emit SpeculativeMergeClean (SpeculativeConflict on the conflict path at line 397) to the workspace — the results are surfaced as platform events, with conflict classification (ConflictType::OrderIndependent/OrderDependent) stored in the result."
          }
        ]
      },
      {
        "id": "f62c1815821b8a7e20d37f02",
        "decision": "satisfied",
        "requirement_quote": "The ref `refs/tasks/{task-id}` marks the task's branch, and `refs/agents/{agent-id}/snapshots/{n}` records each session's completion point.",
        "detail": "The concrete platform requirements are implemented exactly: on spawn, refs/agents/{agent-id}/head and refs/tasks/{task-id} are written to the repo (best-effort after worktree provisioning); on every agent completion, refs/agents/{agent-id}/snapshots/{n} is written where n = count_refs_under the snapshot prefix, recording that session's completion point (the source branch's resolved SHA). The jj-side rollback capability is available both locally (the workspace is a real jj workspace with its operation log) and via the platform's jj undo port/endpoint. One noted limitation: the exposed port undoes only the last operation (top-level `jj undo`), not an arbitrary historical operation — but the snapshot refs provide the recorded per-session completion points the section mandates, and agents run the full jj CLI in their workspace where `jj op log`/restore is available.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "            let agent_ref = format!(\"refs/agents/{}/head\", agent.id);\n            let task_ref = format!(\"refs/tasks/{}\", task.id);",
            "detail": "Spawn writes refs/agents/{id}/head and refs/tasks/{task-id} into the repo — the task's branch marker required by this section."
          },
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "        let snap_prefix = format!(\"refs/agents/{}/snapshots/\", agent.id);\n        let n = git_refs::count_refs_under(&repo.path, &snap_prefix).await;\n        let snap_ref = format!(\"refs/agents/{}/snapshots/{}\", agent.id, n);",
            "detail": "Each completion writes refs/agents/{id}/snapshots/{n} with a monotonically increasing n, recording that session's completion point (the source branch tip) — the exact ref scheme the spec specifies."
          },
          {
            "file": "crates/gyre-adapters/src/jj_ops.rs",
            "quote": "        self.run_jj(repo_path, &[\"undo\"]).await?;",
            "detail": "jj undo is exposed through the port/adapter/HTTP route, giving the rollback-to-previous-state capability the section describes for crash recovery."
          }
        ]
      },
      {
        "id": "da459441ae49be4692a0d2ed",
        "decision": "satisfied",
        "requirement_quote": "- **Conventional commits** are validated at push time regardless of whether the client used git or jj\n- **Commit signing** (Sigstore) works on the git objects jj produces",
        "detail": "The diagram is context, but the section's two normative bullets are implemented. (1) Conventional-commit validation happens in the single server-side push path — git_receive_pack's check_pre_accept_gates evaluates the repo's configured push gates including 'conventional-commit'; since both git and jj clients deliver commits through the same smart-HTTP receive-pack (jj work reaches the forge via jj git export into the bare repo and via git push), the validation is client-agnostic by construction; a failed gate undoes the ref updates and rejects the push. (2) Commit signing on jj-produced objects: POST /repos/:id/jj/squash squashes the jj working copy and signs the resulting commit SHA — keyless via Fulcio+Rekor (sign_commit_keyless with the caller's validated JWT attribution) or local Ed25519 with the forge key, with real verification (ring Ed25519 verify over the SHA; Fulcio cert-chain + Rekor entry verification) exposed at GET /repos/:id/commits/:sha/signature/verification.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "async fn check_pre_accept_gates(",
            "detail": "Push-time gate evaluation inside receive-pack: gate names (including 'conventional-commit', exercised by tests asserting gate_name in the rejection detail) are read per-repo and a rejection undoes ref updates — validation at push time, enforced, on the one path both git and jj clients push through."
          },
          {
            "file": "crates/gyre-server/src/api/jj.rs",
            "quote": "        match crate::sigstore::sign_commit_keyless(",
            "detail": "jj squash signs the commit it produces via Sigstore keyless signing (Fulcio certificate against the caller's OIDC JWT, Rekor entry), falling back to local Ed25519 on external-stack failure — signing works on the git objects jj produces."
          },
          {
            "file": "crates/gyre-server/src/commit_signatures.rs",
            "quote": "    let pk = UnparsedPublicKey::new(&signature::ED25519, public_key_bytes);\n    pk.verify(record.commit_sha.as_bytes(), &sig_bytes).is_ok()",
            "detail": "Verification is real crypto (ring Ed25519 over the commit SHA bytes), and the Fulcio path verifies the certificate chain against the configured trust bundle plus the Rekor entry — not a structural check."
          }
        ]
      },
      {
        "id": "185fefd8851a3d28fce11630",
        "decision": "satisfied",
        "requirement_quote": "- **Merge Requests (MRs)** are a first-class platform primitive - not a UI feature layered on top, but a core domain object with identity, lifecycle, and audit trail.",
        "detail": "Every bullet of this section is production code. MRs are a core domain object (gyre-domain MergeRequest with Id, guarded state machine Open→Approved→Merged/Closed/Reverted, depends_on dependencies, atomic_group, merge/revert commit SHAs) persisted through a port-backed repository (SQLite/PostgreSQL adapters), with a timeline endpoint assembling the audit trail from gate results, agent commits, graph extractions, and workspace messages. The merge queue is built in: a 5s processor cycle lists queued entries, builds the dependency graph, topologically sorts with priority-within-tier (urgent/priority entries jump ahead), checks per-MR quality gates (GateType::TestCommand, LintCommand, RequiredApprovals, AgentReview — tests, lints, and review approvals as the spec lists), holds entries until dependencies merge (dependencies_satisfied), merges atomic groups transactionally with rollback/requeue, merges sequentially against the moving head via real git three-way merges (git2 merge_branches), and on baseline movement automatically rebases in-flight agents (§4 above) with conflict escalation to the submitting agent (Escalation message with new_base_sha and conflicting files) and to managers (Critical-priority escalation tasks on dependency-health failures). Spec policies, supervised-trust holds, and attestation ABAC enforce policy inline before the merge commit is created.",
        "evidence": [
          {
            "file": "crates/gyre-domain/src/merge_request.rs",
            "quote": "    pub fn transition_status(&mut self, new_status: MrStatus) -> Result<(), MrError> {\n        let valid = matches!(\n            (&self.status, &new_status),\n            (MrStatus::Open, MrStatus::Approved)\n                | (MrStatus::Open, MrStatus::Closed)\n                | (MrStatus::Approved, MrStatus::Merged)\n                | (MrStatus::Approved, MrStatus::Closed)\n                | (MrStatus::Merged, MrStatus::Reverted)\n        );",
            "detail": "MR lifecycle is a guarded domain state machine — a core domain object with identity and lifecycle, not a UI overlay."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "        available.sort_by(|a, b| {\n            let ea = entry_map.get(a);\n            let eb = entry_map.get(b);\n            let pa = ea.map(|e| e.priority).unwrap_or(0);",
            "detail": "Priority ordering within each topological tier — higher-priority (urgent) entries are selected first, implementing 'priority ordering (urgent fixes jump the queue)'."
          },
          {
            "file": "crates/gyre-common/src/gate.rs",
            "quote": "    /// Require N or more approved reviews.\n    RequiredApprovals,",
            "detail": "Quality gates include tests (TestCommand), lints (LintCommand), and review approvals (RequiredApprovals) — evaluated per-MR by check_gates_for_mr during queue candidate selection before any merge."
          },
          {
            "file": "crates/gyre-adapters/src/git2_ops.rs",
            "quote": "            let mut index = repo.merge_trees(&ancestor_tree, &our_tree, &their_tree, None)?;",
            "detail": "Actual merges are real git three-way merges against the current branch tips (sequential merging against a moving head), producing a true merge commit whose first parent is the pre-merge target tip."
          },
          {
            "file": "crates/gyre-server/src/merge_processor.rs",
            "quote": "                    gyre_common::message::MessageKind::Escalation,",
            "detail": "Conflict escalation to the submitting agent: on a conflicted automatic rebase the author's MR is flagged and an Escalation message is sent to the agent with the new base and conflicting files; dependency-health failures additionally create Critical-priority escalation tasks (the manager path)."
          }
        ]
      },
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
      },
      {
        "id": "09c28182c53ccd934aa3b93a",
        "decision": "gap",
        "requirement_quote": "- Support **pull mirroring** from external forges (GitHub, GitLab) for repos that need to be brought into Gyre.\n- GitHub App integration for authenticated access to private repos (later milestone, not M0).\n- Mirror namespace isolation (e.g., `mirrors/github/{owner}/{repo}`).",
        "detail": "Two of three bullets are fine: pull mirroring is real (POST /api/v1/repos/mirror clones the remote as a bare mirror via git_ops.clone_mirror with SSRF-gated https-only URLs; a mirror_sync job registered in the job framework runs every 60s calling git_ops.fetch_mirror on every is_mirror repo, updates last_mirror_sync, and runs post-sync spec-ledger + graph extraction; pushes to mirrors are rejected read-only in receive-pack), and the GitHub App integration is explicitly deferred by the spec itself ('later milestone, not M0'), so it is not a gap. The gap is mirror namespace isolation: mirror repos are stored at exactly the same path namespace as native repos — {repos_root}/{workspace_id}/{name}.git in both create_repo and create_mirror_repo — with no mirrors/ prefix and no encoding of the upstream forge identity (github/{owner}/{repo}). Mirrors and native repos share one namespace per workspace (backed only by the unique (workspace_id, name) index), so a mirror cannot coexist with a same-named native repo and mirror paths carry no forge-origin isolation; the spec's isolation bullet has no corresponding implementation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "    let repo_path = format!(\"{}/{}/{}.git\", state.repos_root, req.workspace_id, req.name);",
            "detail": "create_mirror_repo (line 639) computes the mirror's storage path with the identical formula create_repo uses for native repos (line 176) — mirrors live in the same namespace as native repos with no mirrors/ prefix and no upstream forge identity (github/{owner}/{repo}) encoded anywhere in the path; the namespace-isolation bullet is unimplemented."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "                name: \"mirror_sync\".to_string(),\n                description: \"Fetches latest refs for all mirror repositories\".to_string(),",
            "detail": "Pull mirroring itself is real: the mirror_sync job is registered in the job framework (60s interval) and mirror_sync::run_once fetches all is_mirror repos via git_ops.fetch_mirror, updates last_mirror_sync, and runs post-sync spec-ledger sync and knowledge-graph extraction — the first bullet is satisfied."
          },
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "        is_mirror: true,\n        mirror_url: Some(url.clone()),\n        mirror_interval_secs: req.interval_secs,",
            "detail": "The mirror Repository record carries is_mirror/mirror_url/mirror_interval_secs, and receive-pack enforces read-only on mirrors — what is missing is specifically the isolated mirror namespace, not mirror functionality."
          }
        ]
      }
    ],
    "gaps": [
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
      },
      {
        "id": "09c28182c53ccd934aa3b93a",
        "decision": "gap",
        "requirement_quote": "- Support **pull mirroring** from external forges (GitHub, GitLab) for repos that need to be brought into Gyre.\n- GitHub App integration for authenticated access to private repos (later milestone, not M0).\n- Mirror namespace isolation (e.g., `mirrors/github/{owner}/{repo}`).",
        "detail": "Two of three bullets are fine: pull mirroring is real (POST /api/v1/repos/mirror clones the remote as a bare mirror via git_ops.clone_mirror with SSRF-gated https-only URLs; a mirror_sync job registered in the job framework runs every 60s calling git_ops.fetch_mirror on every is_mirror repo, updates last_mirror_sync, and runs post-sync spec-ledger + graph extraction; pushes to mirrors are rejected read-only in receive-pack), and the GitHub App integration is explicitly deferred by the spec itself ('later milestone, not M0'), so it is not a gap. The gap is mirror namespace isolation: mirror repos are stored at exactly the same path namespace as native repos — {repos_root}/{workspace_id}/{name}.git in both create_repo and create_mirror_repo — with no mirrors/ prefix and no encoding of the upstream forge identity (github/{owner}/{repo}). Mirrors and native repos share one namespace per workspace (backed only by the unique (workspace_id, name) index), so a mirror cannot coexist with a same-named native repo and mirror paths carry no forge-origin isolation; the spec's isolation bullet has no corresponding implementation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "    let repo_path = format!(\"{}/{}/{}.git\", state.repos_root, req.workspace_id, req.name);",
            "detail": "create_mirror_repo (line 639) computes the mirror's storage path with the identical formula create_repo uses for native repos (line 176) — mirrors live in the same namespace as native repos with no mirrors/ prefix and no upstream forge identity (github/{owner}/{repo}) encoded anywhere in the path; the namespace-isolation bullet is unimplemented."
          },
          {
            "file": "crates/gyre-server/src/jobs.rs",
            "quote": "                name: \"mirror_sync\".to_string(),\n                description: \"Fetches latest refs for all mirror repositories\".to_string(),",
            "detail": "Pull mirroring itself is real: the mirror_sync job is registered in the job framework (60s interval) and mirror_sync::run_once fetches all is_mirror repos via git_ops.fetch_mirror, updates last_mirror_sync, and runs post-sync spec-ledger sync and knowledge-graph extraction — the first bullet is satisfied."
          },
          {
            "file": "crates/gyre-server/src/api/repos.rs",
            "quote": "        is_mirror: true,\n        mirror_url: Some(url.clone()),\n        mirror_interval_secs: req.interval_secs,",
            "detail": "The mirror Repository record carries is_mirror/mirror_url/mirror_interval_secs, and receive-pack enforces read-only on mirrors — what is missing is specifically the isolated mirror namespace, not mirror functionality."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "35dee813aece670bf34875fa6d322ee56de210a9199688e0089f6f696eb4c8ee"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "03f98632204e0c91331d34dfb53d5cbe4a71a7045fa9191ef9efeda3d434afe0"
      },
      {
        "file": "crates/gyre-adapters/src/jj_ops.rs",
        "sha256": "2aab35125010299a36c418e51bf6ed6da8e7f70d4f747cafc854ee2447af55c6"
      },
      {
        "file": "crates/gyre-server/src/api/jj.rs",
        "sha256": "b33891fd0503b996562f72b506941aa93b56d1baa0892121a740a0b2ea5d585c"
      },
      {
        "file": "crates/gyre-server/src/merge_processor.rs",
        "sha256": "fec303f873af9873c51fb9aead25e394047fad4695bbb4237c78427cfc3e5fc2"
      },
      {
        "file": "crates/gyre-ports/src/jj_ops.rs",
        "sha256": "3e52a49c370b1f6e985a2e1f165a0d08fc0169e3121aec73afee07892567e20b"
      },
      {
        "file": "crates/gyre-server/src/speculative_merge.rs",
        "sha256": "b2f85201f20c3e8d880bc1ab0516f986ae8fc04db301d235a2fa2b4ba9279c5c"
      },
      {
        "file": "crates/gyre-adapters/src/git2_ops.rs",
        "sha256": "b2bf5efd76122b510859c5a1fcd88ea23d163d76aea170b8fe6abf7f4eba0da4"
      },
      {
        "file": "crates/gyre-server/src/commit_signatures.rs",
        "sha256": "d3f7a344d7d8a1e3f4016b4c96b83f12e04581537328ceaeda3f7a5f239de8cd"
      },
      {
        "file": "crates/gyre-domain/src/merge_request.rs",
        "sha256": "079a58a8d020aa88775a1969c667814b25cbaf4c3bead3c21b1baf6a644f3b9a"
      },
      {
        "file": "crates/gyre-common/src/gate.rs",
        "sha256": "8d0a43e42284b5077f49726d9d3270a1fa206bea10194ccf07a2326a782c95ce"
      },
      {
        "file": "crates/gyre-domain/src/agent_tracking.rs",
        "sha256": "0f5f5292dd3bfdaa96effe0113029a9f4b2eae25fc2f8b87edd6632cee4ad6ce"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "7cd819be7922b9c62b5e48b6e84cf5c3b75c8015b76f16501439df4afc6219e1"
      },
      {
        "file": "crates/gyre-server/src/api/repos.rs",
        "sha256": "8563468055bc575044403a2235aefa022a8941b3940bfa30b622da8ca9b2e640"
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "sha256": "10f96912418b108b4a0baf56cc0f9edf624e6a7fc51095c1a5a9c31adb40eb2c"
      }
    ]
  },
  "plan": {
    "audit": "7312a54134f405f818b3cacb49eb1f6db85c048f8ee1dd146c8171d47cc9ef04",
    "tasks": [
      {
        "key": "agent-session-commit-tracking",
        "title": "Agent-to-commit tracking at session granularity: session persistence, branch lifecycle, dirty state, file/commit queries, and time-travel",
        "covers": [
          "633595c9df8812514a9517ad"
        ],
        "required_behavior": "Per specs/system/source-control.md §Agent-to-Commit Tracking, implement all five bullets as real, persisted, queryable production behavior. Today only agent-level push recording exists (git_http.rs record_pushed_commits writes AgentCommit rows keyed to agent_id); the Session domain entity (crates/gyre-domain/src/agent_tracking.rs) is never persisted — there is no SessionRepository port, no adapter, no session column on agent_commits — so 'which agent session touched which commits' is answerable only at agent granularity. (1) Session-to-commit mapping (chronological): add a SessionRepository port (crates/gyre-ports/src/agent_tracking.rs, alongside AgentCommitRepository) implemented by the SQLite adapter (next unused migration number — current latest is 000066), the PostgreSQL adapter, and the mem adapter, with every port failure constraint guarded identically in all adapters (scripts/check-mem-port-contracts.sh); wire it into AppState via the existing store! pattern in crates/gyre-server/src/lib.rs (no bare Arc<Mutex<HashMap>> state store — scripts/check-in-memory-state-stores.sh). A Session row is created at agent spawn (api/spawn.rs) and at each Ralph-loop respawn with fresh context (ralph_loop.rs relaunch_agent_session) — one session per context window; ended_at is set on every termination path: complete_agent, fail_agent, and the stale-agent Dead transition (stale_agents.rs). Extend AgentCommit with session_id: record_pushed_commits resolves the pushing agent's currently-open session and stamps it; when no open session exists the commit is recorded with a NULL session_id — never a fabricated id; POST /repos/:id/commits/record follows the same resolution. Expose a chronological commits-by-session query. (2) Branch lifecycle: record the initial branch at spawn (AgentWorktree.branch), the current branch at each session end — resolved with a real lookup against the agent's worktree/refs on the termination paths, never the spawn-time value copied forward — and all observed commits (commits recorded during the agent's lifetime); expose a per-agent branch-lifecycle view. (3) Dirty state: on every termination path, detect uncommitted work in the agent's worktree with a real VCS query (git status --porcelain in the worktree, or equivalent jj working-copy diff) and record it on the session as a dirty flag plus the list of uncommitted paths — never a hardcoded value; a missing/inaccessible worktree records an explicit unknown/error state, not a silent clean. (4) Commit-based queries: record files_changed per pushed commit at push time (git diff-tree --no-commit-id --name-only -r <sha> in record_pushed_commits; the machinery already exists at git_http.rs:3996) stored on AgentCommit, and expose 'find all agent sessions that touched this file/commit': a by-file query returning the sessions (and agents) whose recorded commits touched the path, and a by-commit-SHA query returning the linked session(s). (5) Time-travel: an endpoint that reconstructs what the repo looked like when a given agent session was active — the commits present in the repo during [started_at, ended_at] (git log --before/--after style query against the real object store) plus the tree/commit state at the session's completion point by resolving the existing refs/agents/{agent-id}/snapshots/{n} refs (git_refs::resolve_ref) — real queries against real git state, never current-HEAD or mocked stand-ins. Every new route registered in api/mod.rs MUST have an ABAC RouteResourceMapping entry in abac_middleware.rs (scripts/check-abac-route-registry.sh); any handler running without middleware ABAC MUST enforce per-handler scoped authorization (load the entity, compare tenant/workspace against the caller, Forbidden on mismatch — scripts/check-abac-exempt-handlers.sh); scope fields are derived from or validated against the caller (scripts/check-forged-scope-fields.sh). All state is DB-backed so sessions and session-stamped commits survive a server restart.",
        "acceptance": [
          "Two-session attribution: spawn an agent, push commit A (session 1), trigger a Ralph-loop respawn (fresh context), push commit B (session 2). The agent's session list shows two sessions with start/end times, and the commits-by-session query returns exactly [A] for session 1 and [B] for session 2, chronologically ordered — session granularity, not agent granularity.",
          "Dirty state: completing an agent whose worktree contains an uncommitted modification records the terminated session with dirty=true and that file path listed; completing with a clean worktree records dirty=false; both remain readable after a server restart (port-backed persistence).",
          "Branch lifecycle: the per-agent branch-lifecycle view returns the initial (spawn-time) branch, the current branch recorded at termination (observable to differ after the agent moved branches mid-session), and all observed commits.",
          "File/commit queries: after pushing a commit touching src/lib.rs, the by-file session query for src/lib.rs returns the producing session; a file touched by no recorded commit returns an empty list; the by-commit query for that SHA returns its session.",
          "Time-travel: the session state view returns the repo state during the session's active window — commits that existed during [started_at, ended_at] and the tree at the session's completion snapshot ref; a commit pushed after the session ended is excluded, and the result differs from current HEAD when the branch has moved since.",
          "cargo test --all passes with new adapter contract tests, and scripts/check-mem-port-contracts.sh, check-abac-route-registry.sh, check-abac-exempt-handlers.sh, check-in-memory-state-stores.sh, and check-migration-versions.sh all remain clean (no new exemption entries)."
        ],
        "negative_probe": "Run an agent through two Ralph-loop sessions (spawn → push commit A; gate-failure respawn with fresh context → push commit B) and ask the spec's question 'which agent session touched which commits': on the current tree this is unanswerable — the Session entity is never constructed outside its own unit test, gyre-ports/src/agent_tracking.rs defines no SessionRepository, and the agent_commits table has no session column (every row is session-anonymous). After the task, the commits-by-session query for session 1 must return exactly [A] and not [B]; if session_id were stamped with a fabricated constant instead of the resolved open session, both commits would land in one session and this probe fails. Companion probes for the hollow modes this gap invites: complete an agent whose worktree holds an uncommitted edit and assert the session's dirty state is true with that file listed (a hardcoded false fails); push a commit touching src/lib.rs and query sessions by that file (an empty files_changed Vec — the existing TurnCommitLink pattern at git_http.rs:1715 — returns nothing and fails); request the time-travel view for a session that ended before a later commit landed and assert the later commit is absent (returning current HEAD fails).",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "mirror-namespace-isolation",
        "title": "Mirror namespace isolation: mirrors/{forge}/{owner}/{repo} storage namespace for external-forge mirrors",
        "covers": [
          "09c28182c53ccd934aa3b93a"
        ],
        "required_behavior": "Per specs/system/source-control.md §External Repository Mirroring (mirror-namespace-isolation bullet), store mirror repositories in an isolated namespace that encodes the upstream forge identity, instead of sharing the native-repo namespace. Today create_mirror_repo (crates/gyre-server/src/api/repos.rs:639) computes the mirror's storage path with the exact formula create_repo uses for native repos (line 176): {repos_root}/{workspace_id}/{name}.git — no mirrors/ prefix and no forge identity anywhere in the path. Change create_mirror_repo so a mirror's storage path is {repos_root}/{workspace_id}/mirrors/{forge}/{owner}/{repo}.git (spec example: mirrors/github/{owner}/{repo}), where the segments are parsed from the mirror URL: host github.com → github, gitlab.com → gitlab, any other https host → a sanitized host-derived segment; owner/repo from the URL path segments. Every derived segment must be validated (reject '..', '/', and percent-encoded/separator traversal) BEFORE any filesystem write, so the derived path can never escape the mirrors/ namespace; unsafe URLs are rejected with a 4xx. The stale-directory cleanup that runs before clone_mirror ('Removing stale repo directory before mirror clone') must be scoped so it can only ever remove paths under {repos_root}/{ws}/mirrors/ — creating a mirror must never touch a native repo's directory. Creating a second mirror of the same upstream URL in the same workspace (same derived path) must fail cleanly rather than silently share or overwrite storage. Everything downstream follows the stored Repository.path unchanged: the mirror_sync job / fetch_mirror, post-sync graph extraction and spec-ledger sync, receive-pack mirror read-only enforcement, and smart-HTTP clone-by-name. Existing mirror rows keep their persisted paths (path is per-record; no rewrite or migration of previously created mirrors). Native create_repo is unchanged and stays unable to allocate a path under mirrors/ (its name validation already rejects '/'). Keep the existing SSRF https-only URL gate and the (workspace_id, name) DB identity rule — this task is the storage-namespace isolation the spec mandates, not a rework of mirror functionality. The derived path is canonicalized at rest (absolute under repos_root) per scripts/check-relative-path-defaults.sh.",
        "acceptance": [
          "POST /api/v1/repos/mirror with url=https://github.com/octocat/Hello-World.git in workspace W creates a repo whose record path and on-disk storage are {repos_root}/{W}/mirrors/github/octocat/Hello-World.git; the mirror_sync job fetches refs into that path and the repo is cloneable via the existing smart-HTTP name route.",
          "Disjoint namespaces: a native repo and a mirror in the same workspace never share a storage path — creating the mirror leaves every native repo directory and its refs untouched (the pre-clone stale-dir cleanup provably cannot path outside mirrors/).",
          "A mirror URL containing traversal segments (owner '..', embedded '/', or encoded separators) is rejected with a 4xx and creates no directory anywhere on disk.",
          "Creating a second mirror of the same upstream URL in the same workspace is rejected cleanly (error response, first mirror's storage intact).",
          "Mirror rows created before this change still sync and serve from their persisted paths (no migration required for them); native repo creation is byte-for-byte unchanged.",
          "cargo test --all passes, with existing mirror-creation tests updated to the new path shape."
        ],
        "negative_probe": "POST /api/v1/repos/mirror with url=https://github.com/octocat/Hello-World.git and name Hello-World, then inspect the created Repository.path and the filesystem: on the current tree the mirror is stored at {repos_root}/{ws}/Hello-World.git — byte-identical in namespace to a native repo (create_mirror_repo at crates/gyre-server/src/api/repos.rs:639 uses create_repo's exact formula from line 176), so the spec's mirrors/github/{owner}/{repo} isolation does not exist anywhere in the codebase. After the task, the same request must store the mirror at {repos_root}/{ws}/mirrors/github/octocat/Hello-World.git — assertable on both the repo record and the on-disk directory — and a probe asserting the old flat path fails. Traversal companion: submit a mirror URL whose owner segment is '..' (or contains an encoded separator); a naive format!-concatenated path would resolve to {ws}/mirrors/github/../… and escape the namespace, so the request must be rejected with a 4xx before any filesystem write — a probe that asserts no directory outside mirrors/ is created (and that the native repo directory adjacent to the escape target is intact) fails if the validation is missing.",
        "dependencies": [],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "agent-session-commit-tracking": "task-258",
    "mirror-namespace-isolation": "task-259",
    "_publication": "task-260",
    "_base": "321615eced877c4839ccb98b78fae10778559933"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Task definitions and explicit bindings only; no product implementation claimed.
