---
title: "Implement spec approval ledger with database schema and API"
spec_ref: "agent-gates.md §Part 2 Spec Approval Ledger"
depends_on: [task-242]
progress: complete
coverage_sections:
  - "agent-gates.md §Spec Approval Ledger"
  - "agent-gates.md §The Provenance Chain"
  - "agent-gates.md §How It Works"
commits: ["59d31f8d8e25b2b2eb0cb52f7675fff45073e71a", "c7e74aa50d9a175b7727b7d79350121c29d1baec", "2d54e1d93022730f9c10ec9003c622587cf4d110", "b27d3787d950babde1dfc9469ea2dce4a028ecb0", "30e80d9504f0ae390819ab593c460693f08bcee7", "2f424b88f966e26d656ecac0d382586f26bcc705", "fc2da8ab3711a571abfbc4901ae01fc00047fb80", "72f61cbdc55fab1c8a593b2961db695e0804dcd2"]
---

## Spec Excerpt

From `agent-gates.md` §Spec Approval Ledger:

```sql
spec_approvals table:
  id              TEXT PRIMARY KEY
  spec_path       TEXT NOT NULL        -- e.g., "specs/system/identity-security.md"
  spec_sha        TEXT NOT NULL        -- git blob SHA at approval time
  approver_id     TEXT NOT NULL        -- user or agent who approved
  signature       TEXT                 -- Sigstore signature (optional but recommended)
  approved_at     INTEGER             -- NULL when status is Pending
  revoked_at      INTEGER             -- NULL unless revoked
  revoked_by      TEXT
  revocation_reason TEXT
  rejected_at     INTEGER             -- NULL unless rejected
  rejected_by     TEXT
  rejected_reason TEXT
```

**ApprovalStatus enum:**
```rust
pub enum ApprovalStatus {
    Pending,    // no timestamp columns set
    Approved,   // approved_at is set
    Revoked,    // revoked_at is set (post-merge withdrawal)
    Rejected,   // rejected_at is set (pre-merge decline)
}
```

**Status Logic:** Status derived from which timestamp column is non-null. Mutual exclusivity: handler clears all other timestamp columns when setting new status.

**Transitions:** Pending → Approved → Revoked OR Pending → Rejected. Revocation requires reason and is audited. Rejection closes associated MR. Multiple approvals can exist for same spec (different versions).

From §The Provenance Chain (9-step chain):
1. Spec authored (committed, gets git SHA)
2. Spec approved (reviewer signs spec version via `POST /api/v1/specs/approve`)
3. Task created (references `spec_ref: "path@sha"`)
4. Agent dispatched (context includes spec content at exact SHA)
5. Commits produced (include spec provenance in agent_commits)
6. MR created (references `spec_ref`)
7. Gate agent reviews (receives spec at pinned SHA, not HEAD)
8. Forge validates at merge (spec SHA exists, has approval, not revoked)
9. Merged (full provenance chain recorded)

## Implementation Plan

1. **Domain types in `gyre-domain` or `gyre-common`:**
   - `ApprovalStatus` enum: Pending, Approved, Revoked, Rejected
   - `SpecApproval` entity with all spec fields
   - Status derived from timestamp columns (computed property)

2. **Port trait in `gyre-ports`:**
   - `SpecApprovalRepository`: create, get_by_id, list_by_spec_path, find_by_spec_sha, update_status
   - Transition methods: approve, revoke, reject (enforce valid transitions)

3. **Database migration:**
   - Create `spec_approvals` table per schema above
   - Index on (spec_path, spec_sha) for lookup
   - Index on approver_id for audit queries

4. **SQLite adapter:**
   - Implement `SpecApprovalRepository`
   - Enforce mutual exclusivity: clearing other timestamps on status change
   - Support multiple approvals per spec (different versions)

5. **Enhance existing spec approval API:**
   - `POST /api/v1/specs/:path/approve` already exists — extend to create ledger entry with SHA
   - Add `POST /api/v1/specs/:path/revoke` for revocation (with reason)
   - `GET /api/v1/specs/approvals` already exists — extend to include full ledger data
   - Rejection: `POST /api/v1/specs/:path/reject` already exists — wire to ledger

