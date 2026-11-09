# Review — task-175 (Specs and Inbox view layouts — spec list, inbox cards, briefing narrative)

Spec: `specs/system/ui-layout.md` §6–§8 (Specs View Layout, Inbox Layout,
Item Structure, Action Buttons per Item Type, Briefing Layout), with
`specs/system/human-system-interface.md` §8 as the priority-table reference.

Candidate: `ea4a2f3a4ed86be90c0369755e4ec307b402abd0` on base
`fefde2946f3a0af1f212588f54591516fee0f7b2` (merge-repair round; net diff vs
base: `Inbox.svelte`, `Inbox.test.js`, `api.js`, `en.json`,
`crates/gyre-server/src/api/specs_assist.rs`, `spec_registry.rs`
[visibility only], `specs/tasks/task-175.md`). Verdict: **needs-revision**.

## Round 1

Probe evidence: `/tmp/stage/review-evidence/task-175-review-probes.md`
(commands, exit codes, outputs).

### Verified working (no findings)

- **§6 Specs View / §8 Briefing** surfaces live in base (SpecDashboard,
  DetailPanel, Briefing from tasks 371/173/196 et al.); the candidate rides
  them through the merge intact. Focused suites: Briefing + SpecDashboard +
  DetailPanel + EditorSplit — 133 passed, 0 failed.
- **§7 Inbox structure**: cards render priority badge (P1–P10),
  title/subtitle, type badge, tenant-scope workspace-name badge, relative
  timestamp; accordion expand reveals full context (message, gate
  name/command, conflict nodes, related spec/agent/MR refs). Inbox +
  AccordionItem + AccordionScoping — 83 passed, 0 failed, including the
  PascalCase wire-format matrix over all 10 HSI §8 priority types (server
  wire format confirmed PascalCase via `NotificationType::as_str` in
  `gyre-common/src/notification.rs`; the typeNormMap covers every variant
  with a template branch).
- **Real API wiring for most new actions**: `mrStatus`, `submitReview`,
  `mrDiff` (inline spec-edit diff on P2 expand), `pauseMergeQueue`,
  `resumeMergeQueue`, `sendAgentMessage` (pause_requested StatusUpdate,
  payload satisfies the required `{status, summary}` schema),
  `createTask` (Reconcile) — all verified against the real routes and
  request/response shapes (probe P5). P2 inline diff consumes the real
  `DiffResponse` structure (`{type, content}` line fields confirmed).
- **Server fan-out** (`specs_assist.rs save_spec`): SpecPendingApproval
  notifications now fan out to every workspace Owner/Admin/Developer with a
  structured body (spec_path, spec_sha blob SHA, mr_id, mr_title, branch,
  repo_id); "system" fallback only when no eligible members exist.
  `cargo test -p gyre-server --lib specs_assist` — 22 passed, 0 failed,
  including the new fan-out test (Viewer excluded, no system fallback when
  members exist, per-user exactly-one assertions).
- i18n audit re-run independently: every `$t(...)` key in
  Inbox/Briefing/SpecDashboard/Inbox.test resolves in `en.json`.
- `scripts/check-task-commit-attribution.sh` — OK.

### Findings

- **F1 (major, code — P5 arbitration is non-functional in production; test
  evidence is mirrored-logic).** `Inbox.svelte resolveConflictMrId` resolves
  the losing side's MR via `api.mergeRequests({ repository_id })` and matches
  `m.merge_commit_sha === commitSha`. But `GET /api/v1/merge-requests`
  (`list_mrs`) never serializes `merge_commit_sha`: `MrResponse::from`
  hardcodes it to `None` with `skip_serializing_if = "Option::is_none"`, and
  only the single-MR `get_mr` enriches it (from the attestation store). So
  every list row lacks the field, the lookup always returns null, and Pick
  A / Pick B always end in the targeted error ("Could not find the merged MR
  for commit …") — `api.revertMr` is unreachable in production. The shipped
  tests ("Pick B reverts agent A's merged MR via the recovery endpoint",
  "Pick A reverts agent B's …") pass only because their `mergeRequests` mock
  fabricates `merge_commit_sha` on list rows — exactly the mirrored-logic
  class the task rules exclude ("a test that doesn't fail when the specced
  behavior breaks does not close a section"). Mutation probe (review-evidence
  P4): re-running the Pick B test with the real wire format (no
  `merge_commit_sha` on list rows) fails — `revertMr` is never called.
  Reproduce: `cd web && npx vitest run src/__tests__/Inbox.test.js` (passes
  with fabricated field) vs the P4 probe (fails without it). Remedy: resolve
  the MR by fetching each candidate MR individually (`api.mergeRequest(id)`,
  which does enrich `merge_commit_sha` from attestation) or have the server
  enrich `merge_commit_sha` in list responses; then fix the mock to the real
  wire format so the test can fail on regression.
- **F2 (moderate, code — P3 "Override" does not affect merge outcomes for
  process-gate failures).** `handleOverrideGate` POSTs a review
  (`decision: 'approved'`, `reviewer_agent_id: 'human'`). The merge
  processor's human-approval escape is `mr.status == MrStatus::Approved`
  (set via `PUT /merge-requests/:id/status`, which the Inbox does not call);
  submitted reviews are consumed only by the `agent_review` gate
  (`check_review_verdict` matches the gate's own agent id; the no-command
  stub matches any approved review). For the common P3 case — a process gate
  (e.g. cargo test) failed — the recorded review gates nothing: retry
  re-runs the failed gate and fails again. The button persists a real audit
  record but the specced semantics ("Override" = accept the failure and let
  the MR merge) are not enforced. Remedy: have Override set
  `mr.status = 'approved'` via the status endpoint (the human-approval escape
  the processor honors), optionally in addition to recording the review.
- **F3 (minor, code — legacy approve fallback can silently approve the wrong
  spec version).** `resolveSpecSha` falls back to the ledger's `current_sha`
  (`GET /specs/:path`) when `body.spec_sha` is absent/empty. For a
  still-open spec-edit MR the ledger's current SHA is the pre-edit content,
  so the approval event is recorded for the old version and is invalidated
  when the MR merges (`auto_invalidate_on_change` → Pending) — the human
  sees "Approved" but the approval has no lasting effect. Relatedly, the
  server writes `spec_sha: ""` into the notification body when
  `get_blob_sha` fails (`unwrap_or_default()`), which pushes real traffic
  onto this fallback. Remedy: on fallback, resolve the SHA from the MR's
  spec-edit branch (the diff the human just reviewed) rather than the
  ledger; or fail with the existing `missing_spec_sha` error instead of
  silently approving the pre-edit version.
- **F4 (minor, spec deviation).** HSI §8 P5 speccs Pick A/B as "creates a
  task to revert the non-selected MR's changes" via the existing task domain
  model; the implementation performs a direct revert
  (`POST /repos/:id/revert/:mrId`) instead. Arguably stronger when it works,
  but it contradicts the specced mechanism — per the repo-wide rule, amend
  the spec via spec lifecycle rather than shipping a divergent mechanism
  (and note the direct-revert path is currently unreachable per F1).

### Probe/sandbox notes

TCP listener probe unsupported in this sandbox — no live server/browser
end-to-end run; F1 is provable without a listener (pure wire-format mismatch,
mutation probe P4). Verifier with listener access could additionally confirm:
after seeding a `conflicting_interpretations` notification with real merged
MRs, clicking Pick A/B in the Inbox always shows the
"Could not find the merged MR" error and `POST /repos/:id/revert/:mr_id` is
never issued.
