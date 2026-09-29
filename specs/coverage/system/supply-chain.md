# Coverage: Supply Chain Security for Agentic Development

**Spec:** [`system/supply-chain.md`](../../system/supply-chain.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 6/14 (4 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Agent Stack: The New Build Environment | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 3 | Agent Stack Components | 3 | implemented | - | AgentStack struct in api/stack_attest.rs with all required fields: agents_md_hash, hooks, mcp_servers, model, cli_version, settings_hash, persona_hash. |
| 4 | Stack Attestation at Push Time | 2 | implemented | - | POST/GET /api/v1/agents/:id/stack endpoints. SHA-256 canonical JSON fingerprinting. |
| 5 | How It Works | 3 | implemented | - | Stack fingerprint computed and stored. Comparison against policy fingerprint. |
| 6 | Forge Verification Policy | 3 | implemented | - | PUT/GET /api/v1/repos/:id/stack-policy endpoints. StackAttestationGate pre-accept gate validates fingerprint matches policy in pre_accept.rs. |
| 7 | gyre-stack.lock | 2 | task-assigned | task-165 | Spec defines lockfile format but no .lock file parsing in codebase. Stack stored as in-memory fingerprint only. |
| 8 | Attestation Levels | 2 | task-assigned | task-165 | Three levels defined conceptually (unattested L1, self-reported L2, server-verified L3) but only L2 default used. No runtime type distinction or enforcement by level. |
| 9 | Level 3: Gyre-Managed Runtime (Highest) | 3 | task-assigned | task-165 | Not implemented — requires SPIFFE workload attestation and server-verified runtime. |
| 10 | Level 2: Gyre CLI on Developer Machine | 3 | implemented | - | Default attestation level. Self-reported stack fingerprint accepted. |
| 11 | Level 1: Raw Git Push (Lowest) | 3 | implemented | - | Pushes without stack attestation get lowest level implicitly. |
| 12 | Policy per Level | 3 | task-assigned | task-165 | Stack policy stored as simple fingerprint. No multi-level validation tree or per-level enforcement rules. |
| 13 | AIBOM (AI Bill of Materials) | 2 | implemented | - | GET /api/v1/repos/:id/aibom returns agent attribution, model, attestation levels, chain data via aibom.rs. |
| 14 | AIBOM Contents | 3 | task-assigned | task-166 | AIBOM endpoint exists but lacks full spec compliance: no structured AIBOM file generation for releases, no release artifact packaging. |
| 15 | AIBOM vs. SBOM | 3 | n/a | - | Comparison/rationale — no implementable requirement. |
| 16 | SLSA Provenance Integration | 2 | task-assigned | task-166 | No explicit SLSA Build Level compliance. No hermetic/reproducible build verification. Provenance endpoint exists but not mapped to SLSA attestation format. |
| 17 | The Laptop Problem | 2 | task-assigned | task-166 | No drift detection for stack changes between attestation and push. No override audit trail. |
| 18 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
