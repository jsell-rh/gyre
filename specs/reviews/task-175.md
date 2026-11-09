# Review — task-175 (Specs and Inbox view layouts — spec list, inbox cards, briefing narrative)

Spec: `specs/system/ui-layout.md` §6–§8 (Specs View, Inbox, Briefing layouts) with
`specs/system/human-system-interface.md` §8 as the priority/action-button contract.
Candidate: `daee0c8d9bcf75edb4672a8c75caa216aa596788` (base `fefde294`), repair
commit `98f6c234` (round-3 findings F1–F3 from the prior review).
Verdict: **complete** (approved; evidence: `/tmp/stage/review-evidence/task-175-round3-probes.md`).

## Round 3 (this review)

The §6–§8 surface was reviewed and accepted in substance across earlier rounds
(sortable spec table + detail tabs + editor split; all-10-priority Inbox cards
with per-type actions and accordion context; five-section briefing narrative
with entity links, time-range selector, and chat). This round verified the
three repairs of the latest findings, that earlier repairs remain intact, and
that no new defects were introduced.

### F1 (moderate) — stale `spec_sha` on iterative saves: FIXED

`save_spec`'s append-to-existing-MR path (`crates/gyre-server/src/api/specs_assist.rs`)
now recomputes the blob SHA of the appended content via a shared `spec_blob_sha`
helper (`git rev-parse <branch>:<path>` primary, content-derived SHA-1 fallback —
never `""`) and refreshes the unresolved SpecPendingApproval cards' bodies in
place through a new `NotificationRepository::update_body_by_entity_ref(entity_ref,
type, body)` port method. Verified:

- All three `NotificationRepository` implementations (SQLite, PostgreSQL, mem)
  implement the method with identical match + unresolved/undismissed guard
  semantics — resolved/dismissed cards keep their historical body (audit record
  of what was acted on). `scripts/check-mem-port-contracts.sh` passes.
- The asserted SHA constants match real `git hash-object` output for the test
  content (independently reproduced), so the card carries the true blob ID that
  the ledger's post-merge `current_sha` will equal — Approve after merge records
  a matching approval instead of a permanent masked no-op.
- Mutation probe: replacing the append-path SHA with a wrong constant makes
  `save_spec_existing_mr_refreshes_notification_spec_sha` FAIL; removing the
  SQLite guard makes `update_body_by_entity_ref_updates_only_unresolved_matches`
  FAIL. Both tests are anchored to the fix, not incidental behavior.

### F2 (minor) — dead P5 MR lookup: FIXED

`resolveConflictMr` is gone. `handlePickSide` (`web/src/components/Inbox.svelte`)
creates the specced revert task with a description identifying the losing side
by agent + commit SHA (actionable via `git show`/`git revert <sha>`), matching
the prior round's F4 repair to the task-creation mechanism. Tests assert the
description carries both sides' agents/SHAs and that no MR list fetch occurs.
Mutation probe: dropping `lose_sha` from the description values makes the
Pick B test FAIL. The server-side MR-list attestation enrichment from the prior
round is retained and its Rust test still passes.

### F3 (minor) — dead API wrappers: FIXED

`api.revertMr` and `api.stopAgent` are removed (zero references in web/src).
The api.js wrappers this task adds (`mrStatus`, `pauseMergeQueue`,
`resumeMergeQueue`) all have real consumers in Inbox.svelte.

### Earlier repairs intact

- P3 Override (`handleOverrideGate`) still records the review AND calls
  `api.mrStatus(body.mr_id, 'approved')` — the merge processor's human-approval
  escape.
- `resolveSpecSha` still fails closed (`missing_spec_sha`) when the body lacks a
  branch SHA and the MR is still open, instead of fail-opening to the pre-edit
  ledger SHA.

### Probes (all on this checkout at the candidate SHA)

- `cargo test -p gyre-server --lib api::specs_assist` — 23 passed, 0 failed.
- `cargo test -p gyre-adapters --lib sqlite::notification` — 2 passed, 0 failed.
- `cargo test -p gyre-server --lib notification` — 26 passed, 0 failed.
- `cargo test -p gyre-server --lib list_mrs_enriches` — 1 passed, 0 failed.
- `cd web && npm ci && npx vitest run src/__tests__/Inbox.test.js` — 74 passed,
  0 failed; AccordionItem/AccordionScoping/Briefing — 43 passed, 0 failed.
- `scripts/check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-dead-message-kinds.sh`, `check-task-commit-attribution.sh` — all pass.
- i18n: all `$t()` keys used by the changed components resolve; the removed
  `revert_task_no_mr` key and `{lose_mr}` placeholder have zero references.
- `specs/tasks/task-175.md` frontmatter lists the repair commit `98f6c234`.

No findings. TCP listener probe unsupported in this sandbox, so no live
server/browser end-to-end run; component tests, real-SQLite adapter tests,
endpoint-level Rust tests, and the successful production web build (via cargo
build of gyre-server) stand in, as in prior rounds.
