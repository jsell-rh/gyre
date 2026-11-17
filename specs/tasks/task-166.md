---
title: "Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem"
spec_ref: "supply-chain.md §SLSA Provenance Integration"
depends_on: [task-165]
progress: ready-for-review
coverage_sections:
  - "supply-chain.md §AIBOM Contents"
  - "supply-chain.md §SLSA Provenance Integration"
  - "supply-chain.md §The Laptop Problem"
commits: ["e7d8c215b30105da21112f2a91efc9fce9c0398c", "edfb0ac8fd0ec757e17453a896332b58a084491f", "da1432739b4c912f2d0de1eca7eed70dd606184c", "39a0aaec029ffbfa9025bc0b2d766ca391a15fc1"]
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

All three coverage sections are implemented in production code:
feature work in `39a0aaec`, review repairs in `da143273`, a rustfmt
pass in `edfb0ac8`, and the verification repair in `e7d8c215`.

### AIBOM release artifacts (§AIBOM Contents)

`GET /api/v1/repos/:id/aibom?format=release[&release=v1.2.3][&from=..][&to=..]`
(aibom.rs `build_release_aibom`) produces the structured release artifact
from real state: the release commit set is a real git range (`rev-list`,
default every commit reachable from `to`/the default branch) so
`human_commits` is counted from actual git history; `agents[]` carries
commits, `stack_attestation {fingerprint, level, verified}` (verified =
registered stack fingerprint matches the `gyre-stack.lock` committed at the
release tip), model, `mcp_servers`, `spawned_by`, `task_ids`, and
`sigstore_entries` (Rekor UUIDs per commit); `stack_policy {lockfile_hash,
all_commits_attested, minimum_level, policy_violations}` (drift events with
full override context); `summary` block. Unresolvable refs → 404, unknown
format → 400.

### SLSA provenance mapping (§SLSA Provenance Integration)

`GET /api/v1/repos/:id/slsa-provenance?ref=<sha|tag|branch>` (provenance.rs
`get_slsa_provenance`) emits an in-toto Statement v1 attestation
(`predicateType: https://slsa.dev/provenance/v1`) whose subject is the
resolved ref's commit. Level mapping: Gyre 1 (raw push) → SLSA 0, Gyre 2
(CLI) → SLSA 1 (2 when the subject ref pins an internally-consistent
`gyre-stack.lock`), Gyre 3 (managed) → SLSA 3. `slsaBuildLevel` is the
weakest link across agent-attributed commits reachable from the subject
ref (rev-list of the resolved SHA) — records targeting SHAs on unrelated
or deleted branches are excluded from the attribution and the level.

### Laptop problem (§The Laptop Problem)

Every drift finding at push time (git_http.rs `enforce_stack_policy`) —
block-mode rejection, warn-mode rejection for a missing override reason,
and warn-mode acceptance — records a `StackDriftEvent` as a
`stack_drift_detected` audit event with the full context (both
fingerprints, integrity_verified, reason naming differing components,
mode, outcome, override_reason, branch, agent, repo, workspace). Warn-mode
acceptance requires `X-Gyre-Stack-Override-Reason` (sent by `gyre push
--stack-override-reason`). `GET /api/v1/repos/:id/stack-drift` queries the
repo-scoped history, newest-first, `?limit` default 100 cap 500.

### Verification repair (e7d8c215)

The prior verification round failed 6 tests with bare `commit-tree
failed` panics (aibom ×2, provenance ×4). Root cause, reproduced with
plain git 2.43 (`fatal: path src/a.rs contains slash`, exit 128): the
round-1 test helpers fed slashed paths straight into single-level
`git mktree` with stderr nulled and status unchecked, so the empty stdout
became the tree argument and the following `git commit-tree` failed with
`not a valid object name`. Deterministic — these tests had never executed
before (the agent-sandbox build wrapper blocks server test compilation,
so earlier rounds only ran `cargo check`). `e7d8c215` builds nested trees
recursively (mirroring git_http.rs's proven `write_tree`), asserts status
+ surfaces git stderr on every hash-object/mktree step, and passes `-m`
to commit-tree like every passing helper.

### Test evidence

- `cargo check -p gyre-server --lib --tests` (build wrapper,
  SKIP_WEB_BUILD=1): clean, no warnings in changed files.
- `python3 scripts/check-rustfmt-diff.py 9f446d0`: changed lines clean.
- `python3 scripts/check-clippy-diff.py 9f446d0`: changed lines clean.
- Standalone rustc replica of the fixed helpers (evidence retained at
  `/tmp/stage/review-evidence/helper-logic-probe.rs`): builds the aibom
  3-commit topology (empty root, lockfile-only, lockfile + `src/lib.rs`)
  and the provenance topology (empty-tree init, one `-p`-chained commit
  per extra file), then reads `main:gyre-stack.lock`, `main:src/lib.rs`,
  `main~1:src/a.rs`, rev-list counts, and `rev-list --max-parents=0`
  back through real git — all pass.

### Deferred to the independent verifier

Server test compilation is wrapper-blocked in the agent sandbox (4 GiB
budget). The verifier must run:

- `cargo test --all --quiet` (the assigned repair command),
- `cargo test -p gyre-server --lib api::aibom::tests api::provenance::tests`
  (the six repaired tests, plus the module's other tests).
