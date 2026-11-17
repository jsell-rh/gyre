# Review: TASK-013 — Briefing Data Assembly (Cross-Workspace & Exceptions)

**Reviewer:** Verifier
**Round:** R4
**Verdict:** `complete` (0 findings)

---

## R1 Findings (all resolved)

- [-] [process-revision-complete] **F1: Cross-workspace filter direction is inverted relative to spec and link convention.** — Fixed in `ec71a1f1`. Filter now correctly checks `source_repo_id IN workspace AND target_repo_id NOT IN workspace`. Test data updated to match.
  **Process fixes:** Covered by implementation checklist item #53 (directional filter verification — spec narrative first, map roles to data model fields, derive predicates from mapping, test from spec narrative before code) and verifier target "Inverted directional filter."

- [-] [process-revision-complete] **F2: Missing `actions` field on exception items — spec violation.** — Fixed in `ec71a1f1`. `actions: Vec<String>` added to `BriefingItem` with static labels per exception type.
  **Process fixes:** Covered by implementation checklist item #54 (spec-excerpt response schema field completeness — enumerate every field in the spec excerpt's JSON schema and verify the struct includes each one).

- [-] [process-revision-complete] **F3: Gate results not filtered by gate result timestamp — old failures leak into briefing.** — Fixed in `ec71a1f1`. Inner filter now checks `gr.finished_at.map_or(false, |t| t >= since)`. Test case added for MR updated after `since` with gate result finished before `since`.
  **Process fixes:** Covered by implementation checklist item #55 (nested time-scope propagation — child entity timestamp filtering) and `scripts/check-nested-time-scope.sh` (pre-commit enforced).

## R2 Findings (all resolved)

- [-] [process-revision-complete] **F4: Exception `entity_type` values don't match the spec-defined type enum.** — Fixed in `bdd23253`. Changed `"assertion_failure"` → `"spec_assertion_failure"` and `"mr_revert"` → `"reverted"`. Tests updated.
  **Process fixes:** Covered by implementation checklist item #57 (type discriminator value conformance — verify string discriminator values match spec-defined values and existing serde conventions) and `scripts/check-type-discriminator-values.sh` (pre-commit enforced).

- [-] [process-revision-complete] **F5: Cross-workspace filter used `created_at` which cannot detect target changes.** — Fixed in `bdd23253`. Filter now includes `stale_since` disjunction; title changed to avoid "updated" claim; timestamp prefers `stale_since` when available.
  **Process fixes:** Covered by implementation checklist item #58 (timestamp semantic verification — read field's assignment sites, identify what event triggers the assignment, compare with spec's intended trigger condition, prefer purpose-built fields like `stale_since`).

## R3 Findings

- [-] [process-revision-complete] **F6: Missing `source_workspace_slug` field in cross_workspace items — spec-excerpt field omission.** — Fixed in `bb039b3d`. `source_workspace_slug: Option<String>` added to `BriefingItem`. Populated from `target_display` via `strip_prefix('@')` extraction (`graph.rs:908-912`). Tests assert on expected value `"platform-core"`.
  **Process fixes:** Covered by implementation checklist item #54 (spec-excerpt response schema field completeness).

- [-] [process-revision-complete] **F7: `stale_since` disjunction added in R2 has zero test coverage — untested code path.** — Fixed in `bb039b3d`. Two dedicated tests added: `briefing_cross_workspace_stale_since_includes_recently_stale_link` (created_at=500 < since=1500, stale_since=2000 >= since → included, timestamp=2000) and `briefing_cross_workspace_stale_since_excludes_old_stale_link` (both before since → excluded).
  **Process fixes:** Covered by implementation prompt Needs-Revision Workflow Step 2b (test coverage for fix-round code changes — new conditional branches require dedicated positive and negative tests).

## R4 Verification

All R1–R3 findings verified resolved. Systematic re-review of the full implementation found no new findings:

- **Cross-workspace filter direction:** source IN workspace, target NOT IN workspace ✓
- **Time scoping:** `created_at >= since || stale_since >= since` ✓
- **source_workspace_slug extraction:** correct `@slug/...` parsing ✓
- **Gate failure inner filter:** `finished_at >= since` prevents old-failure leakage ✓
- **Spec assertion failures:** type/workspace/timestamp filters correct ✓
- **MR reverts:** `revert()` updates `updated_at` (domain model line 117), filter sound ✓
- **MCP parity:** delegates to shared `assemble_briefing`, three-step `since` resolution matches REST ✓
- **Test coverage:** 10 dedicated unit tests + 3 integration tests + 4 MCP tests cover positive, negative, and edge cases ✓

