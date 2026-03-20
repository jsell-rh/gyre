# M14: Supply Chain Security

## Goal

Every commit has cryptographic proof of the agent configuration that produced it.
The forge can enforce stack policies at push time. Releases include an AIBOM
(AI Bill of Materials) documenting exactly which AI tooling created each piece of code.

## Reference

Full spec: [`specs/system/supply-chain.md`](../system/supply-chain.md)

## Deliverables

### M14.1 Agent Stack Fingerprinting

Domain model and computation for agent stack identity.

- `AgentStack` struct: agents_md hash, hooks list+hashes, mcp_servers, model, cli_version, settings_hash, persona_hash
- `stack_fingerprint()` function: SHA-256 of canonical JSON of all components
- `POST /api/v1/agents/{id}/stack` — agent reports its stack at spawn time
- `GET /api/v1/agents/{id}/stack` — query agent's current stack
- Stack stored in agent record, linked to all commits produced during session

### M14.2 Stack Attestation at Push Time

Verify agent configuration on every push.

- Push metadata includes stack attestation (hash + component details)
- Pre-accept gate `StackAttestationGate`: verifies attestation matches known stack
- `gyre-stack.lock` file support: repos define required stack fingerprint
- Three attestation levels: unattested (raw git), self-reported (CLI), server-verified (managed runtime)
- Attestation level recorded in commit provenance

### M14.3 AIBOM Generation

AI Bill of Materials for releases.

- `POST /api/v1/repos/{id}/aibom?from={tag}&to={tag}` — generate AIBOM for a release range
- AIBOM includes: per-commit agent attribution, stack fingerprint, model used, attestation level
- JSON output format aligned with emerging AIBOM standards
- Dashboard: AIBOM viewer showing agent contribution breakdown per release

## Acceptance Criteria

- [ ] Agent stack fingerprint computed and stored at spawn
- [ ] Push with mismatched stack attestation is rejected (when policy requires it)
- [ ] AIBOM endpoint returns per-commit agent + config attribution
- [ ] Dashboard shows AIBOM with attestation levels
- [ ] `cargo test --all` passes
