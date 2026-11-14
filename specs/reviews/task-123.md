# Review: TASK-123 — Notification Entity with Full Type Taxonomy and Priority Model

**Reviewer:** Independent review agent
**Date:** 2026-11-10 (attempt 6478ea9b, candidate `5d6af9461e8e0e7bf9a052d6940145c5a32a1bc3` on base `bfc36c9c03b5baddaec30f204feb18ac5981b7d2`)
**Verdict:** approved

---

## Scope of this round

The assigned base already contains the original task-123 implementation
(all five frontmatter commits are ancestors of the base; mainline merged
them). The candidate's new work over the base is:

- `6b3d4641` — derive the four-level notification priority band from the
  notification TYPE (`NotificationPriority::for_notification_type` in
  `gyre-common`), never from the HSI §8 1–10 numeric rank.
- `90edbb2f` — filter `GET /api/v1/notifications?priority=` by the
  type-derived band (type-string set membership on the persisted
  `notification_type` column, legacy DB names included), replacing the
  rank-band `min/max` widening.
- `6c14f6c1`/`810a4288`/`908206d2`/`3a65e90f`/`5d6af946` — spec pointer
  note + task bookkeeping only (verified: no production code in these).

## What was verified

**Task contract (acceptance criteria)** holds in the candidate tree:
`Notification` carries every spec field; all 18 taxonomy variants plus the
HSI §2 inbox types (41 total, `as_str`/`parse`/`default_priority_rank`
tables consistent); `NotificationPriority` Low/Medium/High/Urgent;
`NotificationSource` {entity_type, entity_id, workspace_id, repo_id};
default priority mapping; action_url deep links; creation paths typed per
taxonomy (from the merged original round, present in base).

**The repair is complete across every band surface** (this round's diff):

- `Notification::new` → `default_priority()` → `for_notification_type`
  (type-derived); no production `from_rank`/`rank_band` band derivations
  remain (grep across server/domain/adapters/common: definitions and tests
  only).
- SQLite and PostgreSQL read paths reconstruct the band from the persisted
  `notification_type`, not the rank — fixing pre-repair rows retroactively
  (only the rank is persisted).
- Domain channel-routing `for_notification_type` now delegates to the
  canonical `gyre-common` table — one notification, one band on every
  surface (entity, API `priority_level`, webhook/email payloads, channel
  gating).
- `?priority=` expands to `NotificationPriority::notification_type_strings()`
  (canonical + legacy DB names of renamed variants), intersects `?type=`,
  and pushes down as a set filter; port contract ("empty set matches
  nothing") is enforced identically in SQLite (`eq_any` + explicit
  empty→no-rows), PostgreSQL (same), and mem (`contains`).
- `spec_patrol` error findings keep the rank-3 Inbox position but the band
  stays the type's Medium; `workspace_ownership` ownerless notification is
  band High at rank 2 — both per the amended membership table.
- `min_priority`/`max_priority` remain rank-range filters (HSI §8), unchanged.

**Probe evidence** (logs under `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-common --lib notification` — 12 passed (taxonomy
  round-trip, amended-membership-table pin, band type-string sets,
  legacy-name parse, `all()` enumeration). Exit 0.
- `cargo test -p gyre-adapters --lib sqlite::notification` — 9 passed
  (real migration; read-path band reconstruction, type-set filter incl. a
  raw-inserted legacy-named row, empty-set semantics). Exit 0.
- `cargo test -p gyre-domain --lib` — 510 passed. Exit 0.
- `cargo check -p gyre-server` (lib) — clean. Exit 0.
- **Mutation probes (both restored and re-verified green):**
  - Reverting `default_priority` to `from_rank(default_priority_rank())`
    fails `entity_priority_matches_amended_membership_table`
    ("SpecApprovalRequested must be High") — the creation-path test is
    discriminating, not self-confirming.
  - Reverting the SQLite read path to `from_rank(priority_rank)` fails
    `read_path_reconstructs_band_from_type_not_rank` — the read-path test
    is discriminating.
- `cargo test -p gyre-server --lib …` is blocked by the executor's build
  wrapper (4 GiB budget) — see the deferral note below.

**Mechanical-check exposure:** no new routes, migrations, MCP tools, or
exemption entries in this round's diff; process commits touch only
`specs/tasks/task-123.md` and the spec pointer note. Attribution at the
candidate commit lists all seven commits, including both repairs.

## Deferred to the independent verifier (server tests, 4 GiB budget)

- `cargo test -p gyre-server --lib api::users` — `?priority=` filter
  (high returns both High items incl. the rank-2 divergence; urgent
  returns 0; type∩priority intersection; unknown level → 400),
  `priority_level` agreement, mark-read endpoints.
- `cargo test -p gyre-server --lib notification_dispatcher` —
  `entity_band_equals_channel_routing_band`.
- `cargo test -p gyre-server --lib spec_patrol` and
  `cargo test -p gyre-server --lib workspace_ownership` — band assertions
  (Medium at rank 3; High at rank 2).
- Full `cargo test --all` per the task's acceptance criteria.

The server test diffs were reviewed line-by-line (mechanical
`Some(&["…"])` adaptations plus the new tests above); server lib
type-checks clean.

## Non-blocking observations (recorded, not findings)

1. **Spec note numeric examples are historical.** The amended membership
   note in `user-management.md` justifies non-threshold-mappability with
   "AgentEscalation is numeric 5 … BudgetWarning numeric 7 …
   SecurityFinding and GateFailure share numeric priority 3" — the HSI §8
   draft values. Current `default_priority_rank` values are 2/4/1/3, and
   the true divergences are SpecApprovalRequested (2→High),
   CascadeTestTriggered (4→Low), SecretExpiring (7→High),
   SpecAssertionFailure (9→Medium) — exactly what the code comments and
   tests pin. The note's normative content (type-derived bands; canonical
   `for_notification_type`) matches the code; the membership table also
   still uses pre-rename names and predates the newer variants. Doc
   staleness only, no code/spec contradiction.
2. **Working-tree `specs/tasks/task-123.md` (executor scaffolding, not the
   candidate).** The checkout carries an uncommitted edit reverting the
   frontmatter to the original 5-commit list and a pre-repair Shipped
   narrative ("priority is always the band of the HSI rank") that is
   factually wrong for the candidate tree, and it drops `6b3d4641`/`90edbb2f`
   from the attribution list (`check-task-commit-attribution.sh` exposure).
   The candidate commit `5d6af946` itself carries the correct 7-commit
   frontmatter and current narrative. The controller/verifier must
   reconcile the checkpointed task file against the candidate before
   publication; this review did not modify it.

## Verdict

The candidate is a genuine, mutation-tested repair that closes the
priority-band derivation gap on every surface (creation, persistence read
path, API filter, API response, channel routing) with real production
code, consistent specs/coverage/docs, and discriminating tests. No
blocking findings.
