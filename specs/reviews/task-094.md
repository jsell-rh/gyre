# Review — task-094 (Platform Model Cross-Repo Spec Escalation Protocol)

Spec: `platform-model.md` §3 "Cross-Repo Spec Escalation Protocol".
Commits under review: `0bff43283532f309624a7ceec8f0363cfd034406` (implementation),
`c9bd7fe63a921b45e513a74929a828c1b7cd1a94` (process notes). Base `ee03c299`.
Verdict: **needs-revision**.

Probes run (evidence: `/tmp/stage/review-evidence/task-094-review.md`):
`cargo test -p gyre-common --lib` → 132 passed, 0 failed. `cargo check -p gyre-server
--lib --tests` → clean. All repo mechanical checks on the changed surface pass (arch,
mcp-write-tools, dead-message-kinds, byte-slice, inert-enforcement, fabricated/scope-literal
defaults, relative-path, lossy-secret, fail-open-ref, forged-scope-fields). Server test
EXECUTION is wrapper-blocked in this sandbox (4 GiB); the verifier must run
`cargo test -p gyre-server --lib cross_repo_escalation`,
`cargo test -p gyre-server --lib mcp_escalate_cross_repo`, and `cargo test --all`.

Verified working (no findings):

- `EscalationPayload` + `EscalationType` in `gyre-common/src/message.rs`: both branches,
  kebab-case wire, unknown escalation types rejected rather than defaulted (a defaulting
  would silently skip the human gate); `MessageKind::Escalation` schema extended with
  optional cross-repo fields so generic `{reason, context}` escalations keep validating;
  required-field and type-mismatch tests executed and passing.
- `gyre_escalate_cross_repo` MCP tool: repo-orchestrator tier via the signed JWT's
  `orchestrator_type` claim; target repo validated to exist, live in the caller's
  workspace, and differ from the caller's own repo; optional task/MR ids validated against
  the caller's/target's repos against real state; `needs_write` RBAC gate membership added
  (check-mcp-write-tools passes). Worker-token denial and cross-workspace denial tests use
  meaningful fixtures (`r-x` exists in ws-2, so the denial is a scope denial, not
  not-found).
- ImplementationNeeded branch is real end-to-end: cross-repo Implementation task row in the
  TARGET repo (repo+workspace scoped, labeled), requesting-task `depends_on` (source-repo
  checked), agent-declared MR dependency through the same `depends_on` field
  `merge_processor::dependencies_satisfied` enforces (cycle-checked), TaskAssignment
  Directed message to the target repo's orchestrator (spawned on demand via the task-093
  core).
- Containment: both `escalate()` and `process_escalation()` verify both repos resolve in
  the same workspace; per-workspace registry lock serializes processing (no deadlock — the
  `ensure_*` helpers don't take the workspace lock, same shape as the SpecApproved chain).
- Poll-side receipt path (`drive_pending_escalations`, wired into `message.poll`): the
  polling agent's identity is re-verified against the agent record, not just JWT claims;
  only payloads parsing as a cross-repo `EscalationPayload` are consumed — verified against
  the actual generic-escalation emitters (stale-agent detector, spec-lifecycle patrol, gate
  executor, merge-processor rebase conflicts): none carry the required
  `source_repo_id`/`target_repo_id`/`needed_artifact`/`escalation_type`, so they stay
  unacked for their real consumers.

Findings:

- **F1 (major): the human gate's Approve/Reject decision path dead-ends for every
  cross-repo drafted spec change.** `handle_spec_change_needed`
  (`crates/gyre-server/src/cross_repo_escalation.rs`) writes the drafted spec to a
  `spec-edit/*` branch and creates the MR + priority-2 `SpecApprovalRequested`
  notifications, but never registers the spec: no `specs/manifest.yaml` entry on the branch
  and no `spec_ledger` row. Spec-ledger rows are created exclusively by `sync_spec_ledger`
  (`spec_registry.rs:449`), which runs only on git pushes to the default branch
  (`git_http.rs:1265-1269`), repo import (`repos.rs:947,1088`), and mirror sync — and only
  for paths listed in the manifest at HEAD; the escalation draft is written server-side via
  libgit2 and never passes through git_http. `POST /api/v1/specs/:path/approve`
  (`specs.rs:475-489`) and `reject` (`specs.rs:1351-1355`) both return 404 "spec not in
  registry" on a ledger miss, and the Inbox `spec_approval` card's Approve/Reject buttons
  (`web/src/components/Inbox.svelte` `handleApproveSpec`/`handleRejectSpec`) call exactly
  those endpoints with `body.spec_path`/`body.spec_sha`. SpecChangeNeeded means by
  definition the target repo's spec does not describe the artifact, so the drafted path is
  new, never enters the manifest or the ledger, and both inline actions 404: the
  SpecApproved signal chain never fires, so "If approved: spec lifecycle auto-creates task
  in Repo B" (spec §3, task plan item 5) cannot occur. Merging the spec MR manually
  doesn't close the gap: `merge_processor` merges via `git_ops.merge_branches`
  (`merge_processor.rs:1476`) with no ledger sync and no `SpecApproved` emission (grep:
  zero matches). The shipped notes, `docs/api-reference.md`, and coverage matrix row 18
  claim approval "rides the existing SpecApproved signal chain (auto-task in target repo)"
  — hollow for the only case the SpecChangeNeeded branch produces. No test exercises the
  approve/reject action completing; the spec-change test stops at notification-body
  assertions. Reproduce (verifier): run the
  `spec_change_needed_drafts_spec_mr_notification_and_dependency` fixture, then
  `POST /api/v1/specs/system%2Fuser-port.md/approve` `{"sha": <body.spec_sha>}` with an
  approver token → 404 "spec 'system/user-port.md' not in registry".
- **F2 (major): the rejection leg of the human gate is not implemented.** Task plan item 5
  requires "Rejection notifies workspace orchestrator to find alternative". `reject_spec`
  notifies human workspace Admin/Developer/Owner members (`specs.rs:1533-1569`), never the
  workspace orchestrator, and closes spec-edit MRs matched by an `mr.spec_ref` prefix
  (`specs.rs:1407-1412`) — the escalation's spec MR never sets `spec_ref` (same omission as
  `save_spec`), so it is not closed on rejection either. Combined with F1's 404, the
  reject action is non-functional for cross-repo drafts and no alternative-finding
  notification reaches the workspace orchestrator on any path.
- **F3 (minor): duplicate escalation side effects.** (a) `escalate()` does not dedupe
  repeated SpecChangeNeeded escalations for the same artifact — `save_spec` dedupes on an
  existing open spec-edit MR by branch prefix; the escalation handler always creates a new
  branch + MR + notification fan-out per escalation. (b) If `messages.acknowledge` fails
  after a successful processing run (warn-only in `process_escalation`), the message stays
  unacked and the next workspace-orchestrator `message.poll` reprocesses it via
  `drive_pending_escalations`, creating a duplicate cross-repo task — task creation is not
  idempotent and no processed-marker guards the retry.

Not charged to this task: MR dependency storage/merge-blocking/UI chain
(`merge_requests.depends_on`, `dependencies_satisfied`, `/merge-requests/{id}/dependencies`,
DetailPanel chain rendering) is pre-existing task-100 machinery the handler correctly
records into; the verifier confirmed surface exists.
