# Review — task-327 (Blast radius computed from provenance)

Spec: `specs/system/meta-spec-reconciliation.md` §4 (blast radius computed
from provenance records + bindings, version-discriminated, per-repo
spec/commit counts, §8 reason labels preserved).
Candidate under review: `6b6a57d2e8166e16daa033d2c2020ed6533e580e`
(branch `pipeline/task-327/12eafd310d7f45b5963d68a3ea493f09-2`, base
`87377fe145ebddf27ef975fba7829fb10f4bb136`; product commits `ded8ebc8`,
`0e3bd118`, repair `7a50c088`; process commits `8610ddc6`, `2f165bc0`,
`017756d8`, `3f5c9ebd`, `a7bb7ffb`, `6b6a57d2` — all product commits
recorded in the task `commits:` frontmatter).
Verdict: **approved** — the round-1 finding (mem/SQLite bound-cast
divergence, review checkpoint `39af8df1`) is repaired on both sides with a
discriminating adapter guard, and a fresh full read of the implementation
and tests found no further defects. Server-side test *execution* is
mechanically deferred to the verifier (see below).

## Probes run (review sandbox, `/tmp/stage/bin/cargo` build wrapper)

- `cargo test -p gyre-adapters --lib sqlite::chain_attestation` → **exit 0,
  17 passed / 0 failed**, including the new guard
  `find_by_repo_u64_max_bound_matches_rows`
  (`adapters_chain_attestation_test.out`; cold build 6m52s).
- **Mutation probe on the guard** (temporary edit, restored; working tree
  verified clean afterwards — only the executor's protected task-text
  restoration of `specs/tasks/task-327.md` remains, untouched by review):
  reverting the clamp to the naive `as i64` cast makes
  `find_by_repo_u64_max_bound_matches_rows` **FAIL** exactly as the round-1
  finding predicted — `left: 0, right: 1`, exit 101
  (`mutation_naive_cast_guard_test.out`). The test kills the real
  production divergence; it is not self-confirming.
- `cargo check -p gyre-server` → **exit 0**, no errors/warnings
  (`server_check.out`, 5m22s cold).
- `cargo check -p gyre-server --tests` → **exit 0** (`server_check_tests.out`,
  4m29s). Coverage of the `#[cfg(test)]` module proven by mutation: a
  deliberate type error injected into
  `blast_radius_rejects_backwards_version_range` makes this check fail with
  `could not compile gyre-server (lib test)`, exit 101
  (`mutation_test_coverage_probe.out`) — so all four new blast-radius tests
  are type-verified by this probe, not just the production code.
- Mechanical invariants: `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-migration-versions.sh`, `check-byte-slice-truncation.sh`,
  `check-unbounded-external-http.sh` → all exit 0 (logs in
  `/tmp/stage/review-evidence/`).
- Full diff read (meta_specs.rs, reconciliation.rs tests,
  sqlite/chain_attestation.rs, docs/api-reference.md, coverage row 8,
  task-327.md shipped notes).

## Round-1 finding — repaired and independently verified

`blast_radius_repo_provenance` passed `u64::MAX` as the "no upper bound"
sentinel to `chain_attestations.find_by_repo`; the SQLite adapter filtered
`created_at.le(until as i64)` and `u64::MAX as i64 == -1`, so the
chain-attestation provenance surface always returned an empty set on
production SQLite while the u64-comparing mem adapter kept the tests
green. The repair (commit `7a50c088`) is correct on both sides:

- **Call site** (meta_specs.rs:746): `find_by_repo(repo_id.as_str(), 0,
  i64::MAX as u64)` with a comment documenting the wrap hazard.
- **Adapter** (sqlite/chain_attestation.rs:268-270): `since`/`until`
  clamped via `.min(i64::MAX as u64) as i64` before the SQL predicate —
  bounds beyond i64::MAX are indistinguishable from i64::MAX for any
  storable i64 timestamp, so the u64 port contract now holds on SQLite
  (mem adapter compares the same bound in u64 semantics; parity verified by
  reading both). This also cures the six pre-existing
  `find_by_repo(..., 0, u64::MAX)` calls in `api/specs.rs` (grep evidence in
  the shipped notes) that carried the same latent divergence.
- **Guard test**: SQLite-backed, seeds one attestation, asserts the
  sentinel bound returns it, that the `i64::MAX as u64` idiom returns it,
  and that a `since` bound above every timestamp still excludes rows
  (clamping does not widen a satisfiable range). Discriminating power
  proven by the mutation probe above.

The remaining `7a50c088` meta_specs.rs hunks are rustfmt-only (verified
hunk-by-hunk); the process commits touch only `specs/tasks/task-327.md`.

## Fresh review of the full implementation (no approval transferred)

