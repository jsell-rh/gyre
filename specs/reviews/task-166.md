# Review — task-166 (Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem)

Spec: `specs/system/supply-chain.md` §"AIBOM Contents", §"SLSA Provenance Integration", §"The Laptop Problem".
Candidate under review: `e91749a7cdbf6d17c263d857e96947408cfd1f95` (base `1f86b0064bdd4e3d14cb1e48bdb61270804fcf42`).
Product commits: `39a0aaec` (feature), `da143273` (round-1 review repair), `edfb0ac8` (rustfmt), `e7d8c215` (round-1 verification repair: nested-tree git test helpers), `442e656f` (round-2 verification repair: git-execution-failure vs unresolvable-ref separation + bounded retries), `44f50411` (round-3 review repair: conversation SHAs + per-commit chain attestation in the release artifact, honest stack-drift total via `count_filtered`).
Verdict: **approved** — all three coverage sections are implemented in real production code; prior review/verification findings verified repaired; no new blocking findings. Full-suite execution (`cargo test --all`) is deferred to the independent verifier per the sandbox build budget (gyre-server test-binary code generation is rejected by the executor wrapper — evidence saved); required probes listed at the end.

## Probes run (all evidence under `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-domain` → **564 + 8 passed, 0 failed** (includes the new `stack::StackDriftEvent` round-trip and `AuditEventType::StackDriftDetected` to/from-string tests).
- `cargo test -p gyre-cli` → **222 + 1 passed, 0 failed** (includes `cli_push_stack_override_reason_parses`).
- `cargo test -p gyre-adapters sqlite::audit` → **15 passed, 0 failed** (includes the new `audit_query_by_repo_and_count_filtered`: `repo_id` narrows `query`, `count_filtered` ignores `limit`).
- `cargo check -p gyre-server --tests` → **clean** (type-checks the full server lib + all test code, 7m40s cold).
- 23 mechanical invariant scripts → all pass (`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-fail-open-ref-resolution`, `check-inert-enforcement`, `check-fabricated-scope-defaults`, `check-scope-literal-defaults`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-lossy-secret-conversion`, `check-forwarded-header-trust`, `check-mem-port-contracts`, `check-migration-versions`, `check-dead-message-kinds`, `check-mcp-write-tools`, `check-in-memory-state-stores`, `check-unbounded-external-http`, `check-unnamed-tuple-carriers`, `check-conditional-test-guards`, `check-identical-baselines`, `check-aspirational-test-names`, `check-abac-context-parity`-adjacent set). Note: the candidate *removed* one line from `scripts/unnamed-tuple-carrier-exemptions.txt` (fixed a pre-existing violation at aibom.rs:178) — exemption shrinkage, never growth.
- `cargo test -p gyre-server --lib aibom::tests::aibom_release` → **rejected by the executor build wrapper** ("gyre-server code generation plus the model exceeds 4 GiB … Server builds/tests belong to the independent verifier"). Not a code defect; recorded as a verifier probe below.
- `check-task-commit-attribution.sh` initially FAILed against the *working tree* because the stage reset `specs/tasks/task-166.md` to assignment-time text (`progress: not-started`, `commits: []`) — the known attribution-clobber pattern (cf. task-208 rounds 121+). Verified the candidate commit `e91749a` itself records all six product SHAs in the frontmatter (manual extraction of the 8-char shorts matches the script's expectations); working tree restored to the candidate's committed state so the checkpoint does not propagate the clobber.

## Verified working

**AIBOM release artifact (§AIBOM Contents)** — `GET /repos/:id/aibom?format=release` (`build_release_aibom`, aibom.rs):
- The release commit set is a real git range (`git rev-list from..to`, default every commit reachable from `to`/default branch), so `human_commits` is counted from actual git history, not the agent-commit table. Unresolvable ref → 404; unknown `format` → 400 (validated before any work); `from`/`to`/`ref` validated by `is_valid_ref` (rejects leading dash, `--`, non-ref chars — flag injection blocked).
- `agents[]` carries commits, `stack_attestation {fingerprint (registered stack from KV `agent_stacks`), level (stored per-agent level, else strongest commit label), verified (fingerprint == committed lockfile fingerprint)}`, model, `mcp_servers`, `spawned_by`, `task_ids`, `sigstore_entries` (Rekor UUIDs via the commit-signature repository), and — the round-3 repair — `conversation_shas` (conversation-blob SHAs via `conversations.list_by_agent`, tenant-scoped through the repo's workspace; workspace unresolvable → SHAs omitted + logged, never a fabricated default tenant; lookup failure → 500, not silent drop).
- `commits[]` — the round-3 repair — carries every commit in the range (human included) with `agent_id`/`task_id`/`timestamp` and the full per-commit `{attestation_id, chain_depth, chain}` block via `chain_attestations.find_by_commit` + `load_chain` (absent when none on record; storage errors surface). I verified the mem `load_chain` includes a depth-0 Signed root itself, so the test's non-empty-chain assertion holds against the real adapter contract.
- `stack_policy {lockfile_hash, all_commits_attested, minimum_level, policy_violations}` (violations = real `stack_drift_detected` audit events) + `summary`.
- `run_git_child` separates "git ran and answered no" (Ok(None) → 404) from "git could not be executed" (spawn failure/signal death → bounded 3-attempt retry, then 500) — the round-2 verification repair; `expect_ok` retains the response body in assertion messages so a future failure names the fired path.

**SLSA provenance (§SLSA Provenance Integration)** — `GET /repos/:id/slsa-provenance?ref=` (`get_slsa_provenance`, provenance.rs):
- in-toto Statement v1 envelope (`_type`, `predicateType: https://slsa.dev/provenance/v1`, subject = resolved ref with `gitCommit` digest, `runDetails.builder/metadata.invocationId`).
- Level mapping evaluated against real state, not hardcoded: Gyre 1 → SLSA 0; Gyre 2 → SLSA 1, or 2 when the subject ref pins an internally-consistent `gyre-stack.lock` (real `read_git_file` + `parse` + `verify_integrity`); Gyre 3 → SLSA 3. `slsaBuildLevel` = weakest link across agent-attributed commits *reachable from the subject ref* (rev-list-scoped; records on unrelated/deleted branches are excluded from both attribution and the level — asserted by `slsa_provenance_statement_shape_and_level_mapping`). Invalid ref → 400; unresolvable → 404.

**Laptop problem (§The Laptop Problem)** — `enforce_stack_policy` (git_http.rs) + `GET /repos/:id/stack-drift` (stack_attest.rs):
- Every drift finding — block-mode rejection, warn-mode rejection for a missing override reason, and warn-mode acceptance — records a full `StackDriftEvent` as a `stack_drift_detected` audit event (both fingerprints, `integrity_verified`, reason naming differing components, mode, outcome, branch, agent, repo, workspace). Drift detection itself (lockfile vs attested stack at the pushed tree) shipped in task-165 and is unchanged in scope.
- Warn-mode acceptance REQUIRES `X-Gyre-Stack-Override-Reason` (whitespace-flattened, 2000-char cap, blank filtered): missing/blank → 403 + refs undone + blocked drift event. The no-policy warn-default path also requires it (test `lockfile_drift_without_policy_warned_and_accepted` updated). CLI: `gyre push --stack-override-reason` sends the header.
- Drift history endpoint: newest-first, repo-scoped at the storage level via the new `AuditQueryFilter.repo_id` dimension (honored by mem, sqlite, and postgres adapters — shared predicates with `query`, so they cannot disagree; indexed wrapper delegates), `?limit` default 100/cap 500; `total` from the new `count_filtered` port method (ignores `limit`) — the round-3 minor repair, with `stack_drift_total_exceeds_list_query_cap` (1001 events, asserts total 1001 + page 100) killing the old capped-at-1000 behavior.
- Routes registered in `api/mod.rs` with ABAC `RouteResourceMapping` entries (registry script confirms); SIEM CEF severity 4 for the new event type; `GET /api/v1/audit/events` gains `?repo_id=` consistent with its existing filter semantics.

**Housekeeping**: coverage rows 14/16/17 → `implemented` with accurate landing notes (row 16 explicitly scopes what the endpoint does and does not claim — hermeticity remains a Level-3 runtime property); `docs/api-reference.md` + `docs/cli.md` updated; task frontmatter records all six product commits; hexagonal boundaries intact (`check-arch` green; `StackDriftEvent` lives in gyre-domain, the port extension is filter data only).

## Test-inflation check

None found. The new tests kill real bugs: weakest-link scoping (an unreachable-ref record must not cap the level), override-reason rejection (403 without the header), cross-repo drift scoping, total-beyond-query-cap, nested-tree git helpers with assertion messages carrying git's stderr. The AIBOM/SLSA tests drive the real router against real bare git repos (real `mktree`/`commit-tree`/`update-ref` object construction), real KV/policy/audit/conversation/chain-attestation stores — not stubs or mirrored logic.

## Scope check

No out-of-scope edits. `web/dist/` untouched by the diff (regeneration is verifier-owned). The `search_sync.rs`/`search_indexer.rs`/`merge_processor.rs`/`siem.rs` touches are the mechanical struct-literal/enum-match consequences of the port extension. Exemption files only shrank.

## Required verifier probes (gyre-server test binary exceeds the review sandbox's build budget)

1. `cargo test --all` (the task's acceptance criterion; last full run at `a00bc270` failed only `api::aibom::tests::aibom_release_format_full_attribution` with 404 — since repaired twice: `e7d8c215` nested-tree helpers, `442e656f` git-execution-failure separation + bounded retries with body-retaining assertions).
2. If focused first: `cargo test -p gyre-server --lib aibom::tests::aibom_release aibom_release_format` — note lib-target filters are OR-ed, so run `aibom::tests::aibom_release` and the SLSA/stack-drift sets separately:
   - `cargo test -p gyre-server --lib slsa_`
   - `cargo test -p gyre-server --lib stack_drift`
   - `cargo test -p gyre-server --lib warn_mode_drift block_mode_drift`
3. `bash scripts/check-task-commit-attribution.sh` on a clean checkout of the merged candidate (passes at `e91749a`; the working-tree failure observed in review was the stage's assignment-text reset, not the candidate).