6. **Spec SHA resolution:**
   - On approval: resolve spec file to its current git blob SHA
   - Store SHA in approval record
   - On query: return approval status per SHA

## Acceptance Criteria

- [ ] `spec_approvals` table with all columns per spec schema
- [ ] `ApprovalStatus` enum with Pending/Approved/Revoked/Rejected
- [ ] Status derived from timestamp columns (not stored directly)
- [ ] Mutual exclusivity enforced on status transitions
- [ ] Valid transitions: Pending → Approved → Revoked, Pending → Rejected
- [ ] Revocation requires reason, records revoked_by
- [ ] Multiple approvals per spec path (different SHAs) supported
- [ ] Spec approval creates ledger entry with git blob SHA
- [ ] Revoke endpoint with reason field
- [ ] `GET /api/v1/specs/approvals` returns full ledger data
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` Part 2 §Spec Approval Ledger and §The Provenance Chain. Existing spec approval: `gyre-server/src/api/specs.rs` (approve_spec, reject_spec handlers), routes at `gyre-server/src/api/mod.rs` lines ~353-358. Existing spec approval storage: grep for `spec_approval\|approve_spec` in adapters. Git SHA resolution: check how the codebase resolves file SHAs (likely in git operations or repo utils). Port pattern: look at existing ports in `gyre-ports/src/` for CRUD traits. Check migration numbering: currently at 000049.

## Shipped

Spec approval ledger implemented as a real port-backed store with enforced
lifecycle transitions, plus the repair for the contract finding.

- **Domain** (`gyre-domain/src/spec_approval.rs`): `SpecApproval` with the
  full spec schema; `ApprovalStatus` derived from which timestamp column is
  non-null (never stored); transition methods `approve`/`revoke`/`reject`
  enforce Pending → Approved → Revoked and Pending → Rejected, reject empty
  revocation reasons, and clear sibling timestamp columns for mutual
  exclusivity.
- **Port** (`gyre-ports/src/spec_approval.rs`): `SpecApprovalRepository` —
  create (fails on duplicate id), find_by_id, list_by_path,
  **find_by_spec_sha** (exact (path, sha) version query — plan step 2/6),
  list_active_by_path, list_all, transition methods, revoke_all_for_path.
- **Migration** `2026-10-08-000057_spec_approval_ledger` (renumbered from
  000056 after the task-180 landing claimed that version on main): nullable
  `approved_at` (NULL while Pending), dual-dialect SQLite rebuild + PG
  portability, indexes on (spec_path, spec_sha) and approver_id.
- **Adapters**: SQLite, Postgres, and mem implement the port with identical
  contracts (mem enforces the duplicate-id guard in code;
  check-mem-port-contracts.sh passes).
- **API**: `POST /api/v1/specs/:path/approve` validates the requested SHA
  against the ledger's current blob SHA (409 on mismatch, no partial writes)
  and creates + approves the ledger entry; `POST /api/v1/specs/:path/revoke`
  requires a reason, restricts to original approver or Admin, and audits;
  `POST /api/v1/specs/:path/reject` transitions Pending rows to Rejected and
  closes spec-edit MRs; `GET /api/v1/specs/approvals` returns full ledger
  rows with derived status/active.
- **Forge enforcement** (§The Provenance Chain step 8): `verify_spec_ref`
  queries the exact (path, sha) version via `find_by_spec_sha` and accepts
  only active (Approved, not revoked/rejected) rows; `require_current_spec`
  blocks merges on stale spec SHAs.
- **Push-time invalidation**: a push modifying a spec file revokes all
  active approvals for that path (ledger-path normalization so git paths
  match ledger rows).

**Contract repair** (finding 4f9f2afe1fda4061b89edd486f71331b): the previous
assignment's failure was bookkeeping, not code — commit 90880a97 checked the
Acceptance Criteria boxes with appended evidence, which changes the task's
requirement text (`scripts/dev-contract.py requirement_parts` keeps all prose
except `## Shipped`/`## Review`). This run restores the contract verbatim
(boxes unchecked; this section carries the evidence instead) and adds the
substantive repair above: the port method the plan names (`find_by_spec_sha`)
and the per-SHA query backing "on query: return approval status per SHA",
implemented across all three adapters with `verify_spec_ref` rewired to it.