---

## R5 Independent Review (candidate f8e99449)

**Reviewer:** Independent review agent
**Candidate:** `f8e99449ba5b7e485631b284d74e9a607f30e881` (base `078ec12a`)
**Verdict:** `needs-revision` (3 findings)

**Scope note:** the candidate diff is docs-only (`specs/tasks/task-013.md` +52
lines, `## Shipped` notes). The production implementation rides the assigned
base via mainline merge `d7940e85` (PR #627) — verified: `d7940e85` is an
ancestor of the base, its parent still contains the §9 stubs, and it carries
the implementation. The findings below are defects in that implementation as
it stands in the candidate tree; they were not surfaced in R1–R4.

**Verified correct (independent read):** cross-workspace filter direction and
semantics (`source_repo_id` = declaring spec, `target_repo_id` = external
dependency — matches `SpecLinkEntry` and the HSI §9 narrative), `stale_since`
disjunction and timestamp preference, `source_workspace_slug` extraction,
`spec_path` = local affected spec, gate-result inner `finished_at >= since`
filter, MR-revert scoping, empty sections as `[]`, MCP parity via shared
`assemble_briefing`, and substantive positive/negative unit tests against real
mem-adapter state. `cargo check -p gyre-server --lib --tests` passes; the
sandbox wrapper blocks gyre-server test execution, so
`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib briefing_` (claimed 26
passed) is recorded for the verifier. Evidence: `/tmp/stage/review-evidence/`.

### Findings

- [ ] **F1 (code, blocking): assertion-failure exceptions duplicate per notified member.**
  `graph_extraction.rs:1398-1420` fans one `SpecAssertionFailure` notification
  out to every Admin/Developer/Owner member (identical title/body/
  `source.entity_id`/`created_at`, distinct ids/user_ids).
  `graph.rs:1290-1310` maps every matching notification to an exception item
  with no dedup by source event, and `list_recent` returns all users'
  notifications (mem.rs:2568; sqlite/postgres notification.rs:294). The UI
  renders each item (Briefing.svelte:141-142). One failure in a workspace with
  N qualifying members yields N identical `exceptions` entries. Repro probe:
  create two SpecAssertionFailure notifications in one workspace for different
  user_ids with the same entity_id/created_at; `assemble_briefing` returns 2
  exception items. Fix: dedup by (source.entity_id, created_at) or query one
  representative per failure event.

- [ ] **F2 (code, blocking): gate failures silently missed when the MR's last update precedes `since`.**
  `graph.rs:1258-1261` requires BOTH `mr.updated_at >= since` (outer) AND
  `gr.finished_at >= since` (inner). Gates run asynchronously after enqueue
  (merge_queue.rs:89; enqueue does not touch the MR) and the failure path
  never updates the MR record (gate_executor.rs completion only writes
  gate_results/notifications/attestations; merge_processor.rs:1736-1752 only
  fails the queue ENTRY). So an MR approved days ago, enqueued and failed
  today, is excluded from a 24h briefing even though the failure is fresh —
  the inner filter is the correct time gate and the outer MR filter defeats
  it. No test covers `mr.updated_at < since <= gr.finished_at`. Repro probe:
  MR with updated_at=1000, failed gate result finished_at=2000, since=1500 →
  currently 0 gate_failure items.

- [ ] **F3 (code, blocking): `list_recent(200)` global cap silently drops assertion failures.**
  `graph.rs:1290-1293` reads the 200 most recent notifications across ALL
  users and workspaces (mem/sqlite/postgres `list_recent` truncate the whole
  table by created_at). Under load, a SpecAssertionFailure with
  `created_at >= since` can fall outside that window and be silently omitted
  from `exceptions`. The notification port has no workspace+type+since-scoped
  query; the fix needs one (port + adapters) or an equivalent unbounded
  scoped scan. Repro probe: seed 201 newer notifications of other types after
  a SpecAssertionFailure → exception missing despite `created_at >= since`.

### Non-blocking observations

- The `## Shipped` notes reference a retained test log at
  `/tmp/stage/review-evidence/task-0....log` from a prior branch checkpoint;
  that file does not exist in this stage. The verifier must re-run the
  briefing suite rather than trust the cited log.
- Working-tree note: `specs/tasks/task-013.md` in the checkout differs from
  the candidate HEAD (the stage restored the assigned contract text); the
  verdict targets the committed candidate `f8e99449`, which keeps both the
  assigned contract and the Shipped notes.
