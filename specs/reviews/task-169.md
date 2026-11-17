# Review — task-169 (Spec registry — supply chain AIBOM integration)

Spec: `specs/system/spec-registry.md` §"Supply Chain (supply-chain.md)".
Commits under review: `a4c24fc1` (feat), `ad473d3c` (fmt/clippy repair); process commits `6b0503c2`…`2604b438` carry task state only.
Candidate: `2604b438811481d508e618492e0c6df1d978ec52` (base `ce336114`).
Verdict: **needs-revision** — one material finding (F1: merge-time approval misreporting on post-merge withdrawal); the rest of the implementation is genuine and well-built.

## Probes run (review sandbox)

- `cargo check -p gyre-server --tests` → **exit 0** (9m31s cold, single job). Candidate including all test code type-checks. Log: `/tmp/stage/review-evidence/cargo-check-gyre-server.log`. (gyre-server test *execution* is deferred to the independent verifier per stage budget.)
- Focused behavioral probe on the smaller `gyre-domain` package: a temporary integration test replicating the **exact** `approved_at_merge_time` predicate from `aibom_spec_registry.rs::build_code_bindings` and driving it with the real `SpecApproval` lifecycle (`approve(900)` → merge at 1000 → `revoke(..., 2000)`). All 3 probe tests passed; the key one asserts the predicate returns `false` for an approval that was active when the merge gate ran and withdrawn only afterwards. Source + output saved in `/tmp/stage/review-evidence/probe-test-source.rs`, `probe-run-output.txt`, `probe-predicate-source.txt`. Probe file deleted after the run; working tree otherwise untouched.

## Verified working (no findings)

- **Real data, no stand-ins.** `aibom_spec_registry.rs::build_spec_registry_section` reads real storage: `spec_ledger.list_all()` (scoped by `repo_id`, cross-repo entries excluded — tested), `spec_approval_history.list_by_path` (production `POST /specs/:path/approve` records these events), `spec_approvals.find_by_spec_sha` (the same per-SHA ledger `verify_spec_ref` enforces at the merge gate — confirmed in `api/gates.rs:819`), `merge_requests.list_by_repo`, and real git via the pre-existing `git_refs::resolve_blob_sha` (injection-guarded `git rev-parse HEAD:specs/manifest.yaml`; tested against a real temp git repo). Storage failures propagate (`?` → `ApiError::Internal`) — no fail-open under-reporting. `manifest_version` is reported `null` when absent, never fabricated.
- **All ledger statuses and the approval chain.** Approved/Pending/Deprecated/Revoked/Rejected all counted and emitted per-spec (`status`, `approval_mode`, `drift_status`, `current_sha`); approval chain (approver ids, earliest `approved_at`, full `approval_chain` with type/persona) built from active events for the current SHA — stale-SHA and revoked events excluded (tested). Drift risk per spec-registry.md (`drifted` count) included.
- **Both surfaces share one builder.** `GET /repos/:id/aibom` (`aibom_version` 1.1 → 1.2, doc updated) and `POST /release/prepare` both embed the same `build_spec_registry_section` output, so AIBOM and release metadata cannot disagree. No new routes; both handlers sit behind their existing ABAC mappings. Frontend clients (`web/src/lib/api.js`) are untyped JS and consume the response additively — no breakage; `web/dist` regeneration is owned by the verifier (separate observation, non-blocking per stage rules).
- **Tests are meaningful, not self-confirming.** The unit tests drive the real mem-backed `AppState` and kill real bugs: cross-repo ledger leakage, revoked/stale approval-event exclusion, approved-AFTER-merge → `Some(false)` (mr-2 — fails under a "currently approved" check), revoked-BEFORE-merge → `Some(false)` (mr-3), open-MR → `None`, reverted-MR → `None` (not evaluated against the revert timestamp), cross-repo MR exclusion, real-git manifest SHA. Endpoint tests assert the JSON wire format on both routes.
- `cargo check -p gyre-server --tests` passes; no new migrations, routes, MCP tools, MessageKinds, or AppState stores, so the mechanical guards are unaffected by this diff.

## Findings

- [F1] **`approved_at_merge_time` misreports whenever a spec approval is withdrawn AFTER the merge — which is the ordinary spec-evolution path.** `crates/gyre-server/src/aibom_spec_registry.rs` (`build_code_bindings`, ~line 299) evaluates:

  ```rust
  a.approved_at.map(|t| t <= merged_ts).unwrap_or(false)
      && a.revoked_at.map(|t| t > merged_ts).unwrap_or(true)
      && a.rejected_at.map(|t| t > merged_ts).unwrap_or(true)
  ```

  But `SpecApproval::revoke()`/`reject()` **clear `approved_at`** (mutual exclusivity — `gyre-domain/src/spec_approval.rs`: "clear other timestamp columns"; both the mem and sqlite adapters apply the domain transition). So for every revoked/rejected row `approved_at` is `None`, the first conjunct is `false`, and the `revoked_at > merged_ts` conjunct is **dead code**. Consequence: an MR whose pinned spec had an active approval when the merge gate ran (gate passed) reports `approved_at_merge_time: Some(false)` once the approval is later withdrawn. This contradicts the module's own documented semantics — "An approval counts when it was granted at or before the merge and was not withdrawn (revoked/rejected) at or before the merge — i.e. it was active at the moment the gate ran" and "the exact check `verify_spec_ref` performs when the merge gate runs" — and it is systematically reachable: `git_http.rs` (~line 2906) auto-calls `revoke_all_for_path` on every push that modifies/deletes/renames a `specs/` file, and `POST /specs/:path/revoke` is a manual route. In a repo whose specs evolve, every merged MR pinned to a since-superseded spec version is reported as not-approved-at-merge-time, corrupting `bindings_summary.approved_at_merge_time` for the release artifact. Reproduced by the probe above (approved@900, merged@1000, revoked@2000 → predicate `false`). The shipped test covers revoked-BEFORE-merge (mr-3) but not revoked-AFTER-merge. Repair: for a withdrawn row whose `revoked_at`/`rejected_at` is after the merge, merge-time approval is not reconstructible (the grant timestamp was cleared) — report `None` (indeterminable), exactly as the reverted-MR case already does honestly; keep `Some(false)` only for withdrawals at/before the merge. Add a test for the approved@900 / merged@1000 / revoked@2000 case.

## Deferred checks for the independent verifier

- `cargo test --all` (task AC).
- `cargo test -p gyre-server aibom_spec_registry` (3 unit tests) and the endpoint tests `aibom_includes_spec_registry_section`, `release_prepare_includes_spec_registry_metadata`.
- Standard guards: `scripts/check-arch.sh`, `scripts/check-abac-route-registry.sh`, `scripts/check-abac-exempt-handlers.sh`, `scripts/check-task-commit-attribution.sh` (diff touches no routes/migrations/MCP/MessageKinds, so these should be unaffected).
- Rebuild `web/dist` from merged sources (verifier-owned).

## Minor (non-blocking, recorded for completeness)

- Ledger entries whose `repo_id` is still `None` (predating the sync backfill) are invisible to the section until the repo's next push re-syncs them — a transitional artifact of the pre-existing global, path-keyed ledger design (two repos sharing a manifest path share one entry owned by the first pusher). Not introduced by this task.
- `list_by_path` per spec entry is O(specs) queries per AIBOM build — fine at current scale, worth noting if the ledger grows.
