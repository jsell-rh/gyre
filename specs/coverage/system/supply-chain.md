# Coverage: Supply Chain Security for Agentic Development

**Spec:** [`system/supply-chain.md`](../../system/supply-chain.md)
**Last audited:** 2026-09-29 (code-verified all 7 `implemented` sections against stack_attest.rs, pre_accept.rs, git_http.rs, constraint_check.rs, aibom.rs)
**Coverage:** 7/14 (4 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | The Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | Agent Stack: The New Build Environment | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 3 | Agent Stack Components | 3 | implemented | - | Partial (code-verified 2026-09-29) — AgentStack struct (api/stack_attest.rs:50) real, fingerprint() computes SHA-256 over canonical sorted-key JSON (stack_attest.rs:71-96). 7/8 spec components present; `plugins` component (spec §Components) absent from struct and fingerprint. |
| 4 | Stack Attestation at Push Time | 2 | implemented | - | Partial (code-verified 2026-09-29) — POST/GET /agents/:id/stack registered (mod.rs:250), StackAttestationGate wired into push flow (git_http.rs:1100-1216): resolves agent fingerprint + repo policy, rejects mismatch and undoes refs. But attestation is self-reported via separate endpoint, NOT the spec's signed attestation embedded in push (no sigstore signature, no runtime block/spiffe_id). |
| 5 | How It Works | 3 | implemented | - | Partial (code-verified 2026-09-29) — fingerprint computed/stored (KV agent_stacks) and compared to repo policy at push time (StackAttestationGate). Missing spec §How It Works pieces: signed attestation JSON in push, runtime{type,spiffe_id,attestation_method} block, sigstore signature via agent OIDC identity. |
| 6 | Forge Verification Policy | 3 | implemented | - | Partial (code-verified 2026-09-29) — StackAttestationGate (pre_accept.rs:192-219): match→Passed, mismatch→Failed, no-attestation-with-policy→Failed. 2 of spec's 4 scenarios; missing: override-permission path (accept-with-warning + audit log) and configurable "accept-with-flag per policy" for no-attestation. |
| 7 | gyre-stack.lock | 2 | task-assigned | task-165 | Spec defines lockfile format but no .lock file parsing in codebase. Stack stored as in-memory fingerprint only. |
| 8 | Attestation Levels | 2 | task-assigned | task-165 | Three levels defined conceptually (unattested L1, self-reported L2, server-verified L3) but only L2 default used. No runtime type distinction or enforcement by level. |
| 9 | Level 3: Gyre-Managed Runtime (Highest) | 3 | task-assigned | task-165 | Not implemented — requires SPIFFE workload attestation and server-verified runtime. |
| 10 | Level 2: Gyre CLI on Developer Machine | 3 | implemented | - | Partial (code-verified 2026-09-29) — derive_attestation_level (constraint_check.rs:902-918) returns 2 for stack-fingerprint-present/no-container; self-reported fingerprint accepted; default level 2 (parse_stack_policy). Spec's "CLI computes and signs the fingerprint" — signing not implemented (no sigstore). |
| 11 | Level 1: Raw Git Push (Lowest) | 3 | implemented | - | Genuine (code-verified 2026-09-29) — derive_attestation_level returns 1 when a workload attestation exists without stack fingerprint, 0 when no record (raw push / no attestation → lowest level). Level derivation wired into merge-constraint evaluation via get_repo_required_attestation_level. |
| 12 | Policy per Level | 3 | task-assigned | task-165 | Stack policy stored as simple fingerprint. No multi-level validation tree or per-level enforcement rules. |
| 13 | AIBOM (AI Bill of Materials) | 2 | verified | - | GET /api/v1/repos/:id/aibom registered (mod.rs:130), get_aibom (aibom.rs:106-304) builds AIBOM from real data: per-agent attribution (name/model/commit_count/level), per-commit attestation level + full chain_attestation loaded from chain_attestations repo, from/to SHA range filtering, ref-injection validation. Structured release-artifact packaging is row 14 (task-166). |
| 14 | AIBOM Contents | 3 | task-assigned | task-166 | AIBOM endpoint exists but lacks full spec compliance: no structured AIBOM file generation for releases, no release artifact packaging. |
| 15 | AIBOM vs. SBOM | 3 | n/a | - | Comparison/rationale — no implementable requirement. |
| 16 | SLSA Provenance Integration | 2 | task-assigned | task-166 | No explicit SLSA Build Level compliance. No hermetic/reproducible build verification. Provenance endpoint exists but not mapped to SLSA attestation format. |
| 17 | The Laptop Problem | 2 | task-assigned | task-166 | No drift detection for stack changes between attestation and push. No override audit trail. |
| 18 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
