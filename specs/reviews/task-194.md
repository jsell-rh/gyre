# Review — task-194 (Manifest-driven spec lifecycle task creation)

Spec: `specs/system/spec-registry.md` §14/§15 (Spec Lifecycle Integration) + §Manifest Rules.
Candidate: `dd38cd9347e77ed0e7491543bb8e9a8c8dbe9826` (base `3af80bd21a8a3452420e014e645eefdfd137f9be`; product commits `236056d9`, `a13cf69d`, `06a78d19`, `1a07e183`, merge repair `e61a835ce`).
Verdict: **approved** — the task contract is met; no blocking findings. Server test execution is deferred to the verifier (build-wrapper restriction, not a code defect).

## Round 1 (2026-11-12)

Probes (evidence: `/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server --tests` → exit 0 (all new unit + e2e tests compile; web build via build.rs succeeded).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-in-memory-state-stores.sh` → OK (exemption-file edit is coordinate-only: the three `spec_registry.rs` line numbers shifted because code was added above them; no entries added or removed — the frozen baseline count is unchanged).
- `bash scripts/check-mem-port-contracts.sh`, `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`, `check-inert-enforcement.sh`, `check-dead-message-kinds.sh` → all OK.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 against the committed candidate state (the staged working tree resets `specs/tasks/task-194.md` to `not-started`/`commits: []`, which is controller bookkeeping, not the candidate; verified by temporarily checking out the committed task file, then restoring the working tree byte-identical).
- `cargo test -p gyre-server --lib git_http::` → blocked by the executor's cargo wrapper (exit 64, 4 GiB budget: server codegen + model). **Verifier must run** `cargo test -p gyre-server --lib git_http::` and `cargo test -p gyre-server --lib spec_registry::` (full list in `probe-summary.md`).

Verified working (no findings):

- **Task creation is genuinely manifest-driven.** `process_spec_lifecycle` reads `specs/manifest.yaml` at the pushed HEAD via task-193's `read_manifest` (no second reader added; `manifest_entry_for_path` is an in-memory lookup over the parsed manifest) and gates every changed file through the pure helper `should_create_lifecycle_task(manifest, config, status_char, path, old_path)`. With a manifest: a task is created only for files with a manifest entry whose effective `auto_create_tasks` (per-entry override, else `defaults:`) is true; unregistered `specs/` files create nothing (manifest is the source of truth); renames match on the NEW path per the task instruction. `classify_spec_change` is change-kind-based only (A/M/D/R), so no path-prefix filter survives inside the manifest arm.
- **Manifest-less fallback preserves base behavior.** `None` arm = `config.is_watched(path) || old_path watched`, including the either-side-watched rename rule from base `3af80bd21` (regression-fixed by `06a78d19` and pinned by `should_create_lifecycle_task_no_manifest_rename_either_side_watched` + the `..._rename_from_watched_prefix_e2e` test, which sanity-asserts the fixture diff really shows `R100` entries). Custom watched/ignored config prefixes are honored (ported from the removed `parse_spec_changes_with_config` pre-filter to gate-level tests).
- **Per-spec priority override, no hardcoding.** `SpecEntry.priority` / `ManifestDefaults.priority` (`Option<TaskPriority>`, serde-defaulted) feed `effective_priority` (entry → defaults → None); `process_spec_lifecycle` applies it only when the manifest declares one, else keeps the change-kind default that itself honors `SpecLifecycleConfig`'s `default_priority_*`.
- **Invalidation, dedup, and events are intact.** The stale-approval invalidation block is byte-identical to base except for a `specs/`-prefix scoping of `stale_paths`, required because diff parsing is now unfiltered (non-`specs/` files have no ledger rows; the revoke would no-op anyway). Dedup (`dedup_open_tasks`), the task-104 `SpecChanged` audit event, workspace-scoped `SpecChanged`/`TaskCreated` emission (scope from the per-push `ws_id` resolution, never fabricated), and `notify_cross_workspace_dependents` are unchanged and run only for specs that pass the gate — excluded specs emit no `TaskCreated`/`SpecChanged`, satisfying AC1.
- **The tests are mutation-sensitive, not self-confirming.** `should_create_lifecycle_task_manifest_gate` asserts `false` for an `auto_create_tasks: false` spec under a watched prefix (fails if the manifest arm is removed or `effective_auto_create_tasks` is ignored), `false` for an unregistered file (fails if unregistered files spawn work), `true` for a registered spec under a non-watched prefix (fails on residual prefix filtering). The e2e `spec_lifecycle_manifest_gates_task_creation_e2e` builds a real git repo (two commits, manifest at HEAD marking spec-a false / spec-b true+`priority: Critical`, spec-c unregistered), runs the real `process_spec_lifecycle` against real state stores, and asserts exactly one task by title AND `spec_path`, priority `Critical` (change-kind default would be `High`), and exactly one `SpecChanged` + one `TaskCreated` on the broadcast channel. The manifest-less e2e pins the prefix fallback (one task for `specs/system/watched.md`, none for `specs/milestones/`, priority `High`).
- **Merge repair is faithful.** `e61a835ce` combines task-158's per-repo `SpecLifecycleConfig` / repo-defined personas / task-104 audit events with the manifest gate without dropping either side: config governs the manifest-less fallback and the master `enabled` switch; persona processing uses its own `parse_persona_changes` filter and is unaffected by the unfiltered parser.

Minor (non-blocking, recorded for completeness):

- `read_manifest` returns `None` both when the manifest is absent and when it fails to parse (it logs a `warn`), so an unparseable manifest falls back to path-prefix behavior — a fail-open edge for task creation. This is the exact call the task plan prescribed, the parse failure is logged, and the debug message ("no specs/manifest.yaml at HEAD") is slightly imprecise for that case. If a future round wants fail-closed semantics for corrupt manifests, that belongs in `read_manifest`'s contract (task-193 scope), not here.
- Invalidation now runs for all `specs/` paths rather than only watched prefixes (documented in the Shipped notes). This aligns with `sync_spec_ledger`'s SHA-change invalidation for manifest entries and no-ops for paths without ledger rows; no regression observed.

Deferred verifier checks (build-wrapper restriction, not code defects):

- `cargo test -p gyre-server --lib git_http::`
- `cargo test -p gyre-server --lib spec_registry::`