Verification (this sandbox, focused suites; evidence in
`/tmp/stage/review-evidence/task-138-repair.md`):
- `cargo test -p gyre-adapters --lib sqlite::spec_approval` — 7/7 (incl. new
  `find_by_spec_sha_returns_only_that_version`).
- `cargo test -p gyre-server --lib -- api::gates` — 10/10 (incl. new
  `find_by_spec_sha_scopes_to_exact_version`: a Pending sibling SHA must not
  verify while another SHA of the same path is approved).
- `cargo test -p gyre-server --lib -- api::specs` — 73/73;
  `push_modifying_spec_revokes_ledger_approvals` +
  `require_current_spec_blocks_stale_spec_ref` — pass.
- `cargo test -p gyre-domain spec_approval` — 4/4.
- Mechanical invariants pass: migration-versions, arch, mem-port-contracts,
  inert-enforcement, migration-SQL-portability, ABAC route registry.

Sandbox limitation (recorded, not a code defect): the e2e test
`spec_approval_auto_invalidated_on_spec_change` needs a TCP listener;
`accept(2)` is blocked with EOPNOTSUPP errno 95 (`/tmp/stage/capabilities.json`).
The identical lifecycle is proven listener-free by
`git_http::tests::push_modifying_spec_revokes_ledger_approvals`. Host
checklist: e2e test, `cargo test --all`, GitHub CI on the exact head.

### Round 3 (contract finding c059a12e, round-2 section heading)

Root cause of the contract finding: the run-2 record was appended under
`## Shipped (run 2 — ...)` — the parenthetical suffix defeats
`dev-contract.py requirement_parts`'s strip regex (it matches only
`^## Shipped\s*\n`), so 1803 chars of operational prose leaked into the
contract hash (3b70cbf4... vs the assigned d1cc7a44...) and `implement()`
flagged contract_changed. Repair: the leaking section is removed entirely
(run-2's substance — merge resolution, finding triage, 85/0 suite counts —
is superseded and re-verified below); this round's evidence lives here,
inside the canonical `## Shipped` section that `requirement_parts` strips.
Verified: `requirement_parts(assigned original from 0564e747) ==
requirement_parts(this file)` (frontmatter key fields and normative prose
both equal; only `progress:`/`commits:` lines differ, which the contract
checker excludes by design).

Attribution repair in the same round: the merged origin/main head brought
squash landing commit `27bd585c` (`feat(task-155)`) whose SHA cannot appear
in its own squash, leaving task-155's `commits:` frontmatter one entry short
(task-095 R3-F4 class; no repair had landed anywhere — origin/main grep for
the SHA returns nothing). Applied the check's documented remedy: appended
`27bd585ca7eb429905ccbded1f48b4d0167c0c20` to
`specs/tasks/task-155.md`'s `commits:` frontmatter; no exemption entries
added (file stays at its frozen 3-entry baseline). Mutation check: removing
the SHA re-fails the gate (exit 1, identical violation), restoring re-passes
(exit 0) — the pass is attributable to the recorded SHA, not gate drift.

Verification on this round's working tree (evidence:
`/tmp/stage/review-evidence/task-138-r3-repair.md`,
`task-138-r3-static-gates.txt`, `task-138-r3-mutation-check.txt`,
`task-138-r3-mutation-restore-check.txt`):

- All 21 static gates exit 0 (architecture, hierarchy, ABAC registry +
  exempt handlers, MCP write tools, migration versions + SQL portability,
  dead message kinds, byte-slice truncation, relative path defaults,
  fail-open ref resolution, mem port contracts, fabricated scope defaults,
  lossy secret conversion, scope literal defaults, inert enforcement,
  forged scope fields, forwarded header trust, in-memory state stores,
  unbounded external HTTP, and the repaired task-commit attribution).
- Task-138 focused suites re-run on this exact tree (candidate code surface
  byte-identical to the run-2 verified tree; `git diff 466b6e72 HEAD --
  crates/gyre-server crates/gyre-domain crates/gyre-ports crates/gyre-adapters
  web/src web/tests` is empty): `gyre-domain spec_approval` 4/4,
  `gyre-adapters sqlite::spec_approval` 7/7, and the server lifecycle suite
  (`push_modifying_spec_revokes_ledger_approvals`,
  `require_current_spec_blocks_stale_spec_ref`, `api::gates`, `api::specs`)
  85 passed / 0 failed — identical to run 2.