- **Version discrimination (deliverable 1).** The request carries the
  change context: `?from_version=` (superseded version, defaulted to the
  registry's current version) and `?to_version=` (must be greater when both
  are given — 400 otherwise, tested). A workspace is affected only when
  its effective binding resolves to the superseded version: a spec-level
  `meta_spec_bindings` pin at exactly `from_version`, or required-flag
  visibility at the registry's current version (required flags have no
  pin; a documented, coherent reading of the registry model). Workspaces
  bound at other versions land in `skipped_workspaces` with their pinned
  version and are excluded from every count and total — the spec's
  "internal-tools on v2" case, asserted by
  `blast_radius_version_discriminates_and_counts_provenance`
  (`skipped == [(ws-br-vb, 2)]`) and by
  `blast_radius_defaults_superseded_version_to_registry_current`.
- **Per-repo counts from provenance (deliverable 2).**
  `blast_radius_repo_provenance` queries three real storage surfaces per
  repo — `agent_commits.find_by_repo` joined with the author's spawn-time
  prompt-set record (`meta_specs_used`, the same kv record merge
  attestations persist), `chain_attestations.find_by_repo` (commit SHA from
  `att.output.commit_sha`, spec identity from the signed input's
  `spec_path`), and merge attestation bundles via
  `merge_requests.list_by_repo` + `attestation_store.find_by_mr_id` (the
  bundle's own `meta_specs_used`). Spec identities and commit SHAs are
  deduplicated across surfaces in HashSets; records governed by other
  versions of the same meta-spec are excluded (the v1/v2-discriminating
  seeds in both count tests). Totals `{repos, specs, commits}` sum only
  repos with provenance work; a repo with no provenance under the
  superseded version gets zero counts in the flat `affected_repos` scan
  and is omitted from the workspace's `repos` work list and from
  `totals.repos` (asserted for `repo-br-va3`). Counts derive from
  provenance records, never from workspace repo membership.
- **§8 labels (deliverable 3).** `reason` stays `workspace_binding` vs
  `tenant_inheritance` (own spec-level pins and Workspace-scoped required
  flags are own bindings; Global/Tenant required flags inherited), plus a
  granular `binding` kind. The pre-existing
  `blast_radius_covers_required_scopes_and_spec_bindings` test was updated
  to the new labels; `blast_radius_labels_and_counts_from_chain_and_merge_
  attestations` proves AC3 (inherited `ws-br-g` and own `ws-br-h`, counts
  computed identically — 1 spec / 1 commit each, from the chain and merge
  surfaces respectively).
- **Read-only + ABAC (deliverable 4).** The route is the unchanged GET
  `/api/v1/meta-specs/:path/blast-radius` (mod.rs:986-987), still mapped in
  the ABAC resolver (abac_middleware.rs:836-839, `meta_spec` resource,
  read); no exemption entries touched (`check-abac-route-registry.sh`
  exit 0). The handler performs no writes.
- **Test strength.** All four new tests assert exact seeded values
  (affected/skipped ids, pinned versions, per-repo (spec, commit) tuples,
  totals) through the real axum router with real auth — removing the
  version filter, any provenance surface, or the label logic breaks them.
  Prompt-record loads are cached per agent per request (no N+1 explosion).

## Non-blocking observations (recorded, not findings)

- `ChainAttestationRepository for PgStorage` is a pre-existing `bail!()`
  stub for the whole port (unchanged by this task; present at the base).
  On a PostgreSQL deployment the chain-attestation surface of the blast
  radius (and of the pre-existing §9 drift sweep and specs.rs callers)
  contributes nothing, since the handler's `if let Ok(chain)` swallows the
  error. This predates the task and spans the entire chain-attestation
  feature; the task's contract (real storage queries) is met on the
  backends that implement the port (SQLite production, mem tests). Worth
  a follow-up task if PG parity is ever required.
- `to_version <= from_version` is only validated when both parameters are
  explicitly supplied; a lone `to_version` below the defaulted
  `from_version` is accepted (it is informational). The spec does not
  require more.

## Verifier must run (deferred — the sandbox wrapper refuses gyre-server
test compilation within the 4 GiB model budget)

    cargo test -p gyre-server --lib blast_radius
    cargo test -p gyre-server --lib reconciliation::

plus the standard verifier suite (workspace tests, all-target Clippy,
architecture checks, CI). The test module is compile-verified in this
review (`cargo check -p gyre-server --tests`, exit 0, coverage proven by
mutation); execution is the remaining deferred step.

## Verdict

**approved** — the round-1 finding is repaired with a discriminating
SQLite-backed guard (independently mutation-tested), the §4 behavior
(version discrimination, per-repo provenance counts, totals, §8 labels,
read-only ABAC-mapped endpoint) is genuinely implemented against real
storage surfaces, and the acceptance criteria are covered by exact-value
tests through the real router. `verdict.json`: approved, no findings.
