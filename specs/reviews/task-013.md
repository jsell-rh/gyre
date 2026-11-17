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

## R5 Review (independent, candidate b8597270)

**Reviewer:** Independent review agent
**Round:** R5 (assignment `70ddd158068840978a86060e34e435be`)
**Candidate:** `b8597270fe741cdc836ef4c3a9e46bdf3069c4fa` (base `9f446d0e`)
**Verdict:** approved (0 blocking findings)

### Scope

Diff from assigned base is the R4 production fix (`a09baf85`) plus docs-only
commits; the working tree matches `a09baf85` exactly for `crates/` and `web/`
(verified: `git diff a09baf85..b8597270 -- crates/ web/` is empty). Review
covered the R4 fix and re-verified the full task contract (all six acceptance
criteria) against the assembled implementation.

### Verified

- **Gate-failure time gate (F2 fix):** outer `mr.updated_at >= since` filter
  removed; `gr.finished_at >= since` is the sole time gate
  (graph.rs:1360-1375). Correct: `GateResult` failure path never touches
  `mr.updated_at`, and the inner `finished_at` filter still excludes old
  failures (`briefing_exceptions_gate_failures_excludes_old_gate_results`
  covers that direction).
- **Scoped notification query (F1/F3 fix):** new port method
  `NotificationRepository::list_by_workspace_type_since(ws, type, since)` —
  workspace+type+since scoped, all users, unbounded. Implemented by all four
  implementors (SQLite, Postgres, Mem, BroadcastingNotificationRepo) with
  consistent semantics; `cargo check` confirms no missing impl.
- **Fan-out dedup:** verified against the producer
  (graph_extraction.rs:1373-1444): per-member copies share title, `created_at`
  (computed once before the member loop), and `source.entity_id`
  (first_spec_path) — the `(entity_id, title, created_at)` key collapses
  exactly the fan-out and keeps distinct events (different `created_at`).
- **Tests kill the bugs they target** (traced each against pre-fix code):
  stale-MR gate failure (would count 0 under the old outer filter), buried
  assertion failure (201 newer global notifications push it out of the old
  `list_recent(200)` window), fan-out dedup (3 notifications → 2 items).
- **Executed probes** (logs in `/tmp/stage/review-evidence/`):
  - `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` — exit 0.
  - `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` — exit 0
    (briefing test code type-checks at this HEAD).
  - `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib sqlite::notification`
    — **10 passed, 0 failed**, including
    `list_by_workspace_type_since_scopes_and_is_unbounded`.
  - `scripts/check-nested-time-scope.sh`, `check-type-discriminator-values.sh`,
    `check-mem-port-contracts.sh` — all pass.
- **Contract:** assigned `## Git Commits` section byte-identical; R4 commit
  attributed via `commits:` frontmatter; docs-only commits touch no product
  surface, so attribution check scope is satisfied.
- **Cross-workspace re-verified:** directional filter (source IN workspace,
  target NOT IN), `created_at >= since OR stale_since >= since` time scope,
  `source_workspace_slug` parsed from `target_display` (`@slug/...`),
  repo-filter aware; four dedicated tests (populated / same-workspace
  excluded / stale-included / stale-excluded).

### Deferred to verifier (executor wrapper blocks gyre-server test compilation)

```
SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib briefing_
SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib sqlite::notification
```

### Non-blocking observations (recorded, not candidate defects)

1. **Pre-existing frontend/backend schema divergence (in base, outside this
   diff):** `web/src/components/Briefing.svelte` consumes
   `item.mr_id`/`item.repo`/`item.spec_ref` and metrics
   `mrs_count`/`runs_count`/`cost_usd`, while the backend `BriefingItem`
   serializes `entity_id`/`spec_path` and metrics `mrs_merged`/`gate_runs`/
   `budget_spent_usd`. Items still render via `description` fallbacks, but
   exception action buttons (gated on `item.mr_id`) and metrics cells never
   display against the real API. Both sides are unchanged by this candidate;
   the fix belongs to a frontend-surface task, not this backend-assembly task.
2. `assemble_briefing` uses `unwrap_or_default()` on repository queries
   (fail-open on DB error → empty sections). Pre-existing handler-wide
   pattern for this display path, retained by the new query for consistency.
3. Dedup key `(entity_id, title, created_at)` could merge two genuinely
   distinct same-second failure events with identical title and spec — an
   acceptable heuristic edge case (events are indistinguishable at that
   resolution).
