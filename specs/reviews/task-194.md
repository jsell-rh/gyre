# Review — task-194 (Manifest-driven spec lifecycle task creation)

Spec: `specs/system/spec-registry.md` §14/§15 — spec lifecycle hooks use the
manifest (not path prefixes) to decide which files trigger tasks; per-spec
priority override; manifest is the single source of truth for policy.
Comparison base: `fefde2946f3a0af1f212588f54591516fee0f7b2` → candidate
`98b5ce45d8f533c5e8a3fa3ac3a33f79c8053948`.
Diff: 4 files — `crates/gyre-server/src/git_http.rs` (+~606),
`crates/gyre-server/src/spec_registry.rs` (+~104),
`scripts/in-memory-state-stores-exemptions.txt` (coordinate refresh),
`specs/tasks/task-194.md` (status/commits). Working tree clean.

Verdict: **needs-repair** (one finding; functional behavior approved, mechanical
gate failure blocks).

## Diff inspection

- **§15 gating is real.** `process_spec_lifecycle` reads the manifest at the
  pushed HEAD via task-193's single reader `crate::spec_registry::read_manifest`
  (no second reader added; verified by grep). The pure gate
  `should_create_lifecycle_task(Option<&SpecManifest>, status_char, path, old_path)`
  decides task creation: with a manifest, only files resolving to a manifest
  entry (via `manifest_entry_for_path`, `specs/`-relative identity) whose
  effective `auto_create_tasks` (per-entry override, else `defaults:`) is true
  create a task; unregistered `specs/…` files create none; renames match the new
  path (explicit task instruction). Manifest-less repos fall back to the legacy
  watched prefixes, with renames admitted when either side is watched — the
  review-repair commit `06a78d19e` correctly restored base behavior there.
- **Priority override is real and minimal.** Optional `priority` on
  `SpecEntry`/`ManifestDefaults` + `effective_priority()`; applied in
  `process_spec_lifecycle` only when the manifest declares one, else the
  `classify_spec_change` change-kind default is kept. No hardcoded priority.
- **Invariants preserved.** Auto-invalidation of stale approvals runs before the
  gate (independent of task creation, per plan item 4); title dedup and
  `SpecChanged`/`TaskCreated` emission fire only for tasks actually created
  (workspace scope resolved from the repo, no fabricated default).
- **Exemption refresh legitimate.** `in-memory-state-stores-exemptions.txt`
  coordinates moved 218/220/249 → 260/262/291 because task-194 inserted code
  above the same three legacy aliases; entry count still 5, frozen count in the
  script untouched.
- `parse_spec_changes`/`SPEC_WATCHED_PATHS` have no callers outside git_http.rs,
  so widening `parse_spec_changes` to all `specs/` paths (eligibility moved to
  the manifest gate) cannot affect other consumers.

## Probes (evidence: /tmp/stage/review-evidence/)

- Focused tests all pass: 4 gate unit tests, 3 lifecycle e2e tests (real git
  repos), 8 `parse_spec_changes` tests, 3 `spec_registry` manifest tests.
- Full touched-crate suite: `cargo test -p gyre-server --lib` → **1495 passed /
  0 failed**.
- **Independent mutation checks** (mutations applied then restored byte-identical;
  `git status` clean after):
  1. manifest gate removed (`Some(_m) => true`) → 3 tests fail (both gate unit
     tests + manifest-gate e2e);
  2. rename either-side-watched fallback reverted → 2 tests fail (0 tasks);
  3. priority-override binding removed → manifest-gate e2e fails (High instead
     of Critical).
  The tests kill removed gates/overrides — not self-confirming.
- `bash scripts/check-arch.sh` pass; `bash scripts/check-in-memory-state-stores.sh`
  pass.

## Finding (blocking)

**rustfmt gate regression (commit `06a78d19e`).**
`python3 scripts/check-rustfmt-diff.py fefde2946` exits 1: git_http.rs changed
lines 1428, 4485, 4486, 4502 need formatting — all four introduced by the
review-repair commit, i.e. the rename-fallback repair reintroduced exactly the
violation class that verification finding `d7872e9e6` had flagged and that
`a13cf69d7` had repaired, so the shipped-notes gate claim is stale for the final
candidate. This runs as a dedicated CI job (`.github/workflows/ci.yml` fmt job)
and in the verification stage, which will fail closed. Remedy: apply
`rustfmt --edition 2021 --config skip_children=true` to git_http.rs — rustfmt's
own diff touches exactly these 4 sites (closure in the gate's None arm, two
`R100` assert lines, one `titles.contains(…)` line) and nothing else; proposed
diff saved at `/tmp/stage/review-evidence/rustfmt-proposed-reformat.diff`.

## Deferred to verifier

- `python3 scripts/check-clippy-diff.py fefde2946` — the script refuses
  workspace-wide builds in the review stage; the verifier must run it.
- `cargo build --all` — gyre-cli not compiled here (all other crates compiled
  via the gyre-server lib test build).

## Coverage section

`spec-registry.md` §15 behavior itself is genuine and mutation-proven; once the
formatting repair lands, the §15 portion of coverage row 14 (and row 15's
task-194 scope) can be promoted by the controller's auditor. The candidate does
not update the coverage matrix itself — consistent with sibling task-193 and the
audit-stage convention (the matrix row still accurately reads `task-assigned |
task-194`).
