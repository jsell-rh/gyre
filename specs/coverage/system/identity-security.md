# Coverage: Identity & Security

**Spec:** [`system/identity-security.md`](../../system/identity-security.md)
**Last audited:** 2026-04-13 (full audit — reclassification from not-started)
**Coverage:** 8/12 (2 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Agent Identity Stack | 2 | implemented | - | Multi-layer approach: workload attestation (layer 1), OIDC JWT (layer 2), local commit signing (layer 3). |
| 2 | Layer 1: SPIFFE/SPIRE - "This workload is real" | 3 | implemented | - | Pragmatic alternative: workload_attestation.rs provides PID, hostname, compute_target, stack_hash in JWT claims (wl_pid, wl_hostname, wl_compute_target, wl_stack_hash). Not actual SPIFFE/SPIRE but achieves same workload identity goal. |
| 3 | Layer 2: Gyre as OIDC Identity Provider | 3 | implemented | - | oidc.rs serves /.well-known/openid-configuration and /.well-known/jwks.json. EdDSA JWT minting. External systems can verify agent identity via standard OIDC discovery. |
| 4 | Task-Scoped JWT Claims — Reference | 3 | implemented | - | Full AgentJwtClaims struct in auth.rs:46-84 with all spec'd claims: sub, iss, aud, exp, iat, jti, task_id, spawned_by, persona, stack_hash, attestation_level, wl_* fields. Task-scoping enforced per-call. jti revocation on agent complete. |
| 5 | Layer 3: Sigstore/Fulcio | 3 | task-assigned | task-107 | commit_signatures.rs uses local Ed25519 signing. Mode can be set to "fulcio" but falls back to local with warning. No actual Fulcio certificate issuance or Rekor transparency log integration. |
| 6 | Identity Summary | 3 | n/a | - | Summary table — no implementable requirement. |
| 7 | Federation | 3 | implemented | - | Federation JWT validation in auth.rs:537-543, 763-810. Validates JWTs from trusted remote Gyre instances via OIDC discovery. JWKS caching (5 min TTL). SSRF guard (G11-A). |
| 8 | Reusability | 3 | n/a | - | Architectural principle — no implementable requirement. |
| 9 | User Identity | 2 | implemented | - | SSO-compatible JWT auth. User management in api/users.rs. Generic OIDC support (Keycloak, Okta, Entra ID). |
| 10 | SCIM Provisioning | 3 | implemented | - | Full RFC 7643/7644 in api/scim.rs. Users + Groups CRUD. ServiceProviderConfig, Schemas, ResourceTypes. Token auth via GYRE_SCIM_TOKEN. |
| 11 | Access Control | 2 | implemented | - | ABAC policy engine (abac.rs) with attribute-driven policies. RBAC (rbac.rs) with role hierarchy: Admin > Developer > Agent > ReadOnly. Fine-grained filtering on resources. |
| 12 | Impersonation | 2 | task-assigned | task-108 | Not implemented. Spec requires: target user code/approval before impersonation, notification to impersonated user, full audit trail with clear attribution, ImpersonationStarted/Ended events. |