- The merged task-155 CLI surface is byte-identical to origin/main's
  (`git diff 27bd585c HEAD -- crates/gyre-cli` empty), so no CLI probe is
  required beyond the server suite compiling against it.

### Round 4 (merge repair)

The run-3 checkpoint (4e291509) was complete, but the run itself died at
pre-flight (GitHub API TLS timeout — infra, recorded in the run-4 job log).
This round's controller base merge (0cca712d) was committed with two
defects, both repaired in b27d3787:

- **Unresolved conflict markers** in `git_http.rs` (spec-lifecycle push
  hook): the branch resolves the repo's workspace scope once per push
  (Option-typed `ws_id`, events skipped on unresolvable repo); main's
  task-180 landing re-resolved it inline per task. Kept the branch's
  once-per-push resolution — the common emission code below the conflict
  uses `ws_id.as_ref()` Option semantics and the inline version would
  shadow it with a non-Option type.
- **Migration version collision**: task-138's branch-local
  `spec_approval_ledger` and task-180's already-published
  `spec_assertion_results` both claimed `20261008000056`; Diesel runs only
  one per version (alphabetically first wins). Renumbered the branch-local
  migration to `2026-10-08-000057_spec_approval_ledger` (same remedy as
  f1340baf) since 000056 is live on origin/main.

Verification on this exact tree (evidence:
`/tmp/stage/review-evidence/task-138-r4.md`): `gyre-domain spec_approval`
4/4; `gyre-adapters sqlite::spec_approval` 7/7; server suites
`api::gates` + `api::specs` 83/83, lifecycle trio
(`push_modifying_spec_revokes_ledger_approvals`,
`require_current_spec_blocks_stale_spec_ref`,
`find_by_spec_sha_scopes_to_exact_version`) 3/3, `git_http` +
`spec_lifecycle` 38/38, and main's merged `spec_assert` surface 8/8
(server) + 51/51 (domain) — the conflict resolution preserves task-180's
behavior. Static gates: migration-versions, migration-SQL-portability,
arch, mem-port-contracts, task-commit-attribution all exit 0.

Sandbox limitation unchanged (TCP listener blocked, EOPNOTSUPP — see
above); host checklist: e2e
`spec_approval_auto_invalidated_on_spec_change`, `cargo test --all`, full
`npm test`, GitHub CI on the exact head.

### Round 5 (merge with assignment base def0add9e)

Merged the new assignment base `def0add9e` (task-157 landing) into this
branch; all four content conflicts resolved with both sides' behavior
preserved (evidence: `/tmp/stage/review-evidence/task-138-r5-merge.md`):

- `rust_extractor.rs`: dropped HEAD's self-described "TEMP diagnostic
  scaffold" (its own comment said revert before commit); kept base's clean
  single-candidate `DependsOn` edge.
- `api/specs.rs` approve: kept task-138's cloned `approver_id`/`persona`
  (reused by the durable ledger entry below) and added base's new
  `attestation_level`/`stack_hash` `SpecApprovalEvent` fields.
- `api/specs.rs` revoke: kept task-138's durable-ledger revoke + audit +
  `ledger_approval_id` response; replaced the naive reset-to-Pending with
  base's §9-aware `resolve_new_approval_status` recompute, so revoking one
  half of a `human_and_agent` pair does not demote a still-satisfied entry.
- `api/specs.rs` reject: took base's notification block (no early return on
  workspace lookup failure, so the rejection history note is still
  recorded).
- `mem.rs`: kept the `ApprovalTransitionError` import needed by the mem
  `SpecApprovalRepository`; removed the duplicate `BudgetUsage` import.
- `merge_processor.rs`: kept BOTH test sets — task-138's
  `require_current_spec_blocks_stale_spec_ref` regression and base's
  TASK-077 merge-time ABAC tests.
