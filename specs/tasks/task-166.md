---
title: "Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem"
spec_ref: "supply-chain.md §SLSA Provenance Integration"
depends_on: [task-165]
progress: ready-for-review
coverage_sections:
  - "supply-chain.md §AIBOM Contents"
  - "supply-chain.md §SLSA Provenance Integration"
  - "supply-chain.md §The Laptop Problem"
commits: ["39a0aaec029ffbfa9025bc0b2d766ca391a15fc1", "da1432739b4c912f2d0de1eca7eed70dd606184c"]
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

All three coverage sections are implemented in production code: the
original feature work in `39a0aaec`, repaired in `da143273` after review.

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
weakest link across agent-attributed commits **reachable from the subject
ref** (rev-list of the resolved SHA) — records targeting SHAs on unrelated
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

### Review repair (da143273)

- `git_rev_list` placed `--` before the range argument, so git treated the
  range as a pathspec and exited 129 for every input — `?format=release`
  always 404'd. Fixed (separator dropped; the range is a single argv
element that cannot occupy an option position) and verified against a real
  bare repo with git 2.43.0.
- `get_slsa_provenance` attributed every agent commit ever recorded for
  the repo; now scoped to the subject's history (finding 2 above).
- `get_stack_drift` applied no default limit when `?limit` was omitted,
  returning up to the internal 1000-event query cap while the docs claimed
  default 100/cap 500; behavior now matches the documented bounds.
- Two latent test defects fixed by hand-tracing (the release tests never
  got past their first assertion while rev-list was broken): the release
  fixture's tip tree lacked `gyre-stack.lock` (`commit_files` builds each
  tree from only the given files), and the SLSA subject-name assertion
  checked for a repo name the fixture never creates.

### Test evidence

- `cargo check -p gyre-server --all-targets` passes (this sandbox's build
  wrapper defers gyre-server test execution to the independent verifier).
- Real-git probe of the rev-list defect and fix: exit 129 before, correct
  commit sets after (single-SHA and `from..to` forms) — recorded in
  `/tmp/stage/review-evidence/probe-rev-list.md`.
- Verifier must run: `cargo test --all` (acceptance criterion), plus the
  focused `aibom` / `slsa` / `drift` gyre-server test filters listed in
  `/tmp/stage/review-evidence/task-166-repair.md` — including the updated
  `slsa_provenance_statement_shape_and_level_mapping` (asserts an
  unreachable-ref record is scoped out) and the new
  `slsa_weakest_link_among_reachable_commits`.
