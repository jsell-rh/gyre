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
    "stack_hash": "sha256:abc123def456...",
    "persona": "security",
    "attestation_level": 3,
    "exp": 1711036800
  }
  ```
- **Stack hash as an OIDC claim.** The agent's stack fingerprint (from `supply-chain.md`) is embedded in the JWT. Any system verifying the token can check not just who the agent is, but what configuration it was running. This is used by:
  - **Spec registry** (`spec-registry.md`): verify agent approvals came from the required stack
  - **Agent gates** (`agent-gates.md`): verify gate agents match the policy's stack requirements
  - **Merge attestation** (`agent-gates.md`): record which stack produced each gate result
- Tokens are **short-lived and scoped to a task** - no long-lived API keys, no static secrets.
- External systems verify agent identity by trusting Gyre's OIDC issuer (`/.well-known/openid-configuration`).
- This is the same pattern GitHub Actions uses for workload identity federation.
- Cloud providers (GCP, AWS, Azure) already support OIDC workload identity - agents can authenticate natively.

### Task-Scoped JWT Claims — Reference

Every JWT minted by `POST /api/v1/agents/spawn` contains these claims:

| Claim | Type | Description |
|---|---|---|
| `sub` | string | Agent ID (`agent:<uuid>`) — globally unique |
| `iss` | string | Gyre server base URL — verifiable via `/.well-known/openid-configuration` |
| `aud` | string | `gyre` (or `GYRE_OIDC_AUDIENCE` if set) |
| `exp` | integer | Unix timestamp of expiry (configurable via `GYRE_AGENT_JWT_TTL`, default 3600 s) |
| `iat` | integer | Unix timestamp of issuance |
| `jti` | string | Unique JWT ID (UUID) — used for revocation lookup |
| `task_id` | string | The TASK-{id} this agent was spawned to implement |
| `spawned_by` | string | Identity of the caller who called `POST /api/v1/agents/spawn` |
| `persona` | string | Agent persona name (e.g., `security`, `accountability`) — if assigned |
| `stack_hash` | string | SHA-256 of the agent's runtime stack fingerprint — null if not attested |
| `attestation_level` | integer | 1=raw subprocess, 2=CLI-managed, 3=Gyre-managed container with attestation |
| `wl_pid` | integer | OS PID of the agent process (present when spawned as local process) |
| `wl_hostname` | string | Hostname of the compute target (present when spawned on remote target) |
| `wl_compute_target` | string | Compute target ID (present when `compute_target_id` was set on spawn) |
| `wl_stack_hash` | string | Alias for `stack_hash` — carried in workload attestation path |
| `wl_container_id` | string | Docker/Podman container ID (present when spawned in container) |
| `wl_image_hash` | string | Container image SHA-256 digest (present when spawned in container) |

**Task-scoping is a hard security boundary:** The JWT's `task_id` claim is verified on every API call that mutates task-owned resources. An agent cannot transition a task it was not spawned for, write to a repo it was not assigned, or open MRs outside its task scope — even if it holds a valid (non-expired) JWT. This prevents a compromised agent from pivoting to unrelated tasks.

**Revocation:** When `POST /api/v1/agents/{id}/complete` succeeds, the `jti` is written to the revocation table. Subsequent requests with the same `jti` receive `401`. Revocation lookup happens before expiry check. Tokens cannot be un-revoked.

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

---

## User Identity

- **SSO compatible** - integrate with enterprise identity providers.
- **Keycloak** is the primary target. Architecture must support adding any provider (Okta, Entra ID, etc.).
- Support enterprise features: **SCIM** for account provisioning, group/role mapping.
- **Full user management:** profiles with display name, username. Email derived from SSO - not user-managed.

### SCIM Provisioning

SCIM 2.0 (RFC 7643 / RFC 7644) is the standard protocol for enterprise IdP-driven user lifecycle management. Keycloak, Okta, Entra ID, and most enterprise IdPs support SCIM as a provisioning protocol — they push user creates, updates, and deactivations to Gyre automatically.

**Why SCIM matters:**
- Without SCIM, large orgs must manually onboard hundreds of users. Not viable.
- SCIM enables just-in-time user deactivation when someone leaves the org (IdP deactivates → Gyre deactivates automatically).
- Group-to-workspace-role mapping: IdP groups map to Gyre workspace roles without manual role assignment.

#### SCIM Endpoints (stub — to be implemented)

Base path: `/scim/v2` (separate from `/api/v1` — SCIM clients expect this path)

| Method | Endpoint | Description |
|---|---|---|
| `GET` | `/scim/v2/Users` | List users (paginated via `startIndex`, `count`) |
| `GET` | `/scim/v2/Users/{id}` | Get user by SCIM resource ID |
| `POST` | `/scim/v2/Users` | Create user (auto-provision on first appearance in IdP) |
| `PUT` | `/scim/v2/Users/{id}` | Replace user (full update) |
| `PATCH` | `/scim/v2/Users/{id}` | Partial update (e.g., deactivate: `active: false`) |
| `DELETE` | `/scim/v2/Users/{id}` | Deprovision (marks user inactive, does not delete) |
| `GET` | `/scim/v2/Groups` | List groups |
| `GET` | `/scim/v2/Groups/{id}` | Get group |
| `POST` | `/scim/v2/Groups` | Create group (maps to Gyre team or workspace) |
| `PUT` | `/scim/v2/Groups/{id}` | Replace group membership |
| `PATCH` | `/scim/v2/Groups/{id}` | Partial group update (add/remove members) |
| `DELETE` | `/scim/v2/Groups/{id}` | Delete group |
| `GET` | `/scim/v2/ServiceProviderConfig` | SCIM capabilities (filterable, sortable, etag) |
| `GET` | `/scim/v2/Schemas` | Resource type schemas |
| `GET` | `/scim/v2/ResourceTypes` | Supported resource types |

**Authentication:** SCIM endpoints accept a dedicated SCIM bearer token (configured via `GYRE_SCIM_TOKEN` env var). This is separate from the agent JWT — SCIM clients are server-to-server, not agent-to-server.

#### SCIM User Schema

Gyre implements a subset of the SCIM core schema (RFC 7643 §4.1) plus enterprise extension (RFC 7643 §4.3):

```json
{
  "schemas": [
    "urn:ietf:params:scim:schemas:core:2.0:User",
    "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User"
  ],
  "id": "<gyre-user-uuid>",
  "externalId": "<idp-subject-claim>",
  "userName": "jsell",
  "displayName": "Jordan Sell",
  "emails": [
    {"value": "jsell@example.com", "primary": true, "type": "work"}
  ],
  "active": true,
  "meta": {
    "resourceType": "User",
    "created": "2024-01-15T10:00:00Z",
    "lastModified": "2024-03-01T12:00:00Z",
    "location": "/scim/v2/Users/<uuid>"
  },
  "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User": {
    "organization": "<tenant-id>",
    "manager": {"value": "<manager-user-id>"}
  }
}
```

**Gyre-specific SCIM behavior:**
- `userName` maps to Gyre `username` (immutable after creation)
- `active: false` → Gyre deactivates user, revokes all active sessions, removes from active agents' `spawned_by` tracking
- IdP group membership → Gyre workspace membership (mapping configured per-workspace by admin)
- SCIM `externalId` maps to Gyre `external_id` (Keycloak `sub` claim)
- Gyre does NOT expose password fields via SCIM (SSO-only auth)

#### Group-to-Workspace Mapping

Workspace admins configure which IdP groups map to which workspace roles:

```
PUT /api/v1/workspaces/{id}/scim-mappings
{
  "mappings": [
    {"idp_group": "cn=platform-team,ou=groups,dc=example,dc=com", "workspace_role": "Developer"},
    {"idp_group": "cn=platform-leads,ou=groups,dc=example,dc=com", "workspace_role": "Admin"}
  ]
}
```

When a SCIM group push adds a user to `platform-team`, Gyre automatically creates a `WorkspaceMembership` record with `Developer` role. When the user is removed from the group, the membership is revoked.

---

## Access Control

- **ABAC (Attribute-Based Access Control)** - not just roles, attribute-driven policies.
- **Fine-grained filtering** on specific resources:
  - Source control hosts
  - Admin panel sections
  - Agent capabilities
  - *(extensible to any resource)*
- Fine-grained security model throughout.

---

## Impersonation

- Support **user impersonation** for enterprise support scenarios.
- Requirements:
  - Target user must **provide a code or explicit approval** before impersonation begins.
  - Impersonated user is **notified** when someone impersonates them.
  - **Full audit trail** of all actions taken during impersonation - clearly attributed to the impersonator acting as the target.
- Authorization pattern inspired by AP2: intent → signed mandate → receipt with full audit trail.
