# Review — task-288 (Spec Links stored link statuses: broken / conflicted with reversion)

Spec: `specs/system/spec-links.md` §Link Status (spec gap `b222f4b88e85892d669131d7`).
Candidate under review: `9d02bcdc` (branch `pipeline/task-288/0b3a573d5aee43b6861bfe5f43bbd18c-1`, base `f0f2ba5d`; product commits `6a4b091b`, `92850243`, `763671fd` — all attributed in the task frontmatter).
Verdict: **approved** — all five acceptance criteria are implemented in real production code with real tests; probes and evidence under `/tmp/stage/review-evidence/` (summary.md).

## What was verified

**Single status oracle.** `evaluate_link_status` (spec_registry.rs) computes the status a link row must hold right now, in the order conflicted (conflicts_with link with both endpoints ledger-Approved) → broken (target absent from the ledger OR absent from its repo's current manifest, read via `target_manifest_path_presence` git-showing the target repo's `specs/manifest.yaml` at `refs/heads/{default_branch}`) → stale (pinned `target_sha` ≠ ledger `current_sha`) → active. A supersedes-deprecated target still declared in its manifest is not broken (deprecation is not deletion); an unreadable manifest is tri-state `Unknown` and falls through to the SHA rules rather than breaking on undeterminable state.

**Producers, per the task's four delivery points:**
- (a) broken — push-time: `sync_spec_ledger` step 5b marks inbound links broken when the pushed manifest no longer declares the target path (skips `unresolved` links; scoping by `target_repo_id == this repo` or same-repo `source_repo_id == this repo`), written through to the durable table via `spec_link_repo.save`. Daily pass: `run_once` calls the oracle for every non-unresolved link. The ledger-keeps-Deprecated ratchet is defeated because the manifest, not the ledger, drives the removal half.
- (b) broken reversion — the old `status == "stale" || status == "broken" → continue` guard is gone; `run_once` re-evaluates every status each pass and `write_link_status` clears `stale_since` when leaving stale.
- (c) conflicted — set at sync store time (both-approved check before `replace_for_source` persists), on approval (`approve_spec` → `refresh_conflicts_for_spec`, including the supersedes-deprecation side effect for each deprecated target), on revoke (`revoke_spec_approval`), on sync-time approval resets (content invalidation, deprecation, re-Pending re-add, extends drift — all collected in `approval_changed_paths` and reconciled in step 6c), and in the daily pass.
- (d) conflicted reversion + endpoint agreement — `reconcile_conflict_statuses` reverts a stored `conflicted` whose condition ceased to whatever the active/stale/broken rules yield; `GET /api/v1/specs/conflicts` runs the reconciliation over every `conflicts_with` link BEFORE filtering `status == "conflicted"`, so the endpoint and the stored graph cannot drift.

**Tests (reviewed line-by-line; execution deferred to the verifier per the 4 GiB budget rule).** Six tests cover the four verification probes, each driving real production code: `staleness_pass_tracks_target_manifest_presence_both_ways` (real git repo, real manifest removal + re-add, asserts broken then active in hot cache AND durable table — kills both the never-broken and never-revert defects), `staleness_pass_does_not_break_deprecated_target_still_in_manifest` (AC 3), `staleness_pass_sets_and_reverts_conflicted_status`, `sync_marks_inbound_links_broken_when_target_leaves_manifest` (push-time half of AC 1, also asserts the ledger keeps the target as Deprecated), `sync_stores_conflicts_with_link_as_conflicted_when_both_approved`, and `conflicted_status_set_on_approval_and_reverted_on_revoke` (real HTTP router: approve → stored conflicted + listed by `/specs/conflicts`; revoke → delisted + reverted). The pre-existing restart-parity test was adapted to the new semantics (real pinned blob SHA, parent declared in the manifest) and still verifies its original intent. No assertion weakened; no self-confirming or stub-backed test found.

## Probes run

- `git diff f0f2ba5d..9d02bcdc` — full read of all 5 changed files.
- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → exit 0 (10m10s).
- `/tmp/stage/bin/cargo clippy -p gyre-server --lib` → exit 0; zero findings in the three changed files — the five findings cited by verification attempt `a2055071d9874d04a6774fd93c7a22bb` are all fixed by `763671fd` (doc-list continuation indentation; extends sync match collapsed to `if`), behavior-preserving on inspection.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`, `check-inert-enforcement.sh`, `check-mem-port-contracts.sh` → all exit 0.

## Verifier must run (test-target compilation exceeds this sandbox's budget)

    cargo test -p gyre-server --lib -- \
      staleness_pass_tracks_target_manifest_presence_both_ways \
      staleness_pass_does_not_break_deprecated_target_still_in_manifest \
      staleness_pass_sets_and_reverts_conflicted_status \
      sync_marks_inbound_links_broken_when_target_leaves_manifest \
      sync_stores_conflicts_with_link_as_conflicted_when_both_approved \
      conflicted_status_set_on_approval_and_reverted_on_revoke

plus the regression surface: `cargo test -p gyre-server --lib -- spec_link_staleness`, `-- sync_`, `-- get_conflicts`, `-- restart`, and the verification clippy gate.

## Minor (non-blocking, recorded for completeness)

- Push-time step 5b compares the link's `target_path` against `manifest_paths` with an exact string match, while the daily pass normalizes both sides (`normalize_spec_rel_path`). The two can diverge only if a manifest's entry path and a link target string differ in `specs/`-prefix form — a mixed form that already fails the pre-existing ledger `find_by_path` lookup the same way, and step 5/6b use the same exact-match convention. Not a new inconsistency class.
- In the daily pass, a target repo row that cannot be resolved (or a manifest that cannot be read) falls through to the SHA rules — a documented fail-open choice so links are not broken on undeterminable state. In a normal deployment the repo row exists (a push went through it), and push-time 5b does not depend on the row. `ManifestPathPresence::Unknown` is deliberately distinct from `Absent`; only a readable, non-declaring manifest evidences "no longer in the manifest".

Coverage row 6 (Link Status) updated to `implemented` with source evidence matching the code as reviewed; no unrelated rows touched; the "pending independent review" phrasing is honest. Task frontmatter progress `ready-for-review` with all three product commits attributed.