- Migration version collision (same class as round 4): base's
  `2026-10-08-000057_spec_approval_attestation` and task-138's
  `2026-10-08-000057_spec_approval_ledger` both mapped to version
  `20261008000057` (check-migration-versions.sh FAILED pre-fix). Renumbered
  the branch-local ledger migration to `2026-11-08-000058_spec_approval_ledger`
  (pure rename, SQL unchanged); gate exits 0.
- `web/dist` modify/delete conflicts staged from the assignment base per
  executor policy; not rebuilt or modified here.

Verification on this exact tree: `cargo check --all` clean;
`gyre-domain spec_approval` 4/4 and `rust_extractor` 16/16; `gyre-adapters
sqlite::spec_approval` 10/10; server suites `api::specs` + `api::gates`
90/90, `merge_processor` 68/68, `git_http` + `spec_lifecycle` 39/39
(combined 197/197, 0 failed); lifecycle trio passes individually. Static
gates all exit 0 (arch, mem-port-contracts, ABAC registry + exempt
handlers, task-commit-attribution, inert-enforcement, migration versions +
SQL portability, forged-scope-fields, fail-open-ref-resolution,
dead-message-kinds).

Sandbox limitation unchanged (TCP listener blocked, EOPNOTSUPP); host
checklist unchanged: e2e `spec_approval_auto_invalidated_on_spec_change`,
`cargo test --all`, full `npm test`, GitHub CI on the exact head.

### Round 6 (merge with assignment base fefde2946)

Merged the new assignment base `fefde2946` (task-089 HSI Explorer Generated
Views landing) into this branch; the only content conflict was in
`api/gates.rs` (`mem.rs` auto-merged), resolved in `a8fe6e0a` with both
sides' behavior preserved:

- `SpecApprovalResponse`: kept task-138's `rejected_by` + derived `status`
  fields and added base's `rejected_at`/`rejected_reason` doc comments and
  `skip_serializing_if` attributes (`rejected_by` gets the same skip for
  consistency). Both sides' assertions hold under the combined wire format:
  task-138's `assert_eq!(approved["rejected_at"], Null)` passes because
  serde_json indexing of an omitted key yields `Null`, and base's
  `assert!(active.get("rejected_at").is_none())` passes because the field
  is now skipped when `None`.
- Tests: kept task-138's full Spec Approval Ledger suite verbatim
  (`approvals_list_returns_full_ledger_data`,
  `revoked_approval_no_longer_verifies_spec_ref`,
  `reject_transition_pending_to_rejected_and_audit`,
  `find_by_spec_sha_scopes_to_exact_version`) and kept base's
  `list_spec_approvals_includes_rejection_fields`, adapted to the current
  domain API (4-arg `SpecApproval::new`, `approve(now)`,
  `reject(by, reason, now) -> Result`) since base wrote it against the
  pre-ledger domain this branch replaced; the assertion set is unchanged.
- `mem.rs` auto-merge kept: base's `MemSecretRepository::find_by_id` plus
  task-138's mem `SpecApprovalRepository`.

Verification on this exact tree (evidence:
`/tmp/stage/review-evidence/task-138-r6-merge.md`): server suite
`api::gates` 11/11 — both task-138's ledger tests and base's HSI
rejection-fields test pass against the merged wire format;
`api::specs` 80/80; base's auto-merged secret surface
(`api::secrets` + `secret_rotation` + `stale_agents`) 11/11. Static gates
exit 0 (arch, migration-versions, mem-port-contracts, ABAC route registry,
task-commit-attribution, inert-enforcement).

Sandbox limitation unchanged (TCP listener blocked, EOPNOTSUPP —
`/tmp/stage/capabilities.json`); host checklist unchanged: e2e
`spec_approval_auto_invalidated_on_spec_change`, `cargo test --all`, full
`npm test`, GitHub CI on the exact head.

### Round 7 (verification repair: clippy + rustfmt findings)

Repaired verification finding bb672b1a41214775b40908f88db05ddd
(`bash /tmp/stage/checks.sh` exit 1 against base fefde2946, candidate
fee5b3b62) in c7e74aa5. All three failing findings were lint-level, on
task-touched lines; no behavior changed:

