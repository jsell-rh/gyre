# Review — task-166 (Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem)

Spec: `specs/system/supply-chain.md` §"AIBOM Contents", §"SLSA Provenance
Integration", §"The Laptop Problem".
Commits under review (base `e8f73c06` → candidate `492e6e4d`): `39a0aaec`
(feature), `da143273`/`edfb0ac8`/`e7d8c215`/`442e656f`/`44f50411` (review +
verification repairs), `53f87d3d` (round-4 fixture repair).
Verdict: **approved** — no findings. All probes clean; the three coverage
sections are implemented in production code with real state, real git, and a
real audit trail.

## Probes run (checkout at candidate `492e6e4d`)

- All 20 mechanical invariant scripts (`scripts/check-*.sh`): **PASS** —
  notably `check-abac-route-registry.sh` (both new routes
  `/api/v1/repos/:id/slsa-provenance` and `/api/v1/repos/:id/stack-drift`
  have ABAC resolver entries in `abac_middleware.rs`),
  `check-mem-port-contracts.sh` (new `count_filtered` + `repo_id` filter
  dimension implemented by every adapter: sqlite, postgres, mem, and the
  `IndexingAuditRepository` wrapper), `check-inert-enforcement.sh`,
  `check-task-commit-attribution.sh` (all seven product commits attributed).
- `cargo check -p gyre-server --tests` → **clean** (10m24s; the full server
  test binary exceeds this sandbox's 4 GiB cgroup budget — per assignment,
  test execution is deferred to the independent verifier; list recorded in
  `/tmp/stage/review-evidence/probe-log.md`).
- `cargo test -p gyre-domain stack` → **9/9 passed** (lockfile parse,
  integrity, drift, fingerprint domain logic).
- `cargo test -p gyre-adapters --lib audit` → **21/21 passed**, including
  the new `audit_query_by_repo_and_count_filtered` (repo_id narrows both
  query and count; count ignores `limit` — the port contract).
- `cargo test -p gyre-cli` → **222 + 1 passed**, including the new
  `cli_push_stack_override_reason_parses`.
- `cargo fmt --check` on all five touched crates → clean.
- Full source read of `enforce_stack_policy` (git_http.rs:1982-2230),
  `build_release_aibom`, `get_slsa_provenance`, `get_stack_drift`, the three
  audit adapters, and the domain `StackDriftEvent`/`attestation_level_from_label`.

## Verified working

- **§The Laptop Problem — enforcement is real.** Drift detection reads the
  pushed tree's `gyre-stack.lock` via git (`read_git_file` at
  `update.new_sha`), parses it (TOML), verifies integrity (recomputed
  fingerprint vs embedded), and checks drift against the pushing agent's
  registered stack (`kv agent_stacks`) naming the differing components. All
  three outcomes record a `StackDriftEvent` as a `stack_drift_detected`
  audit event through the real audit repository: block-mode rejection,
  warn-mode rejection when `X-Gyre-Stack-Override-Reason` is missing/blank
  (403 + refs undone), and warn-mode acceptance (reason required and
  recorded). The header read flattens whitespace and caps at 2000 chars;
  `gyre push --stack-override-reason` sends it (multi-valued
  `http.extraHeader` git config). `GET /repos/:id/stack-drift` is
  repo-scoped via the audit repository's new `repo_id` filter dimension,
  newest-first, `?limit` default 100 cap 500, with `total` from the new
  `count_filtered` port method (storage-level count that ignores `limit` —
  the `stack_drift_total_exceeds_list_query_cap` test kills the round-3 bug
  where total was the capped list length).
