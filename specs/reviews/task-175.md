# Review — task-175 (Specs and Inbox view layouts — spec list, inbox cards, briefing narrative)

Spec: `specs/system/ui-layout.md` §6 (Specs View Layout), §7 (Inbox Layout, Item Structure, Action Buttons per Item Type), §8 (Briefing Layout), with `specs/system/human-system-interface.md` §8 for the 10-level inbox priority table.
Candidate under review: `ff2bc99c` on `pipeline/task-175/d26589fe67374a86bc7bc5c4e7031b15-8`, diffed against assigned base `59ff5d01` (task-172 Editor Split merge base).
Verdict: **approved**.

## Scope of the diff

`git diff 59ff5d01..ff2bc99c` touches 7 files, all task-relevant:
`crates/gyre-server/src/api/specs_assist.rs` (+212 — HSI §8 P2 notification fan-out),
`crates/gyre-server/src/spec_registry.rs` (get_blob_sha made pub(crate)),
`web/src/components/Inbox.svelte` (+536 — all 10 priority types' action buttons, PascalCase wire normalization, tenant-scope workspace badges, P2 inline MR diff),
`web/src/__tests__/Inbox.test.js` (+584),
`web/src/lib/api.js` (+15 — pauseMergeQueue/resumeMergeQueue/revertMr/mrStatus/stopAgent helpers),
`web/src/locales/en.json` (+57), `specs/tasks/task-175.md`.
All 12 frontmatter-attributed commits verified as ancestors of the candidate. The §6/§8 surfaces (SpecDashboard, DetailPanel, Briefing) predate the branch (built in earlier merged rounds) and were verified in their current state.

## Evidence (commands, exit codes, outputs in /tmp/stage/review-evidence/task-175-review-probes.md)

- `cd web && npm ci` → EXIT 0; `npx vitest run Inbox.test.js AccordionItem.test.js AccordionScoping.test.js` → **78 passed, 0 failed**; `npx vitest run Briefing.test.js SpecDashboard.test.js DetailPanel.test.js EditorSplit.test.js` → **127 passed, 0 failed**.
- `/tmp/stage/bin/cargo test -p gyre-server --lib api::specs_assist::tests::save_spec` → **8 passed, 0 failed**, including `save_spec_fans_out_approval_notification_to_eligible_members`.
- **Mutation probe (frontend)**: removing the `ConflictingInterpretations` entry from the typeNormMap makes the server-wire-format matrix test fail (1 failed | 9 passed) — the matrix genuinely guards the PascalCase normalization; without it a wire type renders a card with no action buttons.
- **Mutation probe (server)**: narrowing the fan-out role filter to Owner-only makes `save_spec_fans_out_approval_notification_to_eligible_members` fail (`user user-admin must get exactly one SpecPendingApproval notification: left 0, right 1`) — the test genuinely guards per-member fan-out, not just notification creation. Both mutations restored; tree clean at candidate.
- Endpoints wired by the new actions all exist and are real: `PUT /repos/:id/queue/pause|resume`, `POST /repos/:id/revert/:mr_id` (recovery.rs), `PUT /merge-requests/:id/status` (real status transition incl. close-without-merge trace deletion), `POST /agents/:id/stop`, `POST /merge-requests/:id/reviews`, `GET /users/me/notifications`.
- Wire-format fidelity: every PascalCase string in the new typeNormMap matches `NotificationType::as_str()` in gyre-common/src/notification.rs; the ConflictingInterpretations server emission (graph_extraction.rs) populates exactly the body fields (agent_a/b, commit_sha_a/b, conflicting_nodes, resolution_options) and `repo_id` that the P5 actions consume.
- §6/§8 structure verified in code: DetailPanel spec tab set is Content/Edit/Progress/Links/History; SpecDashboard has both sortable tables + `[+ New Spec]` modal; Briefing has the five section markers and the InlineChat "Ask about this briefing"; MR detail tabs (diff/gates) honor the `_openTab` hint used by Inbox View Diff/View Output.
- i18n: 800 `$t()` keys extracted from all components, zero missing in en.json (including the 6 decisions.* keys the round notes claim were added).
- rustfmt `--check` on specs_assist.rs → clean.
- Mechanical checks: fail-open-ref-resolution, scope-literal-defaults, fabricated-scope-defaults, conditional-test-guards, aspirational-test-names all pass. `check-assertionless-tests.sh` and `check-comparative-test-claims-js.sh` exit 2 with no diagnostics **identically on base 59ff5d01** (verified in a throwaway worktree) — pre-existing environment behavior, not a candidate defect.

## Findings

None blocking. Minor observations recorded for completeness:

- [~] `get_blob_sha(...).unwrap_or_default()` in `save_spec` yields an empty `spec_sha` when git is unavailable; the Inbox approve path treats empty as falsy and falls back to the spec-ledger SHA, which may differ from the spec-edit branch version. The primary path (blob SHA of the branch) is real and the fallback is documented best-effort for legacy body-less notifications; not hollow.
- [~] P5 `resolveConflictMrId` matches merged MRs by `merge_commit_sha`; the divergence body's `commit_sha_a/b` come from architectural deltas and may be branch heads rather than merge commits. The code deliberately returns null and surfaces a targeted "could not find the merged MR" error (covered by a test) instead of reverting an unmatched MR — fail-safe, not a defect.
- [~] The Shipped notes reference `/tmp/stage/review-evidence/task-175-probes.md`, which was not present in this sandbox; all claimed probe results were re-established independently by this review (identical pass counts). Ephemeral /tmp bookkeeping, not a code defect.

Coverage rows 32-36 in `specs/coverage/system/ui-layout.md` remain `task-assigned`, consistent with the task sitting at `ready-for-review`; rows flip at completion.