- **clippy::double_must_use** (~20x, primary span on the
  `#[async_trait]` line of `gyre-ports/src/spec_approval.rs`): root cause
  is async-trait 0.1.89 (Cargo.lock pin) injecting `#[must_use]` into every
  expanded async trait method (`async-trait-0.1.89/src/expand.rs:69`) while
  the desugared `Pin<Box<dyn Future>>` return type is already `#[must_use]`
  (the `Future` trait is); async-trait's own generated-code suppression list
  (`lint_suppress_with_body/without_body`) does not cover this lint, so
  clippy 1.99 fires per method. The same warning fires repo-wide on
  pre-existing port traits (992 existing warnings outside the changed
  lines — the diff gate correctly ignores those); only this task's NEW port
  file had the attribute line inside the diff. A dependency bump is not
  verifiable in this sandbox (no network; only 0.1.89 in the local registry
  cache), so the fix is a scoped, commented inline
  `#[allow(clippy::double_must_use)]` on the trait, matching the repo's
  established inline-allow practice for clippy lints on macro-generated
  code (20+ `#[allow(clippy::...)]` precedents in crates/). No
  scripts/*-exemptions.txt entries were added.
- **clippy::manual_repeat_n** (sqlite adapter test helper):
  `repeat(seed).take(40)` → `repeat_n(seed, 40)`.
- **rustfmt**: the round-6 merged gates.rs test hunk (lines 718-720)
  formatted; whole-repo `cargo fmt` produced exactly this one hunk.

Verification on this exact tree (evidence:
`/tmp/stage/review-evidence/task-138-r7-verification-repair.md`):
`check-rustfmt-diff.py fefde2946` exit 0 (13 files clean);
`check-clippy-diff.py fefde2946` exit 0 (13 files clean, 992 existing
warnings outside changes) — the exact gates that failed; focused suites
`gyre-domain spec_approval` 4/4, `gyre-adapters sqlite::spec_approval`
10/10, `api::gates` + `api::specs` 91/91 (round-6 baseline for the same
combined filter: 91).

Sandbox limitation unchanged (TCP listener blocked, EOPNOTSUPP); host
checklist unchanged: e2e `spec_approval_auto_invalidated_on_spec_change`,
`cargo test --all`, full `npm test`, GitHub CI on the exact head.

### Round 8 (review repair: weak regression test, finding dcf03cee)

Repaired review finding dcf03cee in 59d31f8d8. The finding's mutation
probe showed `require_current_spec_blocks_stale_spec_ref`
(merge_processor.rs) stayed green under the exact regression its own doc
comment documents — reverting `resolve_spec_blob_sha` back to
`resolve_blob_sha` (merge_processor.rs:1380) left the stale-spec MR MERGED,
because the test only asserted the entry's absence from `list_queue()`,
which the Merged terminal state satisfies exactly like Failed. Per the task
contract that test was not proof. The repair is test-only; no production
behavior changed:

- **Primary assertion**: the stale entry's terminal status is now asserted
  to be exactly `Failed` via `merge_queue.find_by_id`, with
  `error_message` required to contain `requires current spec` — the exact
  remedy the finding prescribes.
- **Control assertion**: the current-SHA entry is now asserted to reach
  exactly `Merged` (not merely not-Failed). The test harness's
  `ConfigurableGitOps` double always succeeds at `merge_branches`, so only
  the stale-spec policy could hold this entry back — Merged positively
  proves the policy does not block current refs.

Mutation verification on this exact tree (evidence:
`/tmp/stage/review-evidence/task-138-r8-mutation-probe.log`): applying the
finding's one-line sed swap and recompiling now FAILS the test with
`assertion left == right failed: stale spec_ref must block the merge with
a Failed entry / left: Merged / right: Failed`; reverting the mutation
restores the pass (1/1). `cargo fmt --all -- --check` exit 0 (the change
introduced no formatting drift); full `merge_processor` suite 68/68 —
identical to the round-5 baseline for the same filter (evidence:
`/tmp/stage/review-evidence/task-138-r8-merge-processor-suite.log`).

Sandbox limitation unchanged (TCP listener blocked, EOPNOTSUPP —
`/tmp/stage/capabilities.json`); host checklist unchanged: e2e
`spec_approval_auto_invalidated_on_spec_change`, `cargo test --all`, full
`npm test`, GitHub CI on the exact head.
