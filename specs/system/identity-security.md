# Identity & Security

## Agent Identity Stack

Three layers, each proving something different. All use existing protocols - no custom auth schemes.

### Layer 1: SPIFFE/SPIRE - "This workload is real"

- **SPIFFE** for cryptographic workload attestation *(Loom's weaver uses SVID for mutual TLS auth - strong reference)*.
- The runtime (container, VM, bare metal) proves it's running on authorized infrastructure.
- Each agent gets a SPIFFE ID: `spiffe://gyre.example.com/agent/{role}/session/{id}`
- Agents operate in secure-by-default environments.
- **Federated** via SPIFFE trust bundle exchange between SPIRE servers.

### Layer 2: Gyre as OIDC Identity Provider - "This agent has these permissions"

- **Gyre mints OIDC tokens for agents.** Each agent session gets a short-lived JWT:
  ```json
  {
    "sub": "agent:worker-42",
    "iss": "https://gyre.example.com",
    "task_id": "TASK-007",
    "spawned_by": "user:jsell",
    "scope": ["repo:gyre:write", "mr:create"],
    "exp": 1711036800
  }
  ```
- Tokens are **short-lived and scoped to a task** - no long-lived API keys, no static secrets.
- External systems verify agent identity by trusting Gyre's OIDC issuer (`/.well-known/openid-configuration`).
- This is the same pattern GitHub Actions uses for workload identity federation.
- Cloud providers (GCP, AWS, Azure) already support OIDC workload identity - agents can authenticate natively.

### Layer 3: Sigstore/Fulcio - "This commit was made by this verified agent"

- **Keyless commit signing** via Sigstore.
- An agent with an OIDC identity gets a short-lived signing certificate from Fulcio.
- Every commit is cryptographically signed - the signature proves which agent, on which task, spawned by which user, made the commit.
- No long-lived GPG keys to manage or rotate.
- Signatures recorded in **Rekor transparency log** (public or private instance) for non-repudiation.

### Identity Summary

| Layer | Proves | Protocol | Federated? |
|---|---|---|---|
| SPIFFE | Workload is real, running on authorized infra | mTLS / x509 SVIDs | Yes (SPIFFE Federation) |
| Gyre OIDC | Agent has specific permissions for specific task | JWT / OIDC | Yes (standard OIDC discovery) |
| Sigstore | This artifact was produced by this verified agent | Fulcio + Rekor transparency log | Yes (public or private Rekor) |

### Federation

- Two Gyre instances federate by trusting each other's OIDC issuers.
- An agent from `gyre-a.example.com` authenticates to `gyre-b.example.com` by presenting its JWT - B verifies against A's OIDC discovery endpoint.
- SPIFFE federation handles the workload attestation layer independently.
- No static credentials exchanged between instances.

### Reusability

- This identity primitive is **reusable as a CI building block** - if agents already have attested identity, CI runners are just another agent workload.
- All agent actions are attributable to a verified identity across all three layers.

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
