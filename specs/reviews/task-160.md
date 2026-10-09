# Review — task-160 (Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth)

Spec: `specs/system/hierarchy-enforcement.md` §2 (Invariant Enforcement), §3 (Enforcement — Consistent Tenant Filtering), §7 (New Scripts table).
Comparison base: `66422bd4b99de70536cce8422ec23db5eaec082d` → HEAD `f5157996f1457faee6efd7de6183631e3a8123a8` (tree clean).
Verdict: **complete** (round-3 findings F1/F2 repaired in `06df8bf` and independently verified; no new material findings on the repair diff). Round-3 detail below is superseded; see "Round 4".

## Verified working

- **The adapter fixes are real enforcement, not lint-appeasement.** Both backends: `workspace.rs find_by_id/list`, `repository.rs find_by_name_and_workspace`, `merge_request.rs` create stamp, `notification.rs` (get/list_for_user/count_unresolved/list_recent/has_recent_dismissal), `activity.rs` query, `analytics.rs` (query/count/aggregate_by_day raw SQL `AND tenant_id = ?`/`$4` + create stamps), `compute_target.rs` (get_by_id/has_workspace_references), `saved_view.rs` (get/list_by_repo/list_by_workspace) — every one adds `.filter(<table>::tenant_id.eq(&tenant))` derived from `self.tenant_id`, and the four create() sites stamp the storage's real tenant instead of `"default"` (merge_request/repository × sqlite+postgres, analytics/cost record × sqlite+postgres).
- **Isolation is proven end-to-end, not just textually.** `crates/gyre-adapters/tests/tenant_isolation.rs` runs two `SqliteStorage` handles over ONE db file differing only by tenant scope. Clean-HEAD run: **2 passed, 0 failed** (exit 0; isolated worktree `/tmp/stage/task160-mutate`, private target dir, re-confirmed this round). Prior-round mutation evidence (persisted, worktree content verified identical for all task files): reverting the sqlite fixes makes both tests FAIL with exact leak diagnostics (`ws list leaked ["ws-a","ws-b"]`, `find_by_id` cross-tenant some, create-stamp invisibility). `cargo test -p gyre-adapters --lib` → **344 passed, 0 failed** on clean HEAD (covers the modified in-file test mods in notification.rs/compute_target.rs, which prior evidence had not built).
- **check-hierarchy.sh is a real scanner with proven kill sensitivity.** Clean HEAD: pass. Mutations (this round, isolated worktree, restored after each): `Task.workspace_id → Option<Id>` → exit 1 naming `task.rs:60`; `Task.repo_id` renamed (field deletion) → exit 1 "missing from the struct" — the deleted-field detection closes the rename-away bypass. Scans by struct name across the whole domain crate (survives file moves), mawk-clean (local awk is mawk 1.3.4; script ran under it). Default-on with `GYRE_CHECK_HIERARCHY=0` opt-out matches spec §103's "enabled in M34 Slice 3 after the non-optional migration lands" — the migration has landed (fields are `Id`, verified), so the inversion is the spec's intended end-state, not a contradiction.
- **check-tenant-filter.sh is substantially stronger than the spec's letter.** Spec §145 literally asks for a `.filter(table::tenant_id.eq(` grep over `sqlite/` and `pg/`; the shipped script instead (a) names the REAL `postgres/` dir (the literal `pg/` never existed — a naive letter-implementation would scan nothing), (b) derives which tables have the column from the migrations themselves (ground truth, with DROP/RENAME-to-`_new` recreation handling — commit ceb6209 fixed live table-loss), (c) reads by behavior not just name prefix (found live: `has_workspace_references`, `has_recent_dismissal`), (d) reports no-column tables as visible non-failing backlog rather than silently dropping them, (e) exits 2 on scanner-broken conditions (0 tables derived, missing dirs, 0 methods checked). Clean HEAD: **109 read methods checked, 0 violations**, exit 0. Mutant (pre-160 sqlite reverted): **21 violations, exit 1** with exact file:line.
- **check-api-auth.sh** adds Check 3 by delegating to `check-abac-route-registry.sh` (single owner of the frozen baselines — re-implementing would fork them). Clean HEAD: pass. Mutant (unregistered route): exit 1 (mut5-api-auth.txt).
- **Skip list is defensible.** Spec §133 names MessageRepository/UserWorkspaceStateRepository as the structural-isolation class. `tenant.rs`: `find_by_id` resolves the tenant identity itself — a self-tenant filter is impossible by construction (used by OIDC/SCIM before tenant context exists). `workspace_membership.rs find_by_id` (by PK): zero production callers — handlers use `find_by_user_and_workspace` (both params tenant-bound; verified: only create/delete/list_by_workspace/find_by_user_and_workspace called in gyre-server). `user_workspace_state.rs`: only `get_last_seen(user_id, workspace_id)`, structurally addressed. `kv_store.rs`: no tenant column, documented backlog.
- **CI gate un-advisored**: `|| true` removed from check-api-auth and check-tenant-filter steps (they now block); hierarchy + tenant-filter + api-auth all wired in pre-commit (`pass_filenames: false`). All three scripts executable, usage comments at top.
- **Exemption file shrank and re-anchored honestly**: 24→18 entries; the 4 adapter create-stamp entries removed because the sites are genuinely fixed (verified: mutations reintroducing `tenant_id: "default"` at sqlite/merge_request.rs:163 and sqlite/repository.rs:94 make `check-scope-literal-defaults.sh` fail with exit 1). The `lib.rs:490→494`, `534→538` moves re-anchor stale entries to the real (unchanged, pre-existing) violations in unchanged lib.rs — not masking anything new.

