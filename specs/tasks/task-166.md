---
title: "Supply chain — SLSA provenance mapping, AIBOM release artifacts, laptop problem"
spec_ref: "supply-chain.md §SLSA Provenance Integration"
depends_on: [task-165]
progress: ready-for-review
coverage_sections:
  - "supply-chain.md §AIBOM Contents"
  - "supply-chain.md §SLSA Provenance Integration"
  - "supply-chain.md §The Laptop Problem"
commits: ["5d443e1328ac2e24764cb1c2953e1343de928557"]
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

All three coverage sections implemented in production code (commit
`5d443e13`).

### AIBOM release artifacts (§AIBOM Contents)

`GET /api/v1/repos/:id/aibom?format=release[&release=v1.2.3][&from=..][&to=..]`
(aibom.rs `build_release_aibom`) produces the spec's structured artifact from
real state:

- The release commit set is a real git range (`rev-list from..to`, default
  every commit reachable from `to` / the default branch), so
  `summary.human_commits` is counted from the actual git history, not guessed
  from the agent-commit table. Unresolvable refs → 404; unknown `format` →
  400.
- `agents[]`: commits, `stack_attestation {fingerprint, level, verified}`
  (fingerprint from the agent's registered stack; level from the push-time
  per-agent level or the strongest commit label; verified = fingerprint
  matches the committed `gyre-stack.lock` at the release ref), model,
  `mcp_servers` (name:version), `spawned_by`, `task_ids`, and
  `sigstore_entries` (Rekor UUIDs looked up per commit through the
  commit-signature repository).
- `stack_policy`: `lockfile_hash` (parsed from the committed lockfile),
  `all_commits_attested`, `minimum_level` (repo stack policy),
  `policy_violations` (the repo's `stack_drift_detected` audit events with
  full override context).
- `summary`: total/attested/unattested commits, models used, agents
  involved, human commits.

### SLSA provenance mapping (§SLSA Provenance Integration)

`GET /api/v1/repos/:id/slsa-provenance?ref=<sha|tag|branch>` (provenance.rs
`get_slsa_provenance`) emits an in-toto Statement v1 attestation
(`predicateType: https://slsa.dev/provenance/v1`) whose subject is the
resolved ref's commit. The level mapping is evaluated against real state:
Gyre Level 1 (raw push) → SLSA 0; Level 2 (CLI) → SLSA 1, or SLSA 2 when the
subject ref pins an internally-consistent `gyre-stack.lock` (the lockfile
supplies SLSA 2's version-controlled build process on top of the CLI's
authenticated provenance); Level 3 (managed runtime) → SLSA 3. The overall
`slsaBuildLevel` is the weakest link — the minimum across agent-attributed
commits — and per-commit `{commit_sha, agent_id, gyre_attestation_level,
slsa_build_level}` attribution rides `predicate.internalParameters.gyre`.

### Laptop problem (§The Laptop Problem)

- Drift detection on push (shipped in task-165) now records a
  `StackDriftEvent` (gyre-domain/src/stack.rs) as a `stack_drift_detected`
  audit event for EVERY finding: block-mode rejections, warn-mode
  rejections (missing override reason), and warn-mode acceptances. The event
carries both fingerprints, `integrity_verified`, the drift reason, mode,
  outcome, branch, and the override reason.
- Warn-mode acceptance now REQUIRES an override reason: the push must carry
  `X-Gyre-Stack-Override-Reason` (single line, whitespace-flattened, capped
  at 2000 chars). Missing/blank reason → 403, refs undone, and a blocked
  drift event on the audit trail. This also applies to the no-policy
  warn-default path (lockfile committed without a repo policy).
- `gyre push --stack-override-reason "..."` sends the header.
- `GET /api/v1/repos/:id/stack-drift[?limit=n]` returns the repo-scoped,
  newest-first drift history with full override context for compliance
  review.
- `AuditEventType::StackDriftDetected` added with SIEM CEF severity 4.

### Test evidence

Executed in-sandbox (see /tmp/stage/review-evidence/task-166-evidence.md):

- `cargo test -p gyre-domain` — 572 passed, 0 failed.
- `cargo test -p gyre-cli` — 223 passed, 0 failed (includes the new
  `--stack-override-reason` parse test).
- `cargo check -p gyre-server --all-targets` — clean; clippy warning-location
  set diffed against the pre-change baseline: zero new warnings.
- Mechanical invariant scripts (arch, ABAC route registry, byte-slice,
  dead message kinds, migrations, in-memory state stores, forged scope
  fields, forwarded-header trust, relative path defaults, lossy secret
  conversion, fabricated/scope-literal defaults, inert enforcement,
  unbounded external HTTP) — all OK.

Deferred to the independent verifier (gyre-server test binary exceeds this
sandbox's 4 GiB budget; `cargo test --all` is verifier-owned):

```
cargo test -p gyre-server --lib aibom_release_format
cargo test -p gyre-server --lib aibom_release_format_range_and_unknown_format
cargo test -p gyre-server --lib aibom_release_format_unresolvable_ref
cargo test -p gyre-server --lib slsa_provenance
cargo test -p gyre-server --lib slsa_level2_maps
cargo test -p gyre-server --lib warn_mode_drift
cargo test -p gyre-server --lib block_mode_drift_rejection
cargo test -p gyre-server --lib lockfile_
cargo test -p gyre-server --lib stack_drift_endpoint
cargo test -p gyre-server --lib stack_policy
cargo test -p gyre-server --lib push_below_min
cargo test -p gyre-server --lib cli_push   # gyre-cli, already run here
cargo test --all
```

Behavioral note for reviewers: `lockfile_drift_without_policy_warned_and_accepted`
(git_http.rs) was updated because warn-mode drift now requires the override
reason — the test sends the header and still asserts the ConstraintViolation
event payload per the §7.5 schema.
