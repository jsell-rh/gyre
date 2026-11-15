# Review: TASK-144 — Implement attestation bundle storage targets

**Reviewer:** Independent review (review stage)
**Candidate:** `070d6d30b40f03ee890a95a594763e2050dd8cb3` (code commit `164c44be`)
**Date:** 2026-04-15
**Verdict:** approved

---

## Scope reviewed

Diff `103d665e..070d6d30`: `AttestationStorage` + `RekorClient` ports (`gyre-ports`),
`MergeAttestationStorage` + `NoopRekorClient` adapters (`gyre-adapters`), storage constants
moved to `gyre-domain`, `AppState` wiring (production `lib.rs`, test `mem.rs`, `middleware.rs`
test-state clone), `store_merge_attestation_bundle` delegation to the ports, merge-processor
test extension, coverage-row and task-record updates.

## Evidence (under /tmp/stage/review-evidence)

- `cargo test -p gyre-adapters --lib attestation_storage` — **7 passed** against a real temp
  git repo and a real SQLite audit store (git note retrievable via raw `git notes show`; audit
  event queryable by MR id via the raw repository AND the port; commit retrieval; audit
  fallback when the note is removed; AIBOM bulk query over three merges; note-failure surfaces
  `Err` while the audit record stays queryable).
- `cargo test -p gyre-adapters --lib rekor` — 1 passed.
- **Mutation probe** (`mutation-note-write-disabled.log`): disabling `write_note` inside
  `store` fails `bundle_stored_as_git_note_retrievable_via_git_notes_show` and
  `store_surfaces_note_failure_but_keeps_audit_record` — the git-note test genuinely kills a
  disabled note write; it is not self-confirming. File restored byte-identical after the probe.
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib` and `--tests` — 0 errors (test target
  included; the extended merge-processor test type-checks).
- `cargo fmt -p gyre-adapters -p gyre-ports -- --check` + `rustfmt --check` on touched server
  files — clean.
- `scripts/check-arch.sh`, `check-in-memory-state-stores.sh`, `check-mem-port-contracts.sh`,
  `check-lossy-secret-conversion.sh`, `check-relative-path-defaults.sh`,
  `check-unbounded-external-http.sh` — all pass.

## Acceptance criteria assessment

- **Port trait** — yes. Signature enriches the task sketch's `store(&self, bundle)` with
  `repo_path` (needed for the git-note target) and `AttestationScope` (workspace/repo identity
  for the audit record — avoids fabricated scope). Retrieval operations as sketched plus the
  AIBOM bulk query.
- **Git note storage** — real: `git notes add -f` via the git binary, retrievable with
  `git notes show` (proven by test + mutation probe). Uses `refs/notes/merge-attestations`,
  not the sketch's `refs/notes/attestations`; verified justified — `refs/notes/attestations`
  is owned by the chain-attestation dual-write (`crates/gyre-server/src/attestation.rs`,
  `CHAIN_ATTESTATION_NOTES_REF`), and the spec names no ref, only "a git note on the merge
  commit (accessible via `git notes show`)".
- **Audit events storage** — real: `merge_attestation_bundle` event with the full signed
  bundle JSON in `detail`, indexed by MR id (`resource_type`/`resource_id`), scoped to
  workspace/repo; queryable by MR id and commit SHA. The scan is complete (limit derived from
  `stats_by_type`, grown under races), and the audit-port query contract (newest-first) is
  documented and honored by the mem adapter (verified in `mem.rs`).
- **Rekor stub** — per the task plan (step 4 explicitly stages a no-op adapter with a TODO and
  a warning). `NoopRekorClient` warns on every submission, `is_configured() == false`, returns
  no entry. The coverage row is honestly kept **in-progress** (not `implemented`) because
  non-repudiation is pending — no fake completion.
- **AIBOM query** — `retrieve_by_commits` bulk query with a dedicated test.
- **Merge processor wiring** — both success paths (single-MR `process_next` at ~line 2745 and
  each atomic-group member at ~line 1512) store through the port; `store_merge_attestation_bundle`
  delegates to `AttestationStorage.store` + `RekorClient.submit`, keeping the
  never-roll-back-the-landed-merge contract (failures logged, surfaced). The git-note write is
  now awaited inline rather than fire-and-forget — an improvement (failures are visible to the
  caller) with no merge-rollback risk (errors are only logged by the caller).
- **Tests** — adapter tests verified here; server tests type-check.

## Deferred to the independent verifier (sandbox 4 GiB test-codegen limit)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_attestation`
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib merge_processor_generates_multi_party_attestation_bundle`
  (extended this round to retrieve the bundle by MR id through the `AttestationStorage` port
  after a real merge cycle — proves the wiring end-to-end)
- Full workspace suite, Clippy all-targets, `check-task-commit-attribution.sh` (needs full
  history; the code commit `164c44be` is listed in the task frontmatter; the docs commit
  `070d6d30` touches only specs/, not product surface).

## Non-blocking observations

1. **Rekor remains a stub** — this is the task's own staged plan and the coverage row says so;
   the §Storage row stays open until a real Rekor API integration lands behind the port.
   When implemented, remember the unbounded-external-http invariant (latency-bounded client).
2. **`retrieve_by_mr`/`retrieve_by_commits` take no workspace scope** — matches the task's
   trait sketch and MR ids are UUIDs; no HTTP route exposes the port yet. If a route is added,
   per-handler ABAC scoping is mandatory (the audit record carries workspace/repo for it).
3. **`read_note` maps any git failure to `Ok(None)`** (debug-logged) and falls back to the
   audit record — documented in the adapter; acceptable since the audit record is the durable
   target.
4. **Working-tree note:** `specs/tasks/task-144.md` arrived in this checkout reverted to its
   pre-task baseline (not-started / empty commits / no Shipped section) — a pre-existing
   working-tree state, not a review edit; the candidate commit itself contains the full record.
