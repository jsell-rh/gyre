# Review — task-109 (Externalize spec lifecycle configuration)

Spec: `specs/system/spec-lifecycle.md` §Configuration (per-repo `[spec_lifecycle]` config: enabled, watched/ignored paths, auto_invalidate_approvals, dedup_open_tasks, default_priority_new/modified/deleted).
Candidate: `65ccbf14af3075b64b1dd4210d4a9f2ba4973968` (assignment base `7f8b5d9cbdff735f15f513f007fbb12c4d4fe545`; a prior interrupted review round left only mechanical-check evidence, which was re-run fresh — its domain-test probe never completed and was re-run in full here).
Verdict: **approved**.

## Probes (all at the exact candidate, evidence under /tmp/stage/review-evidence/task-109/)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain --lib spec_lifecycle` → **5/5** (defaults match spec exactly; partial-JSON serde defaults; ignored-over-watched precedence; string priorities).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib spec_lifecycle` → **4/4** against a real temp-file SQLite DB via `SqliteStorage::new` (which runs the embedded migrations) — proving the renumbered migration `2026-10-08-000057_spec_lifecycle_configs` actually creates the table: unconfigured defaults, custom-config round-trip, upsert-overwrite, per-repo isolation.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib no_duplicate_migration_version_prefixes` → **1/1** (the 000056 collision that motivated the renumber repair is confirmed absent).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib spec_lifecycle` → **7/7**: API 5 (GET defaults for unconfigured repo, PUT round-trip through the same port the hook reads, partial-body serde defaults, unknown repo 404, **Agent-role 403 with a real RS256 JWT** on the assigned `/settings/spec-lifecycle` path) + hook 2 end-to-end against a **real bare git repo with real pushed commits**: `enabled=false` creates no tasks; custom `watched_paths: ["docs/guides/"]` fully replaces the default prefixes (the `specs/system/` file in the same diff produces no task) and `default_priority_new: Critical` lands on the created task.
- `git_http::tests::parse_spec_changes*` → **9/9** and `classify_spec_change*` → **6/6** (config-driven filtering, custom paths, ignored-override, configured priorities for A/M/D/R).
- `npm ci` then `npx vitest run src/__tests__/RepoSettings.test.js` in `web/` → **56/56** (4 new Spec Lifecycle tab tests: panel render, load via `api.repoSpecLifecycle`, field render from data, save via `api.setRepoSpecLifecycle`).
- Mechanical checks at the candidate, all exit 0: `check-migration-versions.sh`, `check-migration-sql-portability.sh`, `check-arch.sh`, `check-abac-route-registry.sh`, `check-in-memory-state-stores.sh`, `check-inert-enforcement.sh`, `check-task-commit-attribution.sh`.

## Verified against the acceptance criteria

- **Struct**: `gyre-domain/src/spec_lifecycle_config.rs` — all eight spec fields; priorities typed as `TaskPriority` (invalid values rejected at deserialization rather than stored as bad strings); `Default` matches the prior hardcoded behavior exactly (watched `specs/system/`+`specs/development/`, ignored milestones/prior-art/personas/prompts, Medium/High/High, all flags true).
- **Persistence**: separate `spec_lifecycle_configs` table (repo_id PK, config JSON TEXT), portable SQL; Diesel SQLite + Postgres adapters implement the `SpecLifecycleRepository` port with on-conflict upsert; mem adapter port-backed (durable-state invariant respected); wired via `store!` in `AppState`.
- **API**: `GET/PUT /api/v1/repos/:id/settings/spec-lifecycle` — the route contract from the assignment (restored by 483559c0 after an earlier round drifted to a different path). PUT is Admin/Developer-only in the handler body (Agent 403 proven with a real JWT); GET returns defaults for unconfigured repos; unknown repo → 404. Route registered in `api/mod.rs` with an ABAC `RouteResourceMapping` entry (registry check green), documented in `docs/api-reference.md`. The GET-without-auth-extractor shape mirrors the existing `spec_policy.rs` convention — authn/scope is middleware ABAC's job for registered routes.
- **Hook wiring**: `process_spec_lifecycle` loads config through the same port the API writes; `enabled=false` returns before any diff/task/approval work (proven end-to-end); `is_watched` filtering with ignored-precedence; `classify_spec_change` takes all three priorities from config (proven: Critical lands on the created task); `auto_invalidate_approvals` and `dedup_open_tasks` are gated on the config flags in the hook body. Task workspace scoping now comes from the repo resolved at push time (`resolved.workspace_id`) rather than a post-hoc re-lookup — strictly better than the pre-candidate code, no scope fabrication. No hardcoded path literals remain in production code (grep clean; the only occurrences are the intended `DEFAULT_*` constants in the domain).
- **UI**: RepoSettings "Spec Lifecycle" tab with enabled toggle, tag-style add/remove lists for watched/ignored paths, both flag toggles, three priority selects; api.js + en.json strings present.

`cargo test --all` and exact-head GitHub checks are owned by the verification stage per assignment instructions; this sandbox cannot run TCP listeners (capabilities.json), so `git_clone_empty_repo_via_smart_http` and live browser drive require host verification — a transport restriction, not a code defect.

## Minor (not blocking, recorded for completeness)

- The task file's Shipped narrative states the server API tests "construct a fresh SQLite-backed server, so GET/PUT now exercise the renumbered migration end-to-end." That is inaccurate: `app_with_repo` uses `crate::mem::test_state()` and `put_agent_role_forbidden` uses `make_test_state_with_jwt` (auth.rs:1139), both mem-backed. The renumbered migration is nonetheless genuinely proven end-to-end by the adapters tests (real SQLite through `SqliteStorage::new` → embedded migrations). Narrative misstatement only; no acceptance criterion depends on it.

## Conclusion

Every acceptance criterion is backed by independent, behavior-killing probe evidence at the exact candidate. Approved.
