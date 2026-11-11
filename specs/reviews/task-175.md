# Review — task-175 (Specs and Inbox view layouts — spec list, inbox cards, briefing narrative)

Spec: `specs/system/ui-layout.md` §6-§8 (+ §Item Structure, §Action Buttons per Item Type; HSI §8 priority table).
Candidate: `b85effc610e651bc897dcbaad31b93c61326a11e` (base `cfeefbda975211a6ac24a17e5920be6f8b23008b`).
Verdict: **approved** — no findings.

## Scope of the delta

Net diff vs base: `Inbox.svelte` (§7 rework: card structure, accordion, all 10 priority-type action rows, spec-diff inline review), `Inbox.test.js`, server-side P2 approval fan-out (`specs_assist.rs`), `update_body_by_entity_ref` on the notification port + SQLite/PostgreSQL/mem adapters, MR list attestation enrichment (`merge_requests.rs`), `api.js` wrappers, `en.json` keys, rebase of the branch onto main's notification taxonomy. `SpecDashboard.svelte`, `Briefing.svelte`, and `DetailPanel.svelte` are byte-identical to base — the §6/§8 surface pre-existed this round and was re-verified against the spec at the candidate tree rather than assumed.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cd web && npm ci` → exit 0; `npx vitest run Inbox.test.js Briefing.test.js SpecDashboard.test.js` → **133/133 passed**.
- **Mutation probe**: removing the fail-closed `mr.status !== 'merged'` guard in `resolveSpecSha` (fail-open on an open spec-edit MR) fails exactly the F3 regression test (1 failed / 73 passed). The tests kill real bugs; source restored and verified clean.
- `cargo test -p gyre-adapters --lib sqlite::notification` → **5/5 passed** (incl. the unresolved/dismissed/entity/type guard test for `update_body_by_entity_ref`).
- `cargo check -p gyre-server` → clean.
- `check-arch.sh`, `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`, `check-forged-scope-fields.sh` → all pass.
- `check-task-commit-attribution.sh` fails only against the executor's restored working-tree task record (`commits: []`); all 16 task-labeled SHAs are present in the candidate commit's own frontmatter. Not a candidate defect.

## Verified

- **§6**: tenant/workspace sortable table (Path, Status, Kind, Owner, Last Updated) with status/kind filter pills and `owner=me`; repo-scope progress table (path, status, `N/M tasks` bar); detail tabs Content/Edit/Progress/Links/History (`DetailPanel.svelte:103-107`); `[+ New Spec]` opens the editor modal → `POST /repos/:repo_id/specs/save`.
- **§7**: priority badge, description, tenant-scope workspace attribution badge, relative timestamp; accordion expand with uncertainty text, related spec, agent/persona info; action buttons for all 10 HSI §8 priority types, including P2 inline spec diff before approving, P3 View Diff/View Output/Retry/Override/Close MR, P5 Pick A/Pick B/Reconcile, P7 Increase Limit/Pause Work, P8 Show Dismissed toggle.
- **§8**: COMPLETED/IN PROGRESS/CROSS-WORKSPACE/EXCEPTIONS/METRICS sections, inline action buttons, clickable entity references opening the detail panel, time-range selector, "Ask about this briefing" chat input.
- **Server fan-out is real enforcement, not audit-only**: eligible workspace members (Admin/Developer/Owner) each get a notification; Viewer excluded; structured body carries spec_path/spec_sha/mr_id/branch/repo_id; `spec_blob_sha` never returns "" (documented fail-open avoidance, content-derived fallback over the exact bytes written); the append-to-existing-MR path refreshes unresolved cards' SHA in place with a consistent unresolved/undismissed guard across all three adapters.
- **Rebase resolution loses neither side**: renamed `SpecApprovalRequested` emitted with structured `NotificationSource`; ports union `update_body_by_entity_ref` with main's `mark_read`/`has_active_notification`.
- Compile-critical references in the deferred `#[cfg(test)]` code verified against current struct definitions (AuthenticatedAgent incl. `bearer_token`, WorkspaceMembership::new, MrResponse, MergeAttestation — all field-complete).

## Verifier must run (deferred per 4 GiB build budget)

- `cargo test -p gyre-server --lib api::specs_assist`
- `cargo test -p gyre-server --lib list_mrs_enriches_merged_rows_with_merge_commit_sha`
- Prefer `SKIP_WEB_BUILD=1` (build.rs regenerates `web/dist` on cargo invocations otherwise).
