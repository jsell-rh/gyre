# Review — task-284 (AIBOM dependency-graph snapshot and stale-dependency flags)

Spec: `specs/system/dependency-graph.md` §Version Drift Tracking, third
bullet ("The AIBOM flags stale dependencies in release reports").
Candidate under review: `ba9caef3` (branch
`pipeline/task-284/af0c53e896ca4a8791258bae73a6b2da-2`, base
`9e7cf62e`; product commits `f7a475ee` + `a5d42c8b`, docs `a5bd2953`,
process `6e68a893`/`55c11b31`/`ba9caef3`).
Verdict: **approved** — no findings. Runtime test execution is deferred
to the independent verifier per the stage budget rule (exact command
recorded below and in `/tmp/stage/review-evidence/task-284-verifier-tests.md`).

## Probes run

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --tests` →
  **exit 0** (8m50s; handler + both new probe tests type-check).
- `bash scripts/check-arch.sh` → exit 0.
- `bash scripts/check-inert-enforcement.sh` → exit 0.
- `bash scripts/check-mem-port-contracts.sh` → exit 0.
- `bash scripts/check-task-commit-attribution.sh` → exit 0 against the
  candidate's `specs/tasks/task-284.md` (the working-tree failure is
  solely the executor's protected-task-text restoration resetting
  `commits: []`; the candidate records both product-surface SHAs).
- Full diff read base→candidate (4 files: api/aibom.rs, api-reference
  row, coverage row 13, task file); line-by-line verification of the new
  section builder, both response shapes, and both tests against the
  actual port/adapter/serde semantics.
- Evidence under `/tmp/stage/review-evidence/`.

## Verified working (no findings)

- **Real store, no re-derivation.** `build_dependencies_section`
  (api/aibom.rs) reads `state.dependencies.list_by_repo` (outgoing
  edges) and `list_dependents` (incoming count) and copies the stored
  status/drift/versions verbatim — no `compute_version_drift` call, so
  the graph (maintained by the push-time pass and the daily
  `dep_staleness_check` job) and the AIBOM cannot disagree. Storage
  errors propagate as 500 (`?` + `ApiError::Internal`) — the compliance
  artifact never silently under-reports. Adapter semantics verified in
  all three implementations (mem: retain+push upsert and matching
  filters; SQLite and Postgres: `on_conflict(id).do_update()` upsert,
  list filters on source/target repo id) — production deployments read
  the same values the tests exercise through the mem store.
- **Both shapes carry the section.** `get_aibom` builds the section once
  before the format fork; it is embedded in `AibomResponse.dependencies`
  (default working summary) and `AibomReleaseResponse.dependencies`
  (`format=release`, threaded through `build_release_aibom`). No other
  construction site of either struct exists (grep sweep); `api/release.rs`
  embeds only the spec-registry section, so no other surface is missed.
  Response change is additive for the frontend consumer
  (`ExplorerCodeTab.svelte` reads named fields).
- **Section contents match the task contract.** Per edge: target repo,
  dependency type, detection method, pinned version, target current
  version, version drift, status, explicit `stale` flag
  (`status == Stale`); plus `dependent_count`; plus the
  `stale_dependencies` supply-chain risk list naming each stale edge
  with drift and both versions and a `supply_chain_risk` statement, and
  `stale_count`. Enums serialize snake_case (verified on the domain
  derive attributes); an all-current set shows the edges with empty
  flags; a repo with no edges shows an empty snapshot with a stating
  `note` (`skip_serializing_if` otherwise). The section is never
  omitted.
- **Tests are real probes, not mirrored logic.** Both tests drive the
  real axum router (`api_router().with_state`) with `oneshot` requests
  and assert fixed expected JSON values (statuses "stale"/"active",
  versions 1.2.0/1.5.0, drift 3, counts, risk-string contents, note
  presence/absence via `Value::get`); repos are created through the API.
  `aibom_release_format_flags_stale_dependency` covers AC1 (stale edge
  flagged with pinned/current/drift in the release format), AC2 (second
  GET = all-current, section present, flags empty) and AC3 (resolve the
  edge in the store via the port's upsert, re-GET the SAME release
  inputs → flags change) in one dependency chain;
  `aibom_summary_carries_stale_dependency_flags` covers AC4 plus the
  no-edge empty-snapshot case. Seeding the store with the post-pass
  state (status Stale + drift) rather than driving the dep_staleness
  pass itself is correct scoping: the pass is specced "real and
  unchanged" and already covered elsewhere; the unit under test is the
  AIBOM reading the store.
- **Partition respected.** No release-MR/tag/publish machinery added;
  the only production-code change is the dependencies content of the two
  AIBOM shapes, composing with the planned `release-artifacts-aibom-sbom`
  task (which owns per-release artifact generation).
- **Bookkeeping accurate.** Coverage row 13
  (specs/coverage/system/dependency-graph.md) updated with concrete
  task-284 source evidence only; docs/api-reference.md row updated to
  document the new section; task file records both product-surface
  commits (attribution check passes against the candidate). No unrelated
  rows touched; no generated web/dist changes in the diff.

## Verifier must run

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib aibom` — selects
  `aibom_release_format_flags_stale_dependency` and
  `aibom_summary_carries_stale_dependency_flags` plus the pre-existing
  aibom tests for regression. Deferred from this sandbox per the stage
  budget rule (gyre-server test-binary compilation); type check of both
  tests already passed here.

Generated-output note (non-blocking, for the verifier): the candidate
diff contains no `web/dist/` changes — bundle regeneration is the
verifier's responsibility per the stage partition.
