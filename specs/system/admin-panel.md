# Admin Panel

- **Credential management** for agents - admin controls what agents can authenticate to.
  - **Vertex AI for Claude Code** is the primary use case.
  - **Claude Max accounts** supported via OAuth + token refresh (secondary use case).
- **Secrets management:** SOPS as default, with **Vault** support as an option.
- **Audit log viewer** - searchable, filterable trail of all agent and user actions.
- **Background jobs** - the server runs scheduled maintenance tasks:
  - Global repository maintenance
  - OAuth state cleanup
  - Remote agent cleanup (expired/orphaned instances)
  - Expired user session cleanup
  - Token refresh (OAuth, API keys, any expiring credentials)
  - Job history cleanup (self-managing)
  - *(more TBD)*
- Full **job history stored and visible** in admin UI - status, duration, failures, logs.
- **Server logs** - accessible from the admin UI.
  - Explicit scoping decision (task-273 review): server logs are
    **process-level operational telemetry** of the gyre-server process.
    Tracing events cannot be reliably attributed to a single tenant
    (startup, background-job and infrastructure spans span tenants), so
    the server-log viewer is deliberately an operational, cross-tenant
    surface restricted to the Admin role. Deployments hosting mutually
    untrusting tenants must treat tenant Admins as trusted operators for
    process telemetry. Tenant-scoped activity trails are served by the
    audit log viewer (tenant_id-scoped), never by the server-log viewer.
