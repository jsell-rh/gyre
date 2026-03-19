# Identity & Security

## Agent Identity

- **SPIFFE** for cryptographic agent identity *(Loom's weaver uses SVID for mutual TLS auth - strong reference)*.
- Agents operate in secure-by-default environments.
- All agent actions must be attributable to a verified identity.
- **Workload attestation** on remote runners via SPIFFE - every runner proves what it is, not just who it is.
- This identity primitive should be **reusable as a CI building block** - if agents already have attested identity, CI runners are just another agent workload.

## User Identity

- **SSO compatible** - integrate with enterprise identity providers.
- **Keycloak** is the primary target. Architecture must support adding any provider (Okta, Entra ID, etc.).
- Support enterprise features: **SCIM** for account provisioning, group/role mapping.
- **Full user management:** profiles with display name, username. Email derived from SSO - not user-managed.

## Access Control

- **ABAC (Attribute-Based Access Control)** - not just roles, attribute-driven policies.
- **Fine-grained filtering** on specific resources:
  - Source control hosts
  - Admin panel sections
  - Agent capabilities
  - *(extensible to any resource)*
- Fine-grained security model throughout.

## Impersonation

- Support **user impersonation** for enterprise support scenarios.
- Requirements:
  - Target user must **provide a code or explicit approval** before impersonation begins.
  - Impersonated user is **notified** when someone impersonates them.
  - **Full audit trail** of all actions taken during impersonation - clearly attributed to the impersonator acting as the target.
- Authorization pattern inspired by AP2: intent → signed mandate → receipt with full audit trail.
