# Review — task-194 (Manifest-driven spec lifecycle task creation)

Spec: `specs/system/spec-registry.md` §14/§15 (Spec Lifecycle Integration) + §Manifest Rules.
Commits under review (base `1e61f663` → candidate `61c1db92`): `236056d9` (implementation), `a13cf69d`/`06a78d19`/`1a07e183`/`4e2a1b6c` (verification repairs), process/docs commits.
Verdict: **approved** — no blocking findings. Server test execution deferred to the independent verifier (see below); everything review-runnable was run and passes.

## What was reviewed

Full diff of `crates/gyre-server/src/git_http.rs` (+719) and `crates/gyre-server/src/spec_registry.rs` (+158), plus coverage-matrix and task-file updates. No changes to `scripts/`, `gyre-domain`, or other crates (the exemption file `scripts/in-memory-state-stores-exemptions.txt` is byte-identical to the assignment base — verified by empty `git diff` on `scripts/`).

## Probes run (evidence in /tmp/stage/review-evidence)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → exit 0 (new tests type-check).
- `bash scripts/check-arch.sh` → pass.
- `bash scripts/check-in-memory-state-stores.sh` → pass, against the **baseline** exemption file; the three grandfathered aliases verified present at `spec_registry.rs:218/220/249` (the `4e2a1b6c` relocation repair is real: pure code movement of the manifest helpers below the ledger-alias section, no verifier or exemption edits).
- `bash scripts/check-inert-enforcement.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-fail-open-ref-resolution.sh`, `check-relative-path-defaults.sh`, `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-mem-port-contracts.sh` → all pass.
- `rustfmt --check` on both touched files → clean.
- `cargo test -p gyre-server` is blocked in the review stage by the build-memory wrapper (gyre-server codegen + model exceeds 4 GiB). Recorded in `05-deferred-verifier-checks.md`; the verifier must run the focused `git_http::tests` and `spec_registry::` suites listed there.

## Verified against the acceptance criteria

- **Manifest gate replaces path-prefix classification.** `process_spec_lifecycle` now parses the diff unfiltered (`parse_name_status`) and reads the manifest at the pushed HEAD via task-193's single `read_manifest` (no duplicate reader — grep confirms one definition; the function was relocated, not copied). Every change is gated through the pure helper `should_create_lifecycle_task(Option<&SpecManifest>, &config, status_char, path, old_path)`: with a manifest, a task is created only for a manifest entry whose `effective_auto_create_tasks(&defaults)` is true; unregistered `specs/` files create nothing (manifest is the source of truth). Manifest-registered specs under non-watched prefixes (e.g. `specs/coverage/…`) are no longer missed — the manifest, not the prefix, decides. Renames match on the NEW path, per the task instruction.
- **No task, no events for excluded specs.** The gate `continue`s before `classify_spec_change`, dedup, task creation, and both the `SpecChanged` audit event and the `SpecChanged`/`TaskCreated` workspace broadcasts — satisfying "emits no TaskCreated/SpecChanged for it".
- **Manifest-less fallback preserved.** `None` manifest → `config.is_watched(path) || old_path watched`, exactly the base `parse_spec_changes_with_config` predicate (either side of a rename), including custom `watched_paths`/`ignored_paths` precedence (`is_watched` checks ignored first). The `06a78d19` repair fixed the earlier new-path-only rename regression in this arm; dedicated unit + e2e regression tests cover old-side-watched renames both to unwatched `specs/` paths and out of `specs/` entirely, with a diff-shape sanity assert (`R100` lines) so the test cannot silently stop exercising renames.
- **Per-spec priority, nothing hardcoded.** `SpecEntry.priority` / `ManifestDefaults.priority` (`Option<TaskPriority>`, serde-defaulted) resolve via `effective_priority` (entry → defaults → None); `None` keeps the `classify_spec_change` change-kind/config default. This is plan option (b) — the field addition is in scope per §15 "manifest can override default task priority". The e2e test asserts `Critical` (manifest override) for spec-b and `High` (change-kind default) in the manifest-less repo.
- **Invalidation kept intact and correctly adapted.** The auto-invalidation block is unchanged except for a `starts_with("specs/")` filter on stale paths, which is required by the now-unfiltered diff parse (otherwise every source-file change would drive spurious revocation calls/logs). The filter is consistent with ledger keying: approvals exist only for manifest-registered specs (`approve_spec` requires a ledger row; `sync_spec_ledger` prefixes manifest paths with `specs/`), so no real invalidation is lost. Invalidation still runs for `auto_create_tasks: false` specs — correct, it is independent of task creation.
- **Dedup, `SpecChanged` audit (task-104), and workspace-scope resolution (task-097 F3: skip-and-log, never a fabricated "default" scope) are preserved** for specs that do get tasks.

## Test quality (no inflation found)

- `should_create_lifecycle_task_manifest_gate` asserts specific true/false per path and change kind, including unregistered-file exclusion, non-watched-prefix inclusion via manifest, and rename new-path-only matching — not "some task exists".
- `spec_lifecycle_manifest_gates_task_creation_e2e` builds a real git repo with a fixture manifest (spec-a `false`, spec-b `true` + `priority: Critical`, spec-c unregistered), drives the real `process_spec_lifecycle`, and asserts **exactly one** task with the exact title `Review spec change: specs/system/spec-b.md`, its `spec_path`, its `Critical` priority, absence of spec-a/spec-c tasks by title and path, and exactly one `SpecChanged` + one `TaskCreated` broadcast with the right `spec_path` payload. Removing the manifest gate (falling back to prefixes) makes this test fail — `specs/system/` is watched, so spec-a and spec-c would spawn tasks — so the test is a genuine guard.
- The two manifest-less e2e tests pin the fallback (one task for the watched path, none for `specs/milestones/`) and the rename either-side rule against base behavior.
- `state_with_registered_repo` and `git_repo_with_commits` follow the established harness patterns in the same test module (workspace `ws-test` under the default tenant, repo row pointing at the temp git repo).

## Non-blocking observations (recorded, no action required)

- The task-194 "Shipped" notes describe the priority fields as "(task-193 schema)"; they were actually added by this candidate (base `1e61f663` has no `priority` field). Documentation-attribution nit only — the addition itself is explicitly permitted by the implementation plan option (b).
- Approval invalidation now runs for all `specs/` diff paths rather than only watched prefixes. This broadens (never narrows) invalidation and matches the manifest-as-source-of-truth model; for unregistered paths it is a no-op (no approval rows can exist).
- The working tree carries an uncommitted revert of `specs/tasks/task-194.md` to the base frontmatter (progress/commits reset) — controller staging behavior, not part of the candidate; the committed candidate `61c1db92` contains the complete task record.

## Required verifier execution (review stage could not run these)

See `/tmp/stage/review-evidence/05-deferred-verifier-checks.md`: `cargo test -p gyre-server --lib git_http::tests` (new gate tests + pre-existing lifecycle regression tests), `cargo test -p gyre-server --lib spec_registry::`, and `cargo build --all`. Optional decisive mutation check: drop the manifest arm of `should_create_lifecycle_task` and confirm `spec_lifecycle_manifest_gates_task_creation_e2e` fails.