## Findings

### F1 (material, must fix) — 6 dead exemption entries left in `scripts/scope-literal-defaults-exemptions.txt`

The task's `From<&ActivityEvent>` → `ActivityEventRecord::new(e, tenant)` rewrite (and the analytics/cost record-literal fixes) eliminated the `"default"` stamps at `sqlite/activity.rs:56`, `sqlite/analytics.rs:115`, `sqlite/analytics.rs:225`, `postgres/activity.rs:56`, `postgres/analytics.rs:115`, `postgres/analytics.rs:225` — but their exemption lines remain, and `FROZEN_EXEMPTION_COUNT` stayed at 18 instead of dropping to 12. Verified: scanning `crates/` with no exemptions finds exactly **12** violations, all outside those 6 keys (the file's 18 entries = 12 live + 6 dead). The file's own header policy says "Fix a site ... then DELETE its line and lower FROZEN_EXEMPTION_COUNT". As shipped, the count is lying upward (harmless today) but the 6 stale `file:line` keys are live exemption slots: a future `"default"` stamp landing at exactly `sqlite/analytics.rs:115` (a struct-literal position that shifts by single lines) would be silently exempted. Repair: delete the 6 lines, set `FROZEN_EXEMPTION_COUNT = 12` (both in the header comment and the Python constant).

### F2 (material, should fix) — check-tenant-filter.sh has a 2-fn blind spot outside the read-name heuristic

