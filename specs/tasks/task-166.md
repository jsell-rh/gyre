---
title: "Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem"
spec_ref: "supply-chain.md §SLSA Provenance Integration"
depends_on: [task-165]
progress: complete
coverage_sections:
  - "supply-chain.md §AIBOM Contents"
  - "supply-chain.md §SLSA Provenance Integration"
  - "supply-chain.md §The Laptop Problem"
commits: ["53f87d3dbc5a5e1eeef680542882c159e7998233", "44f5041118a1dabcf198eed43090855fa4d88394", "442e656f346c207e45f96bb19863dab7c7f5bd66", "e7d8c215b30105da21112f2a91efc9fce9c0398c", "edfb0ac8fd0ec757e17453a896332b58a084491f", "da1432739b4c912f2d0de1eca7eed70dd606184c", "39a0aaec029ffbfa9025bc0b2d766ca391a15fc1"]
---

## Spec Excerpt

### §AIBOM Contents (supply-chain.md)

The AIBOM (AI Bill of Materials) captures which AI models, prompts, and tools produced each commit. For releases, the AIBOM should be generated as a structured artifact (JSON) suitable for compliance review.

### §SLSA Provenance Integration (supply-chain.md)

Map Gyre's attestation chain to SLSA (Supply-chain Levels for Software Artifacts) framework. Level 2 requires: version-controlled build process, authenticated provenance. Level 3 requires: hermetic, reproducible builds.

### §The Laptop Problem (supply-chain.md)

When agents run on developer laptops (Level 2), the stack can drift between attestation and push. The forge should detect this drift and maintain an audit trail of overrides.

## Implementation Plan

1. **AIBOM release artifacts**: Extend the existing `GET /api/v1/repos/:id/aibom` endpoint to support `?format=release` that produces a structured JSON document suitable for packaging alongside release artifacts. Include: all agents that contributed commits, their model contexts, attestation levels, conversation SHAs, and the chain attestation data.

2. **SLSA provenance mapping**: Create a `GET /api/v1/repos/:id/slsa-provenance` endpoint that maps the existing provenance data to the SLSA provenance attestation format (in-toto v1). Map Gyre's attestation levels to SLSA Build Levels: Level 1 (raw push) = SLSA 0, Level 2 (CLI) = SLSA 1-2, Level 3 (managed) = SLSA 3.

3. **Laptop problem mitigation**: 
   - On push, if the agent's submitted stack hash differs from the `gyre-stack.lock` committed in the repo, record a `StackDriftEvent` in the audit log
   - If the push is allowed (Warn mode), require the agent to include a `stack_override_reason` field
   - Store override events with full context for compliance review
   - Add `GET /api/v1/repos/:id/stack-drift` endpoint to query drift history

## Acceptance Criteria

- [ ] AIBOM release artifact format generates structured JSON with full agent attribution
- [ ] SLSA provenance endpoint maps to in-toto attestation format
- [ ] Stack drift detection on push creates audit events
- [ ] Override reason required when stack drift is detected in Warn mode
- [ ] Stack drift history queryable via API
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/supply-chain.md` §"AIBOM Contents", §"SLSA Provenance Integration", and §"The Laptop Problem". The existing AIBOM endpoint is in `crates/gyre-server/src/aibom.rs` (or `api/aibom.rs`). The provenance data is in `api/provenance.rs` and `api/agent_tracking.rs`. The audit system is in `api/audit.rs`. For SLSA format, reference the in-toto attestation specification.

## Shipped

All three coverage sections are implemented in production code and marked
`implemented` in `specs/coverage/system/supply-chain.md`: the feature work in
`39a0aaec`, review repairs in `da143273`, a rustfmt pass in `edfb0ac8`, the
round-1 verification repair in `e7d8c215`, the round-2 verification repair in
`442e656f`, the round-3 review repair in `44f50411`, and the round-4
verification repair (the swapped test fixture binding that made the suite
fail) in `53f87d3d`.

### AIBOM release artifacts (§AIBOM Contents)

`GET /api/v1/repos/:id/aibom?format=release[&release=v1.2.3][&from=..][&to=..]`
(aibom.rs `build_release_aibom`) produces the structured release artifact from
real state: the release commit set is a real git range (`rev-list`, default
every commit reachable from `to`/the default branch) so `human_commits` is
counted from actual git history; `agents[]` carries commits,
`stack_attestation {fingerprint, level, verified}` (verified = registered
stack fingerprint matches the `gyre-stack.lock` committed at the release tip),
model, `mcp_servers`, `spawned_by`, `task_ids`, `sigstore_entries` (Rekor
UUIDs per commit), and `conversation_shas` (the agent's conversation-blob SHAs
from the conversation repository, tenant-scoped via the repo's workspace; when
the workspace cannot be resolved the scope cannot be determined, so the SHAs
are omitted and the skip is logged, never fabricated with a default tenant);
`commits[]` carries every commit in the release range (agent-attributed and
human) with `agent_id`/`task_id`/`timestamp` and the per-commit chain
attestation data `{attestation_id, chain_depth, chain}` (via
`chain_attestations.find_by_commit` + `load_chain`, absent when none is on
record); `stack_policy {lockfile_hash, all_commits_attested, minimum_level,
policy_violations}` (drift events with full override context); `summary`.
Unresolvable refs → 404, unknown format → 400.

### SLSA provenance mapping (§SLSA Provenance Integration)

`GET /api/v1/repos/:id/slsa-provenance?ref=<sha|tag|branch>` (provenance.rs
`get_slsa_provenance`) emits an in-toto Statement v1 attestation
(`predicateType: https://slsa.dev/provenance/v1`) whose subject is the
resolved ref's commit. Level mapping evaluated against real state: Gyre 1
(raw push) → SLSA 0, Gyre 2 (CLI) → SLSA 1 (2 when the subject ref pins an
internally-consistent `gyre-stack.lock`), Gyre 3 (managed) → SLSA 3.
`slsaBuildLevel` is the weakest link across agent-attributed commits
reachable from the subject ref (unreachable records are scoped out).

