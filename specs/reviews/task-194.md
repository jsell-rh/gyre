# Review: TASK-194 — Manifest-driven spec lifecycle task creation

**Reviewer:** Independent review (pipeline stage)
**Candidate:** `e934b941713448119f8288489933d92af87886af` (base `fefde2946f3a0af1f212588f54591516fee0f7b2`)
**Date:** 2026-09-30
**Verdict:** approved

---

## Scope

Diff base..candidate touches `crates/gyre-server/src/git_http.rs` (+617),
`crates/gyre-server/src/spec_registry.rs` (+104), the frozen
in-memory-state-stores exemption file (line-number updates only, entry count
stays 5), and `specs/tasks/task-194.md` (shipped notes). No other crates,
routes, MCP tools, or migrations affected — the relevant mechanical gates are
arch, rustfmt/clippy changed-lines, in-memory-state-stores, and
task-commit attribution.

## What was verified (evidence in /tmp/stage/review-evidence/)

1. **Contract compliance (code inspection).** `process_spec_lifecycle`
   (git_http.rs:1439) reads the manifest at the pushed HEAD via task-193's
   `crate::spec_registry::read_manifest` (single reader, reused; unparseable
   manifest → `None` → prefix fallback). Auto-invalidation of stale approvals
   runs per changed `specs/` file **before** the task-creation gate, so it
   stays independent of the manifest decision (task plan step 4). The pure
   gate `should_create_lifecycle_task(Option<&SpecManifest>, status_char,
   path, old_path)` matches the task instruction: manifest mode resolves the
   entry on the NEW path (renames included) and requires effective
   `auto_create_tasks`; unregistered files create no task; manifest-less
   repos fall back to the legacy watched prefixes with either-side-watched
   rename semantics matching base `parse_spec_changes`. Priority override
   (`SpecEntry::priority` / `ManifestDefaults::priority` +
   `effective_priority()`) applies only when declared — no hardcoded value.
   Dedup and `SpecChanged`/`TaskCreated` emission fire only for tasks
   actually created. `parse_spec_changes` broadening (all `specs/` paths)
   affects no consumer outside `process_spec_lifecycle` and its tests
   (grep-verified).

2. **Tests run (candidate, unmodified).** `cargo test -p gyre-server --lib
   spec_lifecycle` → 3/3 ok; `… should_create_lifecycle_task` → 4/4 ok;
   `… parse_spec_changes` → 8/8 ok; `… manifest_entry_for_path` → 1/1 ok;
   `… effective_priority` → 2/2 ok; full `cargo test -p gyre-server --lib` →
   **1495 passed / 0 failed** (matches shipped claim). The e2e asserts
   specific inclusion/exclusion by title AND spec_path plus exact event
   counts — not weakened to "some task exists".

3. **Mutation checks (source restored to exact candidate afterwards;
   `git status` clean, HEAD == candidate).**
   - *Manifest gate removed* (manifest branch replaced with prefix
     matching): `should_create_lifecycle_task_manifest_gate`,
     `…_defaults_section_applies`, and
     `spec_lifecycle_manifest_gates_task_creation_e2e` all FAIL (e2e
     produced 3 tasks including spec-a and unregistered spec-c) — the tests
     kill a removed manifest gate, proving the manifest is really read and
     really gates.
   - *Rename fallback removed* (None arm ignoring `old_path`):
     `…_no_manifest_rename_either_side_watched` and
     `spec_lifecycle_without_manifest_rename_from_watched_prefix_e2e` FAIL
     (0 tasks instead of 2) — AC3 regression protection is real.

4. **Gates.** `bash scripts/check-arch.sh` → pass.
   `python3 scripts/check-rustfmt-diff.py fefde2946` → exit 0, changed lines
   clean. `python3 scripts/check-clippy-diff.py fefde2946` → blocked by the
   executor's cargo wrapper (workspace-wide `--all-targets` invocation
   rejected); equivalent focused probe run instead: `cargo clippy -p
   gyre-server --lib --all-features --message-format=json -- -W clippy::all`
   → exit 0, and replicating the gate's changed-lines filter over the JSON
   gives **0 failures on changed lines, 0 errors** (476 pre-existing
   warnings outside the change — repo baseline). Deferred for verification:
   the workspace-wide `--all-targets` clippy run and `cargo build --all`
   (only gyre-server files changed; the focused build compiled the crate and
   its full dependency graph). `bash
   scripts/check-in-memory-state-stores.sh` → pass. `bash
   scripts/check-task-commit-attribution.sh` → pass (all 4 product commits
   in frontmatter).

## Findings

None. Both prior durable findings are confirmed repaired and
mutation-verified in this candidate: the manifest-less rename either-side
fallback (finding c3db9fd41, repaired in 06a78d19e) and the changed-lines
rustfmt regression (finding 2358a16f1, repaired in 1a07e1837).
