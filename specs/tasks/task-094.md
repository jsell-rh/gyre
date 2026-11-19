---
title: "Platform Model Cross-Repo Spec Escalation Protocol"
spec_ref: "platform-model.md §3 Cross-Repo Spec Escalation Protocol"
depends_on:
  - task-093
progress: not-started
coverage_sections:
  - "platform-model.md §3 Cross-Repo Spec Escalation Protocol"
commits: []
---

## Spec Excerpt

When a repo orchestrator needs something from another repo:

```
1. Repo A orchestrator: "I need trait UserPort from Repo B"

2. Repo A checks: does Repo B's spec describe UserPort?

   YES -> Repo A escalates to Workspace Orchestrator:
          "Repo B has UserPort in its spec but hasn't implemented it yet.
           I need it for TASK-042."
          Workspace Orchestrator:
          - Creates cross-repo task in Repo B
          - Creates MR dependency (Repo A's MR depends on Repo B's MR)
          - Notifies Repo B's orchestrator

   NO  -> Repo A escalates to Workspace Orchestrator:
          "Repo B does not have UserPort in its spec. I need a spec change."
          Workspace Orchestrator:
          - Drafts a spec change for Repo B (agent writes it)
          - Escalates to HUMAN for approval (spec change = design decision)
          - Human approves or rejects
          - If approved: spec lifecycle auto-creates task in Repo B
          - Workspace Orchestrator creates the MR dependency
```

**The human gate:** Cross-repo spec changes always require human approval.

## Implementation Plan

1. **Escalation message schema:**
   - Define `EscalationPayload` struct with `source_repo_id`, `target_repo_id`, `needed_artifact` (spec path or interface name), `reason`, `task_id`, `escalation_type` (ImplementationNeeded | SpecChangeNeeded)
   - Use existing `MessageKind::Escalation` with this structured payload

2. **MCP tool for escalation:**
   - Add `gyre_escalate_cross_repo` MCP tool for repo orchestrators
   - Input: target_repo, needed_artifact, reason, escalation_type
   - Sends Directed-tier Escalation message to workspace orchestrator

3. **Workspace orchestrator escalation handler:**
   - When workspace orchestrator receives Escalation via `message.poll`:
     - If ImplementationNeeded: create task in target repo, create MR dependency
     - If SpecChangeNeeded: draft spec change, create human notification (priority 2)
   - MR dependency: extend `MergeRequest` domain with `depends_on_mr_ids: Vec<Id>`

4. **MR dependency tracking:**
   - Add `mr_dependencies` table linking MRs across repos
   - Merge processor checks dependencies before merging: blocked MR cannot merge until dependency MR is merged
   - UI shows dependency chain on MR detail panel

5. **Human approval flow for cross-repo spec changes:**
   - Notification with "Approve spec change" / "Reject" inline actions
   - Approval triggers spec lifecycle auto-task creation
   - Rejection notifies workspace orchestrator to find alternative

## Acceptance Criteria

- [ ] EscalationPayload struct covers both escalation types
- [ ] `gyre_escalate_cross_repo` MCP tool available to repo orchestrators
- [ ] Workspace orchestrator can create cross-repo tasks
- [ ] MR dependency table and tracking implemented
- [ ] Merge processor blocks MRs with unmerged dependencies
- [ ] Human notification created for cross-repo spec changes
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/platform-model.md` §3 "Cross-Repo Spec Escalation Protocol". The Escalation MessageKind already exists in `gyre-common/src/message.rs`. MCP tools are defined in `gyre-server/src/mcp.rs`. The merge processor is in `gyre-server/src/merge_processor.rs`. MR dependencies are a new concept — check if any `depends_on` field already exists on the MR domain entity before creating one.