### Laptop problem (§The Laptop Problem)

Every drift finding at push time (git_http.rs `enforce_stack_policy`) —
block-mode rejection, warn-mode rejection for a missing override reason, and
warn-mode acceptance — records a `StackDriftEvent` as a `stack_drift_detected`
audit event with the full context (both fingerprints, integrity_verified,
reason naming differing components, mode, outcome, override_reason, branch,
agent, repo, workspace). Warn-mode acceptance requires
`X-Gyre-Stack-Override-Reason` (sent by `gyre push --stack-override-reason`).
`GET /api/v1/repos/:id/stack-drift` queries the repo-scoped history,
newest-first, `?limit` default 100 cap 500; `total` is a storage-level
`count_filtered` count, never bounded by the list cap.

### Round-4 verification repair (53f87d3d)

The durable finding (`cargo test --all --quiet`, exit 101) failed
`api::aibom::tests::aibom_release_format_full_attribution` with a 404 whose
body was empty, while its identically-set-up sibling passed in the same
sequential run. Root cause: the test fixture helper returned the positional
tuple `(app, state, repo_id, repo_path, c1, c2, conv_sha)` and the failing
test destructured it with `repo_id`/`repo_path` swapped (both `String`, so
`cargo check` could not catch it) — the request URI was built with the repo's
`/tmp/.../rel-*.git` filesystem path in place of the repo id, matched no
route, and fell through to axum's empty-body 404 fallback; the request never
reached the handler, which is why the round-2 (git-child) and round-3
(attribution) diagnoses missed. The fixture now returns the named struct
`ReleaseRepoFixture` and both call sites destructure by field name
(field-swap-hazard elimination, same class get_aibom documents for
`AgentSummary`). Evidence in `/tmp/stage/review-evidence`: a standalone
axum 0.7.9 replica of the exact 315-route table proves the swapped-binding
URI yields 404 with an empty body (the observed signature) and the correct
id yields 200.

### Test evidence

- `cargo check -p gyre-server --tests` — clean (run in this assignment).
- `rustfmt --check` on the changed file — clean.
- Standalone axum 0.7.9 routing replica (locked versions, exact route table):
  `cargo test -p route-replica` — 2 passed (swapped-binding reproduction +
  correct-binding 200); output retained in
  `/tmp/stage/review-evidence/replica-run-output.txt`.
- Deferred to the independent verifier (the build wrapper blocks gyre-server
  test compilation in the model sandbox): `cargo test -p gyre-server --lib
  api::aibom` (all aibom tests, including the three release-format tests),
  then `cargo test --all` (the durable-finding command). Note the failing
  test's post-200 assertions (conversation SHAs, per-commit chain
  attestation, stack policy, summary) execute for the first time after this
  repair; each was cross-checked line-by-line against
  `build_release_aibom`'s implementation in this round.