- **§AIBOM Contents — built from real state.** `?format=release` on
  `GET /repos/:id/aibom` enumerates the release commit set with a real git
  range (`rev-list from..to`, default everything reachable from
  `to`/default branch), so `human_commits` comes from actual git history;
  `agents[]` carries commits, `stack_attestation {fingerprint, level,
  verified}` (registered stack fingerprint vs the lockfile committed at the
  release tip), model, `mcp_servers`, `spawned_by`, `task_ids`,
  `sigstore_entries` (Rekor UUIDs via the commit-signature repository), and
  `conversation_shas` (conversation-blob SHAs, tenant-scoped through the
  repo's workspace with a logged skip — no fabricated default tenant when
  the workspace is unresolvable); `commits[]` carries every commit with
  per-commit chain attestation (`find_by_commit` + `load_chain`, storage
  errors surfaced, absent field when none on record); `stack_policy`
  carries the parsed lockfile hash, attestation coverage, policy minimum,
  and drift events with full override context. Unresolvable refs → 404,
  unknown format → 400. Refs are validated by `is_valid_ref` (rejects
  leading dash, `--`, and non `[A-Za-z0-9._/-]` characters) before any git
  child; `git rev-parse --verify --end-of-options` and a retried
  `run_git_child` that distinguishes "git ran and said no" (404) from "git
  could not run" (internal error).
- **§SLSA Provenance Integration — real mapping, correct shape.**
  `GET /repos/:id/slsa-provenance?ref=` emits an in-toto Statement v1
  (`_type`/`predicateType: https://slsa.dev/provenance/v1`, subject = the
  resolved ref with a `gitCommit` digest). The level mapping is evaluated
  against real state: Gyre 1 → SLSA 0, Gyre 2 → SLSA 1 (2 when the subject
  ref pins an internally-consistent `gyre-stack.lock`), Gyre 3 → SLSA 3;
  `slsaBuildLevel` is the weakest link across agent-attributed commits
  reachable from the subject ref (rev-list scoping — records targeting SHAs
  on unrelated/deleted branches are excluded, per the
  `slsa_provenance_statement_shape_and_level_mapping` test). The task plan's
  "Level 2 = SLSA 1-2" is honored by the lockfile-conditional 1↔2 mapping.
- **Test quality — no inflation found.** The new tests drive the real
  router with real bare git repos (per-test unique repo names to avoid
  parallel git-dir collisions), record attribution through the real
  agent-commit repository, and assert against the real audit store
  (`latest_drift_event` deserializes the stored `stack_drift_detected`
  detail). Cross-repo scoping, cap-vs-total, flag-injection refs (400),
  unresolvable refs (404), and the weakest-link/unreachable-ref semantics
  are all asserted with outputs that fail when the behavior is disabled.
  The round-4 fixture repair (named `ReleaseRepoFixture` struct replacing
  the positional 7-tuple whose `repo_id`/`repo_path` swap caused the empty
  404) eliminates the field-swap hazard class at both call sites.
- **Docs and coverage updated**: `docs/api-reference.md` rows for all three
  endpoints (including the git-receive-pack `X-Gyre-Stack-Override-Reason`
  header), `docs/cli.md` push example, coverage rows 14/16/17 →
  `implemented` with accurate landing notes (row 16 honestly notes the
  endpoint maps the chain to SLSA and does not itself verify hermeticity —
  a Level-3 runtime property). SIEM CEF severity 4 for the new event type;
  `AuditEventType::StackDriftDetected` round-trips and has an emitter.

## Deferred to the independent verifier

`cargo test -p gyre-server --lib api::aibom`, `api::provenance`,
`api::stack_attest`, `git_http` (drift tests), then `cargo test --all` (the
durable-finding command). These exceed the model sandbox's 4 GiB cgroup
build budget; everything type-checks cleanly and all smaller-package suites
pass in-sandbox.

## Non-blocking observations (recorded, not findings)

- `stack_drift_events_for_repo` (AIBOM `policy_violations` source) uses an
  internal 1000-event query cap, documented in code; the stack-drift
  endpoint's `total` is uncapped via `count_filtered`.
- `get_slsa_provenance` per-commit attribution does not dedup multiple
  agent-commit records for one SHA (the AIBOM builder does); duplicates
  cannot change the min-level, only repeat list entries.
- The CLI flattens whitespace in `--stack-override-reason` but does not cap
  length at 2000 chars as the server-side header read does; the server-side
  cap bounds what is persisted.
- `web/dist` was not rebuilt in this checkpoint — generated output owned by
  the deterministic verifier (per assignment).
