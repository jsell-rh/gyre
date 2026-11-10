# Review — task-175 (Specs and Inbox view layouts — spec list, inbox cards, briefing narrative)

Spec: `specs/system/ui-layout.md` §6-§8, `specs/system/human-system-interface.md` §8 priority table.
Base `89025d728` → candidate `96908da2536e3c76936c9bf928f0ec80a813022d`.
Verdict: **approved** — no findings.

## Scope of this round

The full §6-§8 surface (SpecDashboard sortable table + detail tabs + editor split,
all-10-priority Inbox cards, Briefing narrative) was built and reviewed in earlier
rounds and is present in the assignment base. This candidate adds the final
verification-gate repairs (commits `db5b4f822`, `956897081`) on top of the reviewed
P2 fan-out / stale-`spec_sha` refresh / MR-list enrichment work (`98f6c234` and
earlier): the E0063 stale `AuthenticatedAgent` initializer (main's task-108 added
`impersonation`), rustfmt on the new SQLite test's assert lines, and the
repo-standard clippy allows on the `NotificationRepository` port trait. No product
behavior changed in the final two commits.

## Probes run (evidence: /tmp/stage/review-evidence/evidence-log.md)

- **Hardcoded SHA anchors verified externally**: `git hash-object` over the exact
  test contents reproduces all three asserted blob ids (`8035a516…`, `98537aa0…`,
  `1a862bdc…`) — the spec_sha assertions anchor to real git blob ids, not mirrored
  logic.
- `cargo test -p gyre-server --lib api::specs_assist` — **23 passed** (includes the
  repaired fan-out initializer; the lib test target compiles, proving the E0063 fix).
- `cargo test -p gyre-adapters --lib sqlite::notification` — **2 passed**.
- `cargo test -p gyre-server --lib api::merge_requests::tests::list_mrs_enriches_merged_rows_with_merge_commit_sha` — **1 passed**.
- **Mutation probe**: redirecting the append-path `update_body_by_entity_ref` call
  to a nonexistent entity (production refresh disabled at runtime) makes
  `save_spec_existing_mr_refreshes_notification_spec_sha` **fail** with the stale
  v1 SHA — the F1 test discriminates, it is not self-confirming. Source restored;
  suite re-run green.
- Frontend (after `npm ci`): `Inbox.test.js` **74/74**, `Briefing/AccordionItem/
  AccordionScoping` **43/43**, `SpecDashboard.test.js` **27/27**. Spot-check
  confirms behavior-asserting tests with negative assertions (fail-closed
  `missing_spec_sha` path asserts `approveSpec` NOT called; Pick A/B assert
  `createTask` payload names the losing side and `mergeRequests` NOT called).
- `check-rustfmt-diff.py 89025d728` — changed lines clean (7 files).
- Focused clippy (wrapper blocks workspace-wide `--all-targets`): `gyre-ports`,
  `gyre-adapters`, `gyre-server` lib — **zero warnings in any changed file**
  (`notification.rs` ×3, `specs_assist.rs`, `merge_requests.rs`); the mem.rs /
  spec_registry.rs warnings are on pre-existing lines outside the diff's changed
  line ranges (verified with `--unified=0`).
- Mechanical checks: task-commit attribution, mem port contracts, arch,
  fail-open-ref-resolution, inert-enforcement — all OK. Zero new routes in the
  diff (no ABAC registry implications).

## Verified clean

- `update_body_by_entity_ref` is implemented uniformly across SQLite, PostgreSQL,
  and mem adapters with the identical unresolved/undismissed/entity/type guard
  (mem guards in code per the port-contract invariant); the SQLite test covers
  every guard branch.
- `spec_blob_sha` never returns `""` (SHA-1 git-blob-id fallback), closing the
  empty-`spec_sha` fail-open; `resolveSpecSha` in Inbox.svelte fails closed
  (`missing_spec_sha`) when the body lacks a SHA and the spec-edit MR is open.
- The P2 fan-out delivers to eligible Admin/Developer/Owner members with a
  `system` fallback only when none exist (test asserts the Viewer gets nothing
  and the fallback is unused when members exist).
- Inbox per-type action rows cover all 10 HSI §8 priority types plus the extended
  `NotificationType` variants; P3 Override records both the review and the
  MR-status approval; P5 Pick/Reconcile create real tasks via the task domain.
- No fakery indicators: no bail-stubs, no audit-only enforcement, no fabricated
  scope defaults, no new exemptions.

## Deferred to verification

The sandbox wrapper rejects workspace-wide `cargo clippy --all-targets
--all-features` (and therefore `check-clippy-diff.py`'s internal invocation) and
the full `checks.sh` gate; the changed-line clippy claim was verified with
focused per-package probes instead. The verifier should run the standard gate
suite (`checks.sh`) to confirm the failed run from candidate `daee0c8d` is fully
repaired at `96908da25`.
