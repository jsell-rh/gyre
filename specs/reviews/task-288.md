# Review — task-288 (Spec Links stored link statuses: broken / conflicted with reversion)

Spec: `specs/system/spec-links.md` §Link Status (spec gap `b222f4b88e85892d669131d7`).
Candidate under review: `7c1a3d2aabb5b009015613088e3bef64c602c905` (branch
`pipeline/task-288/7815440e3fc44b048622cb64a9be2c3a-2`, base
`ba2d16e3d3f60c4acb1c37c046e55ee867d3fdc0`; product commits `6a4b091b` +
`92850243`, both recorded in the candidate's `commits:` frontmatter).
Verdict: **approved** — all four status behaviors and both reversions are
real production code with write-through persistence; the six tests are
kill-tested against the pre-fix behavior; type check, formatting, and all
relevant mechanical invariant scripts pass. Test execution is deferred to
the independent verifier (executor wrapper rejects gyre-server test
codegen in this sandbox — infrastructure, not a code defect); exact
commands recorded below and in `/tmp/stage/review-evidence/README.md`.

## Round 1

Probes run (checkout at candidate `7c1a3d2a`; the working tree carries
only the stage's reset of `specs/tasks/task-288.md` to the assignment
state, which is not part of the product diff):

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests`
  → **exit 0, zero warnings** (cold 8m19s, cached 0.42s).
- `/tmp/stage/bin/cargo fmt -p gyre-server -- --check` → exit 0.
- `cargo test -p gyre-server --lib -- <six tests>` → exit 64, rejected by
  the build wrapper ("Server builds/tests belong to the independent
  verifier") — recorded, not a defect.
- Invariant scripts, all exit 0: `check-arch.sh`,
  `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`,
  `check-lossy-secret-conversion.sh`, `check-inert-enforcement.sh`.
- `check-task-commit-attribution.sh`: fails only against the stage's
  working-tree reset of task-288.md; **exit 0 against the committed
  candidate tree** (verified via a temporary stash, restored afterwards).
- Full line-by-line read of all three source diffs (+1276/−107) and both
  spec-file diffs.

Verified against the task contract:

- **Single status oracle.** `evaluate_link_status` (spec_registry.rs)
  computes conflicted (conflicts_with + both endpoints ledger-Approved) →
  broken (target absent from ledger OR `ManifestPathPresence::Absent` in
  the target repo's current manifest, read by git-showing
  `specs/manifest.yaml` at `refs/heads/{default_branch}`; `Unknown` falls
  through to the SHA rules — no breaking on undeterminable state) → stale
  (pinned ≠ current, empty current excluded) → active. Supersedes-deprecated
  targets still declared in the manifest are not broken (approval status
  is not an input to the broken rule).
- **Broken, push-time (AC 1).** `sync_spec_ledger` step 5b breaks inbound
  links whose target resolves to the pushed repo but whose path the pushed
  manifest no longer declares (unresolved links excluded; cross-repo
  targets scoped by `target_repo_id`, same-repo by `source_repo_id`),
  written through via `link_repo.save`. The ledger's save-not-delete
  deprecation is explicitly asserted by the test.
- **Broken reversion (AC 5, the ratchet defect).** The daily pass's
  `status == "stale" || status == "broken" → continue` guard is gone;
  `run_once` re-evaluates every non-unresolved link through the oracle, so
  a target re-entering its manifest (or the ledger) transitions the link
  back to active/stale per the SHA rules. `stale_since` is preserved on
  no-transition and cleared on exit from stale (`write_link_status`).
- **Conflicted (AC 2).** Three producers: (1) stored at sync — set on
  `new_links` before the store push and `replace_for_source` persistence;
  (2) approval changes — `approve_spec`/`revoke_spec` call
  `refresh_conflicts_for_spec` after the ledger persist (revoke after the
  §9 recompute; the supersedes-deprecation loop also refreshes for the
  deprecated target), and sync reconciles every approval change it makes
  (content invalidation, deprecation, re-Pending on re-add, extends drift)
  via `approval_changed_paths` → `refresh_conflict_statuses_for_spec`;
  (3) the daily pass. `GET /api/v1/specs/conflicts` runs
  `reconcile_conflict_statuses` over every conflicts_with link before
  filtering `status == "conflicted"`, so endpoint and graph cannot drift;
  the pre-existing `get_conflicts` tests remain consistent (link-3
  connects two Approved specs; the Pending-pair test stays excluded).
- **Conflicted reversion (AC 4).** `reconcile_conflict_statuses` reverts
  a stored conflicted whose both-approved condition ceased to whatever the
  active/stale/broken rules yield — invoked from the daily pass, approve,
  revoke, every sync-time approval change, and the conflicts endpoint.

Test-strength check (no inflation): all six claimed tests exist and each
asserts BOTH the hot cache and the durable `spec_links` table (the API
test asserts via `GET /specs/{path}/links` and `GET /specs/conflicts`).
Each fails against the base implementation: "conflicted" had no producer;
manifest removal left links active through the `Ok(Some(entry))` path; and
the `broken → continue` guard skipped re-evaluation forever. The
verification-intent probes (declare conflicts_with after both Approved;
remove B's manifest entry; re-add and re-run the pass; reset an endpoint
to Pending) map one-to-one onto the four probe tests. The one modified
pre-existing test (restart durability) was legitimately updated to declare
the parent in the manifest and pin the real blob SHA — necessitated by the
new (correct) push-time broken rule; its assertions are not weakened.

Coverage: only row 6 (Link Status) of specs/coverage/system/spec-links.md
updated, from the stale Partial note to implemented with accurate source
evidence and an explicit "Pending independent review for verified" — no
unrelated rows touched, no overclaiming.

Minor observations (non-blocking, recorded for completeness):

- Step 5b (push-time broken) sets `broken` unconditionally, including for
  a conflicts_with link whose endpoints are both Approved, while the
  oracle's precedence is conflicted-first. The degenerate corner (a
  conflicts_with link whose target leaves the manifest while both
  endpoints stay Approved) transiently holds `broken` until the next
  daily pass / conflicts-endpoint reconcile converges it to `conflicted`.
  Neither the spec nor the task defines precedence for the corner; the
  graph converges to the oracle.
- Step 5b compares `manifest_paths.contains(&link.target_path)` raw,
  while the oracle normalizes both sides (`normalize_spec_rel_path`).
  Manifest convention is prefix-less (sync itself prefixes `specs/` when
  reading files), so the divergence is theoretical.
- `GET /api/v1/specs/conflicts` is now state-mutating (reconcile writes
  through). Documented in-code; intentional, so endpoint and graph agree.

Verifier must run (executor wrapper rejects gyre-server test codegen in
this sandbox; 4 GiB budget):

    cargo test -p gyre-server --lib -- \
      staleness_pass_tracks_target_manifest_presence_both_ways \
      staleness_pass_does_not_break_deprecated_target_still_in_manifest \
      staleness_pass_sets_and_reverts_conflicted_status \
      sync_marks_inbound_links_broken_when_target_leaves_manifest \
      sync_stores_conflicts_with_link_as_conflicted_when_both_approved \
      conflicted_status_set_on_approval_and_reverted_on_revoke

Plus the regression surface: `cargo test -p gyre-server --lib --
spec_link_staleness`, `... -- get_conflicts`, `... -- sync_`, and the
workspace suite per the standard verifier protocol.
