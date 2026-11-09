# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration — per-repo config
(`enabled`, `watched_paths`, `ignored_paths`, `auto_invalidate_approvals`,
`dedup_open_tasks`, `default_priority_new/modified/deleted`) replacing the
hardcoded watched paths and task priorities in the post-receive hook.

Assignment: base `774737638`, candidate `d63778ff` (docs-only tip over merge head
`e135f37de`, which merged the assignment base into the branch — conflict resolved
in `process_spec_lifecycle`).

Verdict: **complete**.

## Round 1 (this round, fresh instance; prior attempt's gateway failed before review)

All evidence under `/tmp/stage/review-evidence/task-109/` (summary:
`review-summary.md`; raw outputs alongside).

### Implementation verified (full line-by-line review of the 25-file diff)

- **Domain** (`gyre-domain/src/spec_lifecycle_config.rs`): `SpecLifecycleConfig`
  with all eight spec fields; typed `TaskPriority` priorities (invalid values
  rejected at deserialization); serde field-level defaults so partial payloads
  keep spec baselines; `is_watched()` with ignored-over-watched precedence;
  defaults byte-equal to the prior hardcoded behavior (watched
  `specs/system/`,`specs/development/`; ignored milestones/prior-art/personas/
  prompts; Medium/High/High; all flags true) — verified against the base's
  `SPEC_WATCHED_PATHS` const and priority literals.
- **Persistence**: migration `2026-10-08-000059_spec_lifecycle_configs`
  (`repo_id` PK, `config` JSON TEXT, portable SQL); SQLite and Postgres adapters
  implement the `SpecLifecycleRepository` port with upsert-on-conflict; absent
  rows mean defaults (port contract, guarded in both adapters and the mem repo);
  wired through the `store!` macro like sibling stores; middleware clone path
  carries the field.
- **API** `GET/PUT /api/v1/repos/:id/settings/spec-lifecycle`: GET returns
  defaults for unconfigured repos, 404 unknown repo; PUT is Admin/Developer-only
  (Agent → 403, tested through real JWT machinery — an agent cannot disable the
  drift detector it is subject to); route registered with a non-exempt ABAC
  `RouteResourceMapping` (middleware policy evaluation applies, same pattern as
  the sibling spec-policy route); documented in `docs/api-reference.md`.
- **Hook wiring** (`process_spec_lifecycle`): per-repo config loaded through the
  same port the API writes; `enabled=false` returns before any diff/task/approval
  work; filtering, all three task-creation priorities, and both flags
  (`auto_invalidate_approvals`, `dedup_open_tasks`) driven by config; no hardcoded
  spec path literals remain in production code (remaining occurrences are inside
  `#[cfg(test)]` modules).
- **Merge resolution independently verified**: diffing the merged
  `process_spec_lifecycle` against the base's version shows only the additive
  config machinery; the base's once-per-push workspace-`Option` resolution,
  skip-events-on-unresolvable-repo (task-097 F3 class), and
  approval-invalidation decoupling are preserved verbatim; the base test
  `push_modifying_spec_revokes_ledger_approvals` remains consistent. The diff vs
  base touches only task-109 files (+1758/−57); nothing from the base is
  reverted; scripts/, .github/, web/dist untouched.
- **UI**: "Spec Lifecycle" tab in RepoSettings.svelte (10 tabs) with enabled
  toggle, tag-style add/remove watched/ignored path lists, both flag toggles,
  three priority selects, save via `api.setRepoSpecLifecycle`; locale strings in
  `en.json`.

### Tests executed at the exact candidate head (this sandbox)

- `npm ci` → exit 0; `npx vitest run RepoSettings.test.js` → **57/57**;
  `npx vitest run api.test.js` → **20/20**.
- Mutation probe: `addWatchedPath` body replaced with a no-op →
  `saves edited config via api.setRepoSpecLifecycle` **fails**; source restored
  (tree clean, re-run green) — the save test is anchored to the edit path, not
  incidental rendering.
- `cargo fmt -p gyre-domain|gyre-ports|gyre-adapters|gyre-server --check` → all
  clean (syntax validation of the merged tree).
- All 91 mechanical gate scripts run: 56 pass; every failure is pre-existing or
  environmental (awk dialect, usage-arg hooks) — the single failure pointing into
  a diff-touched file (`accumulate_chain_constraints` tuple at git_http.rs:3465)
  is byte-identical in the base and untouched by the candidate.

### Transport restriction (not a code defect)

This instance has no outbound network and no cargo registry/target cache
(probes: `curl https://index.crates.io` → connection refused; `cargo check -p
gyre-domain --offline` → exit 101, no registry). Rust suites could not execute
here. Remote verification must run at the candidate head:

1. `cargo test -p gyre-server --lib spec_lifecycle` (incl. the two end-to-end
   hook tests against a real bare git repo and the SQLite migration-traverse
   API test)
2. `cargo test -p gyre-server --lib push_modifying_spec_revokes_ledger_approvals`
3. `cargo test -p gyre-adapters --lib spec_lifecycle`;
   `cargo test -p gyre-domain --lib spec_lifecycle`
4. `cargo test --all` (acceptance criterion) + clippy

The hook tests are logically anchored (verified by reading, in lieu of
execution): the disabled-config test pushes a `specs/system/` file and asserts
zero tasks — it fails if `enabled` is ignored; the per-repo test asserts exactly
one task with the custom path and priority — it fails if watched paths or
priorities fall back to defaults.

Findings: none.

— Reviewer, 2026-11-09