Independent cross-check (probe re-implements the scanner's table derivation and fn walk): fns with Diesel read terminals on tenant-column tables that the `is_read` name heuristic does NOT classify as reads — `sqlite/agent.rs::record_usage` and `sqlite/secret.rs::resolve_for_agent`. Both currently DO filter (`agents::tenant_id.eq(&tenant)` at agent.rs:330; `secrets::tenant_id.eq(tenant_id)` at secret.rs:333/secret.rs scope filter) — **no live leak** — but the lint would not catch their regression. Cross-validation: the probe agrees with the script on exactly 109 name-checked fns, so this is a pure blind-spot extension, not a disagreement. Repair options (either acceptable):
- Add `record` and `resolve` to the read-prefix set (they are reads-by-behavior here: `record_usage` reads-then-updates, `resolve_for_agent` is a pure read) — simplest, matches the script's existing prefix philosophy; note `record` must not sweep analytics `record()`/cost `record()`/activity `append`-style writes into the checked set — verify they don't touch tenant-column tables with read terminals (they are pure inserts; probe confirms only `record_usage` has a read terminal among `record*`).
- Or require the tenant predicate for any fn whose body contains a pure-read terminal (`.load`/`.first`/`.load_one`) on a tenant-column table regardless of name; probe shows this newly checks exactly the 2 fns and no write-shaped false positives except `record_usage` itself (which is read-modify-write and legitimately needs the filter it already has).

The pg twin of `resolve_for_agent` is a pre-existing `bail!` stub (postgres/secret.rs:38) — out of scope for this task (task-160 does not touch it; `SecretRepository not implemented for PgStorage` is a separate gap, not introduced or worsened here).

## Evidence

- `/tmp/stage/review-evidence/task-160/` — mutation-sensitivity.txt (tenant_isolation kills both defect classes; 21-violation lint on mutants), lint-on-mutants.txt, mut3/mut4 (per-file tenant-filter kills), mut5 (api-auth kill), hierarchy-mutations.txt (this round: Option-kill + field-deletion-kill, both exit 1, worktree restored), tenant-filter-clean-head.txt / api-auth-clean-head.txt (this round, exit 0), probe_unchecked_reads.py + probe_pure_read_terminals.py (blind-spot cross-check, 109-fn agreement), tenant_isolation.rs.snapshot.
- Clean-HEAD runs this round: `check-hierarchy.sh` exit 0; `check-tenant-filter.sh` exit 0 (109 checked / 0 violations); `check-api-auth.sh` exit 0; `check-scope-literal-defaults.sh crates` OK; `cargo test -p gyre-adapters --lib` 344 passed / 0 failed; `cargo test -p gyre-adapters --test tenant_isolation` 2 passed / 0 failed (isolated worktree at review-HEAD content, private `CARGO_TARGET_DIR=/tmp/stage/task160-mutate/target`, sequential runs).
- Commit attribution: all 6 `commits:` entries are ancestors of HEAD; diff base 66422bd confirmed.

## Repair path

1. F1: delete the 6 dead exemption lines (activity/analytics × sqlite+postgres), lower `FROZEN_EXEMPTION_COUNT` 18→12 in both the comment and the constant in `scripts/check-scope-literal-defaults.sh`; update the exemptions header comment. Re-run `bash scripts/check-scope-literal-defaults.sh crates` (must stay OK) and confirm no-exemption count is 12.
2. F2: extend `scripts/check-tenant-filter.sh`'s `is_read` prefix set with `record`/`resolve` (or implement the pure-read-terminal rule); re-run on clean HEAD — must remain 0 violations (both fns already filter); re-run the probe to confirm the blind spot is closed; add a one-line comment documenting why the prefixes are reads-by-behavior.
3. No product-code changes required. Both repairs are lint/tracking hygiene on files this task already owns.


## Round 4 — repair verification (verdict: complete)

Repair commit: `06df8bfb02cbf0c50059c580f1f87860708543ff` (plus process-only `f515799` updating this task file). HEAD `f515799` tree clean. Each round-3 finding independently re-verified:

**F1 — dead exemption entries (FIXED).** Zero-exemption scan (script copy with emptied exemption file, `crates/` arg) finds exactly **12** live violations, and `diff` against the remaining 12 exemption lines shows **identity** — the file is now exactly the live set, no dead slots. `FROZEN_EXEMPTION_COUNT=12` in both the header comment and the Python constant; header updated with the 24→18→12 history. Mutation probe: reintroducing `tenant_id: "default"` at the formerly exempted `sqlite/activity.rs` ctor position fails `check-scope-literal-defaults.sh crates/` with exit 1 and exact file:line — the dead slots no longer silently absorb a regression. Clean run with real exemptions: OK.

**F2 — read-name blind spot (FIXED).** Prefix set extended with `record|resolve`. Ablation: dropping the two prefixes returns 109 checked; with them, 111 — the +2 is exactly `sqlite/agent.rs::record_usage` (read-modify-write, `.first()` on `agents` — my own enumeration initially missed it because of `.load::<…>` turbofish, the awk regex `[^a-z_]` handles it) and `sqlite/secret.rs::resolve_for_agent` (pure read, `.load::<…>` on `secrets`). Mutation probes (isolated worktree `/tmp/stage/task160-mutate` at review HEAD, restored clean after each): stripping the tenant predicate from either fn → exit 1 with exact file:line. Every other `record*`/`resolve*` fn across both adapter dirs has no read terminal (pure insert/update `execute()` bodies, or `bail!` stubs) — enumerated independently, zero false positives.

**Residual blind-spot sweep (new probe this round).** Independent harness replicating the lint's table derivation + fn walk (validated: reproduces exactly the lint's 111-fn checked set once the per-file SKIP_LIST is honored) found **zero** read-terminal fns on tenant-column tables outside the name heuristic. Remaining name-uncovered read terminals (`kv_get`/`kv_list`/`is_token_revoked`/`upsert_*` read-backs/`meta_spec update`'s archive fetch) all touch tenant-less tables (`kv_store`, `revoked_tokens`, `budget_usages`, `llm_function_configs`, `prompt_templates`, `meta_specs` — verified against migrations: none has a `tenant_id` column) and are already reported in the visible backlog. The name heuristic is currently exhaustive over the tenant-column read surface.

**Regression re-runs on clean HEAD.** `check-hierarchy.sh` exit 0 (and still kills `Task.workspace_id → Option<Id>` with exit 1 naming `task.rs:60` — re-probed this round); `check-tenant-filter.sh` exit 0, 111 checked / 0 violations; `check-api-auth.sh` exit 0; `check-scope-literal-defaults.sh crates/` OK; `check-task-commit-attribution.sh` OK (repair commit `06df8bf` is process-clean: scripts+specs only, no product surface, so no frontmatter requirement — and `(task-160)` labeled).

**Non-findings (checked, not material):** (a) `check-scope-literal-defaults.sh` invoked no-arg by pre-commit/`dev-check.sh`/CI scans nothing (vacuous OK) and CI keeps `|| true` — pre-existing task-099 wiring at the comparison base, outside this task's diff and this task's file ownership; the F1 repair itself is verified with the `crates/` path. Worth a follow-up task for the task-099 owner. (b) pg `notification.rs::resolve` is a pure update (no read terminal) — correctly outside the checked set. (c) The three task scripts hardcode their scan roots, so their no-arg pre-commit/CI invocations are sound (unlike the arg-taking scope script). (d) task file round-3 section says "bail! stub" for pg `resolve_for_agent` — accurate.

Evidence: `/tmp/stage/review-evidence/task-160-round4/` — zero-exemption scan, exemption-set vs live-violation diff, mutation outputs (record_usage, resolve_for_agent, activity.rs default-literal, hierarchy Option), ablation (109 vs 111), clean-HEAD lint runs.

## Round 5 — independent review of candidate e1e16020 (verdict: complete)

Candidate: `e1e16020` against base `8c2d1775`. The task surface
(scripts/, adapters, pre-commit, CI wiring) is byte-identical to the
round-4-reviewed tree `13ff2a51` (empty diff on all task files); the only
candidate-authored commit is the docs commit recording re-verification at
the merged HEAD plus the task-210 attribution-drift repair. All round-5
evidence independently re-derived, not reused:

**Clean runs at candidate (exit 0 all):** check-hierarchy.sh;
check-tenant-filter.sh (111 checked / 0 violations, backlog visible);
check-api-auth.sh (3 checks, Check 3 delegated to the frozen-baseline
owner); check-scope-literal-defaults.sh `crates`; check-abac-route-registry;
check-abac-exempt-handlers (89 handlers); check-task-commit-attribution.
`cargo test -p gyre-adapters --test tenant_isolation` 2 passed / 0 failed;
`cargo test -p gyre-adapters --lib` 344 passed / 0 failed.

**Mutation kills (isolated worktrees, restored after each):** hierarchy —
`Task.workspace_id → Option<Id>` exit 1 naming task.rs:60, field deletion
exit 1; tenant-filter — sqlite workspace find_by_id, pg notification get,
pg compute_target has_workspace_references, sqlite resolve_for_agent,
sqlite agent record_usage (both F2-repair prefixes), sqlite secret
get_value — all exit 1 with exact file:line; api-auth — abac_middleware
layer removal, resolver-entry deletion, route-rename-without-registry,
POST handler auth-extractor removal all exit 1. The tenant_isolation test
kills both defect shapes: deleting the workspace find_by_id tenant
predicate fails with "find_by_id leaked tenant B's workspace id to tenant
A"; corrupting it fails the positive control.

**Skip-list controls:** disabling the message.rs skip entry surfaces 10
MISSING violations (the skip list is load-bearing, not blanket-pass);
disabling the kv_store.rs skip entry changes nothing — its read methods
(`kv_get`/`kv_list`) lack read-name prefixes so they were never in the
checked set; the entry is redundant documentation today, not a bypass.

**Non-findings (checked, not material):** (a) coverage rows 10/14/27 in
`specs/coverage/system/hierarchy-enforcement.md` remain `task-assigned` —
the task contract has no coverage-flip clause (unlike task-207), and the
task-206 precedent (d60a850b) established that implementer/review
self-flips are rejected as self-grading; status stays literally true until
the task completes; the stale notes describe the base's advisory state.
PM/auditor follow-up. (b) check-api-auth Check 2 covers mutating verbs
only by documented design (base and candidate identical); a GET outer-
router handler losing its auth extractor is outside it — git handlers
derive the repo from `auth.tenant_id`-scoped find_by_slug, so the actual
exposure is compile-time or tenant-scoped anyway. (c) `check-path-scope-
binding.sh` dies with a mawk syntax error (gawk-only `match(..., arr)`)
and exits 2 — pre-existing at base, file untouched by this task; belongs
to that script's owner. (d) pg secret `resolve_for_agent` remains a
pre-existing `bail!` stub — out of scope (no tenant-column read terminal
either way).

Evidence: `/tmp/stage/review-evidence/task-160-r5/` (25 files: clean runs,
all mutation outputs with commands, skip-list controls, coverage-matrix
check, GET-handler gap analysis). Network note: crates.io sparse index
unreachable from the review sandbox; the registry index cache was seeded
from the exact Cargo.lock crate list via static.crates.io-backed fetches,
then normal cargo built and ran unchanged.
