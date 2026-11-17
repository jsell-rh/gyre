---
title: "Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem"
spec_ref: "supply-chain.md §SLSA Provenance Integration"
depends_on: [task-165]
progress: ready-for-review
coverage_sections:
  - "supply-chain.md §AIBOM Contents"
  - "supply-chain.md §SLSA Provenance Integration"
  - "supply-chain.md §The Laptop Problem"
commits: ["39a0aaec029ffbfa9025bc0b2d766ca391a15fc1", "da1432739b4c912f2d0de1eca7eed70dd606184c", "edfb0ac8fd0ec757e17453a896332b58a084491f", "e7d8c215b30105da21112f2a91efc9fce9c0398c", "442e656f346c207e45f96bb19863dab7c7f5bd66", "44f5041118a1dabcf198eed43090855fa4d88394"]
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

All three coverage sections are implemented in production code: feature
work in `39a0aaec`, review repairs in `da143273`, a rustfmt pass in
`edfb0ac8`, the round-1 verification repair in `e7d8c215`, the round-2
verification repair in `442e656f`, and the round-3 review repair in
`44f50411`.

### AIBOM release artifacts (§AIBOM Contents)

`GET /api/v1/repos/:id/aibom?format=release[&release=v1.2.3][&from=..][&to=..]`
(aibom.rs `build_release_aibom`) produces the structured release artifact
from real state: the release commit set is a real git range (`rev-list`,
default every commit reachable from `to`/the default branch) so
`human_commits` is counted from actual git history; `agents[]` carries
commits, `stack_attestation {fingerprint, level, verified}` (verified =
registered stack fingerprint matches the `gyre-stack.lock` committed at
the release tip), model, `mcp_servers`, `spawned_by`, `task_ids`,
`sigstore_entries` (Rekor UUIDs per commit), and `conversation_shas`
(the agent's conversation-blob SHAs from the conversation repository,
tenant-scoped via the repo's workspace — the prompt provenance the spec
section calls for; when the workspace cannot be resolved the scope cannot
be determined, so the SHAs are omitted and the skip is logged, never
fabricated with a default tenant); `commits[]` carries every commit in
the release range (agent-attributed and human) with
`agent_id`/`task_id`/`timestamp` (null for human commits) and the
per-commit chain attestation data `{attestation_id, chain_depth, chain}`
(full chain nodes via `chain_attestations.find_by_commit` + `load_chain`,
absent when none is on record); `stack_policy {lockfile_hash,
all_commits_attested, minimum_level, policy_violations}` (drift events
with full override context); `summary` block. Unresolvable refs → 404,
unknown format → 400.

### SLSA provenance mapping (§SLSA Provenance Integration)

`GET /api/v1/repos/:id/slsa-provenance?ref=<sha|tag|branch>` (provenance.rs
`get_slsa_provenance`) emits an in-toto Statement v1 attestation
(`predicateType: https://slsa.dev/provenance/v1`) whose subject is the
resolved ref's commit. Level mapping: Gyre 1 (raw push) → SLSA 0, Gyre 2
(CLI) → SLSA 1 (2 when the subject ref pins an internally-consistent
`gyre-stack.lock`), Gyre 3 (managed) → SLSA 3. `slsaBuildLevel` is the
weakest link across agent-attributed commits reachable from the subject
ref.

### Laptop problem (§The Laptop Problem)

Every drift finding at push time (git_http.rs `enforce_stack_policy`) —
block-mode rejection, warn-mode rejection for a missing override reason,
and warn-mode acceptance — records a `StackDriftEvent` as a
`stack_drift_detected` audit event with the full context (both
fingerprints, integrity_verified, reason naming differing components,
mode, outcome, override_reason, branch, agent, repo, workspace). Warn-mode
acceptance requires `X-Gyre-Stack-Override-Reason` (sent by `gyre push
--stack-override-reason`). `GET /api/v1/repos/:id/stack-drift` queries the
repo-scoped history, newest-first, `?limit` default 100 cap 500; `total`
is a storage-level `count_filtered` count, never bounded by the list
query's internal 1000-event cap.

### Round-3 review repair (44f50411)

F1 (major): the release artifact now includes the two content items the
plan requires that round 2 omitted — per-agent conversation SHAs
(`state.conversations.list_by_agent`, tenant-scoped via the repo's
workspace; unresolvable workspace → omitted + logged, lookup failure →
error) and per-commit chain attestation data (new `commits[]` array with
the full `{attestation_id, chain_depth, chain}` block).
`aibom_release_format_full_attribution` stores a real zstd-compressed
conversation blob and a real attestation chain node and asserts both
fields plus the human-commit null attribution. Coverage row 14 amended.

Minor: `get_stack_drift`'s `total` was the length of the (limit-1000)
list query. `AuditQueryFilter` gained a `repo_id` dimension (honored by
`query` in mem/sqlite/postgres; `stack_drift_events_for_repo` now scopes
at the storage level) and `AuditRepository` gained `count_filtered`
(count ignores `limit`), implemented by mem (shared predicate with
`query`), sqlite + postgres (shared `apply_filters`), and the indexing
wrapper; `get_stack_drift` uses it. New tests:
`stack_drift_total_exceeds_list_query_cap` (1001 events; fails with
total==1000 against the old code) and
`audit_query_by_repo_and_count_filtered` (sqlite).

### Test evidence and deferred probes

In-sandbox (build wrapper permits these):

- `cargo check -p gyre-ports -p gyre-adapters` — pass.
- `cargo check -p gyre-server --lib --tests` — pass (includes the new
  test code).
- `cargo clippy -p gyre-server --lib --tests` — no new warnings in the
  changed files.
- `cargo test -p gyre-adapters --lib sqlite::audit` — 15 passed,
  including the new `audit_query_by_repo_and_count_filtered`.
- Mechanical invariant scripts re-run clean: check-arch,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-inert-enforcement, check-lossy-secret-conversion,
  check-mem-port-contracts, check-relative-path-defaults,
  check-assertionless-tests, check-conditional-test-guards,
  check-aspirational-test-names, check-fail-open-ref-resolution,
  check-dead-test-code.

The build wrapper blocks gyre-server test compilation in this sandbox
(4 GiB budget — see `/tmp/stage/review-evidence/`). The independent
verifier must run:

- `cargo test --all` (the task's acceptance criterion), in particular
  `cargo test -p gyre-server --lib api::aibom` and
  `cargo test -p gyre-server --lib api::stack_attest`, which exercise
  the round-3 changes (new assertions in
  `aibom_release_format_full_attribution`, new
  `stack_drift_total_exceeds_list_query_cap`).
