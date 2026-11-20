---
title: "Admin Panel completion: SOPS/Vault secret backends, durable job history with logs, maintenance jobs, complete token refresh, server-log viewer, durable searchable admin audit"
spec_ref: "Spec gap spec-5b647653a4554eea3e93ac19 / admin-panel-remaining-gap / e61ffd082155"
depends_on: []
progress: ready-for-review
commits: ["4647377515f604b9df541e77c570aa0ca53fff4d", "815b24d9bbcb59c8222a9c69176886ffd107f0b7", "7548ead498317f2de4606238ee1e6cfb53eeac60", "7fe530de59c902ef18f9aadc7448aa6ff8acb3fe", "fa69e871b93d4c6e1cf2f6aeb2b39332dd9d7524", "44eb55a239681f4510d8388525ddf7a3257226dd", "3f51333665c3ad4284f27996e79c14af545db856", "56e5ea5f797f1f19037b40e65f529239c550d649", "a0dc3e637e6ffbae976675b755abf0b2891bf481", "4c71fe4827a62f792912b4e5ab257a50c1882860", "d6f32bd0269314f9ee552ed72df33322b0fe2238", "cd2f616e3ebd96addab2dcaf78242d31c58efc04", "808e3c10c035c9fc6fe8c5ee96d474fc5468acf3", "b979fefeeb7a6bb8340ccd85c64b467e0f46c553", "210dfa61bc1ea637a29a79c26af80746b2fb002c", "3ebd143d29eb52bb2bc0bd748f60fe52fa960f66", "9b81c9816f13a3d637057a6c3149c9be92f38a3c", "0fda00bfe9860c8c8b66ea1235f0d2e3251df67a", "1bc00b6cb9212e6ac6dfc7c764afede98ae8fd64", "a2b31ac19c05310ebd05a072ddde1acf0c51f2ec", "f6e31cf57779a7e117f5a0b9bd75720aed90ca56", "b101d27aafb7bde0907a568e7bb8a3ffbdd3ae67", "ee253c8854f16664f91d16171e08414effc314b7", "db320101b1540a35cf9986408e70ac46008b1982"]
---

## Required behavior

Close every unimplemented bullet of specs/system/admin-panel.md while preserving the satisfied parts (secrets API + scope cascade + cred-proxy delivery, OAuth refresh-token rotation, session cleanup, job registry/scheduler/Jobs tab, durable /audit/events viewer). Deliverables, each a real production implementation: (1) Secrets management - SOPS default, Vault option (platform-model.md §7 Storage Backend is the canonical cross-reference). Add a secret at-rest backend selection (GYRE_SECRET_BACKEND, default sops). SOPS backend: secret values are encrypted at rest as genuine SOPS documents (SOPS JSON format with an age-encrypted data key wrapped for the configured age identity - GYRE_SOPS_AGE_KEY or key file; an auto-generated identity persisted server-side only when none is configured) stored in the existing database columns; decryption happens server-side on resolve/rotate, and the stored ciphertext must be real SOPS format (decryptable with the sops CLI given the identity), replacing the AES-256-GCM app-managed format as the default; secrets stored before the change must remain resolvable after upgrade (re-encrypt at startup or first access). Vault backend (GYRE_SECRET_BACKEND=vault, GYRE_VAULT_ADDR + auth config): the secrets port operations (create/list/delete/rotate/resolve_for_agent) are served from Vault KV v2 with Gyre as intermediary - agents never receive Vault credentials or talk to Vault directly; per §7, resolution for an agent authenticates to Vault with that agent's JWT (Vault JWT auth backend) so Vault policies scope the returned secrets, while the admin write path uses the server's Vault token/AppRole. The Vault HTTP client MUST be latency-bounded (30 s total timeout). With Vault configured and unreachable, secret operations fail closed with an error - never silently fall back to the database store. Binary secret values must never pass through String::from_utf8_lossy. (2) Job history - stored, durable, with logs. Add a logs field to JobRun and capture per-run log lines (a run-scoped sink job handlers write to, or an equivalent tracing capture). Persist job runs through a new gyre-ports JobRunRepository (record/list_by_job/list_recent/delete_older_than) with SQLite + PostgreSQL adapters (next unused migration sequence numbers; SQL portable across BOTH backends), a mem adapter enforcing the same port failure constraints in code, and store! wiring in build_state - the in-memory HashMap history becomes a fronting cache at most, with the durable store authoritative. GET /api/v1/admin/jobs serves persisted history including logs and derived duration; the TenantSettings Jobs tab renders status, duration, failures and logs. Job history cleanup (self-managing): a registered+scheduled job_history_cleanup job prunes runs older than a configurable retention and is visible in the Jobs UI like every other job. (3) Global repository maintenance. New registered+scheduled repo_maintenance job (daily wall-clock) that iterates every stored repository (RepoRepository + repos_root) and performs real git maintenance (gc / prune of unreachable objects / repack) through the server's git execution layer, recording per-repo outcomes in the run logs; a repo whose maintenance fails is recorded as failed without silently aborting the others. (4) OAuth state cleanup. Implement durable OAuth state storage and the real flow that creates it: admin-initiated Claude Max account enrollment (the spec's secondary use case). An Admin-only initiation route generates an OAuth 2.0 authorization request (state + PKCE verifier) persisted via a new OAuthStateRepository port (adapters + portable migration; state rows carry the initiating admin and tenant - scope fields derived server-side, never client-supplied - plus an expiry), then redirects to the provider; the callback handler validates the state row, exchanges the code at the provider token endpoint (latency-bounded HTTP), and stores the resulting refresh token as the scoped claude_max_refresh_token secret, which the existing refresh job then keeps fresh. New registered+scheduled oauth_state_cleanup job deletes expired state rows. All new routes registered in the ABAC RouteResourceMapping (no exemption-file growth) with per-handler authorization. (5) Token refresh - API keys, any expiring credentials. Generalize rotation dispatch in secret_rotation_check so EVERY expiring secret is dispatched through the handler registry (matching on credential-document shape, not name suffix alone): (i) the RFC 6749 refresh-token grant handler applies to any secret whose value is an OAuth credential document (token_endpoint/client_id/refresh_token) regardless of name; (ii) a new GCP service-account handler (Vertex AI, the spec's primary use case) exchanges SA JSON for short-lived access tokens via a real RS256-signed JWT-bearer grant to Google's token endpoint (real signing, latency-bounded HTTP), re-storing the refreshed document and expiry; (iii) API-key or other secrets carrying a refresh document are refreshed by the document-driven handler; secrets with no refresh mechanism are escalated via the existing SecretExpiring notification with the per-secret outcome (refreshed / no-refresh-mechanism / failed: reason) recorded in the durable job-run logs - no expiring credential is silently skipped. (6) Remote agent cleanup (expired/orphaned instances). Persist spawned instance handles: at spawn, record (agent_id, backend descriptor, handle) in a durable agent-instance store (port + adapters + portable migration); mark terminated on clean exit/kill. New registered+scheduled remote_agent_cleanup job that kills still-recorded-running instances whose agent is Dead/Stopped or absent (including orphans left by a server restart) via ComputeTarget::kill_process, marks them terminated, and records what was reaped in the run logs. The in-memory process_registry/spawned_backends remain the fast path; the durable store is the source of truth for orphan sweeps. (7) Server logs in the admin UI. Add a tracing capture layer in telemetry.rs (bounded ring buffer of formatted server log events with timestamp/level/target/message, installed alongside the existing stdout/JSON layers without changing them), an Admin-only GET /api/v1/admin/server/logs endpoint (level/since/limit filters, ABAC-registered), and a Server Logs tab in TenantSettings rendering it. (8) Audit log viewer - durable admin trail + free-text search. Rewrite GET /api/v1/admin/audit to query the durable audit repository instead of the in-memory TelemetryBuffer: add tenant_id to AuditEvent (derived server-side from the acting identity at emit - never client-supplied) with a portable migration backfilling it from workspace scope; add tenant_id and a free-text q filter to AuditQueryFilter enforced in SQL by both the SQLite and PostgreSQL adapters (matching over the detail payload, transparently decompressing zstd-compressed details); update the TenantSettings Audit tab to render the durable events with the same filter set (including search) used by /audit/events. A server restart must not lose the admin-visible trail. Cross-cutting invariants: hexagonal boundaries; new durable state only via gyre-ports + adapters with mem-contract parity (check-mem-port-contracts, check-in-memory-state-stores); next unused migration numbers with SQL portable to SQLite AND PostgreSQL; every new route in the ABAC resolver with no exemption growth; latency-bounded outbound HTTP; conventional commits with task commit attribution.

## Acceptance Criteria

- [ ] SOPS default: with GYRE_SECRET_BACKEND unset, a secret created via POST /api/v1/secrets is stored as a genuine SOPS document - exporting the stored value and decrypting it with the sops CLI (or a spec-conformant native age decryption of the SOPS data key + payload) yields the original plaintext - and resolve_for_agent still injects it as GYRE_CRED_* at spawn; a secret created before the change (AES-256-GCM at rest) remains resolvable after the upgrade.
- [ ] Vault option: with GYRE_SECRET_BACKEND=vault against a real Vault dev server (or a test HTTP server speaking the Vault KV v2 and JWT-auth API), create/list/delete/rotate and agent-scoped resolve are served from Vault KV v2, agent resolution authenticates with the agent's JWT per platform-model §7, agents never receive Vault tokens, and with Vault stopped the operations return errors (fail closed) rather than falling back to the database store.
- [ ] Job history durability: after recording job runs, tearing down the server process and rebuilding AppState against the same database, GET /api/v1/admin/jobs still returns the runs with status, derived duration, error and logs; each run's logs field contains the per-run captured log lines (e.g. per-repo maintenance or per-secret refresh outcomes).
- [ ] Job history cleanup: the registered job_history_cleanup job, triggered via POST /admin/jobs/job_history_cleanup/run, deletes runs older than the configured retention from the durable store (verified by querying before/after) and appears in GET /api/v1/admin/jobs like every other job.
- [ ] Repo maintenance: triggering repo_maintenance against storage containing at least two real git repos (one seeded with unreachable objects) measurably reclaims space (git count-objects -v before/after differs), and the run's logs record per-repo outcomes, including a failure for a deliberately corrupted repo without aborting the others.
- [ ] OAuth state: initiating Claude Max enrollment as Admin creates a durable state row (state + PKCE verifier + expiry + initiating admin/tenant); completing the callback with the valid state exchanges the code at the token endpoint (stubbed provider in tests) and stores a claude_max_refresh_token secret; a replayed or expired state is rejected; running oauth_state_cleanup deletes only expired state rows.
- [ ] Token refresh coverage: an expiring secret carrying an OAuth refresh document under a non-_refresh_token name is refreshed by the job (token endpoint called, value rotated, expires_at extended); a GCP service-account secret is exchanged for a short-lived access token via a real RS256 JWT-bearer grant; an expiring API key with no refresh mechanism produces a SecretExpiring notification AND a recorded no-refresh-mechanism outcome in the run's durable logs - the run record accounts for every expiring secret.
- [ ] Remote instance cleanup: spawn an agent on a real compute target, record its instance, mark the agent Dead (or restart the server leaving the instance recorded as running), then run remote_agent_cleanup: the instance is killed via ComputeTarget::kill_process (the process/container is gone), the durable record is marked terminated, and the run logs name the reaped instance; instances of live agents are untouched.
- [ ] Server logs: GET /api/v1/admin/server/logs returns recent gyre-server log events (timestamp/level/target/message) captured by the new layer - a request that produces a recognizable server-side log line is retrievable - with level and since filters honored; non-Admin callers get 403; the route is in the ABAC resolver and scripts/check-abac-route-registry.sh passes with no new exemptions.
- [ ] Admin audit durability + search: events recorded before a server restart appear in GET /api/v1/admin/audit after the rebuild (durable store, not TelemetryBuffer); results are scoped to the caller's tenant (a second tenant's events are not visible); a free-text q match against a detail payload returns only matching events, including events whose detail was zstd-compressed by the log-compression job; the TenantSettings Audit tab renders the durable events with working filters and search.
- [ ] No regressions in satisfied behavior: the Vertex AI primary path (scoped GYRE_CRED_GCP_SA_JSON delivery + cred-proxy metadata emulator), the existing claude_max_refresh_token rotation, expired-session cleanup, and the /audit/events viewer keep their existing tests passing; scripts/check-arch.sh, check-migration-versions.sh, check-migration-sql-portability.sh, check-mem-port-contracts.sh, check-in-memory-state-stores.sh, check-abac-route-registry.sh and check-unbounded-external-http.sh all pass with no new exemption entries.

## Verification intent

Run each probe against the assigned base and it must fail: (1) Restart probe - build a server with GYRE_DATABASE_URL, trigger a job via POST /admin/jobs/{name}/run, kill the process, rebuild against the same DB, GET /api/v1/admin/jobs: today run history lives only in Arc<Mutex<RegistryInner>> HashMap<String, VecDeque<JobRun>> (jobs.rs, capped at 50) so the response is empty and no logs field exists anywhere - the probe asserts non-empty persisted history with logs, failing until the durable JobRunRepository exists. (2) SOPS probe - export a stored secret's value today: it is ring AES-256-GCM ciphertext (sqlite/secret.rs encrypt_value), not a SOPS document, so sops decryption of the blob fails; the probe asserts SOPS-format ciphertext decryptable with the configured age identity. (3) Server-logs probe - GET /api/v1/admin/server/logs today returns 404 (api/mod.rs registers only /api/v1/agents/:id/logs routes; TenantSettings has no server-logs tab); the probe asserts 200 with captured events for an Admin caller. (4) Maintenance-job probe - POST /admin/jobs/repo_maintenance/run (and oauth_state_cleanup, remote_agent_cleanup, job_history_cleanup) today fails with 'job not found' (start_job_registry registers none of them, main.rs spawns no such loops); the probe asserts each job exists and does real work (git objects pruned per git count-objects; only expired state rows deleted; an orphaned instance actually killed via ComputeTarget::kill_process). (5) Admin-audit probe - record audit events, restart the server, GET /api/v1/admin/audit: today it reads only state.telemetry_buffer (admin.rs admin_audit, in-memory ring buffer) so the trail is empty, and no free-text q filter exists on any audit route; the probe asserts durable post-restart results, tenant scoping, and a q match. Every probe must fail on the current code and pass only with the production behavior above - none may be satisfied by a stub, no-op handler, or unenforced flag.

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/admin-panel.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `c52f9b2fb67229f530193cd0ff524e87a3aaaf053ab3f140bdcfb19ba25120e9`.

[
  {
    "id": "42df7a7e48e1904b76eface1",
    "decision": "gap",
    "requirement_quote": "- **Credential management** for agents - admin controls what agents can authenticate to.\n  - **Vertex AI for Claude Code** is the primary use case.\n  - **Claude Max accounts** supported via OAuth + token refresh (secondary use case).\n- **Secrets management:** SOPS as default, with **Vault** support as an option.\n- **Audit log viewer** - searchable, filterable trail of all agent and user actions.\n- **Background jobs** - the server runs scheduled maintenance tasks:\n  - Global repository maintenance\n  - OAuth state cleanup\n  - Remote agent cleanup (expired/orphaned instances)\n  - Expired user session cleanup\n  - Token refresh (OAuth, API keys, any expiring credentials)\n  - Job history cleanup (self-managing)\n  - *(more TBD)*\n- Full **job history stored and visible** in admin UI - status, duration, failures, logs.\n- **Server logs** - accessible from the admin UI.",
    "detail": "The Admin Panel spec is a single section mixing several realized and several unimplemented requirements; because the unimplemented ones are concrete normative bullets, the section is a gap overall.\n\nSATISFIED parts:\n(1) Credential management for agents is real. The Admin secrets API (POST/GET/DELETE /api/v1/secrets, POST /secrets/:id/rotate, Admin-only via ABAC + in-handler require_admin) stores secrets scoped tenant/workspace/repo/task with AES-256-GCM encryption at rest. At spawn, resolve_for_agent (real SQLite and PostgreSQL implementations) resolves the scope cascade (nearest scope wins) and injects GYRE_CRED_* env vars; the cred-proxy sidecar (docker/gyre-agent/cred-proxy.mjs) scrubs them from process.env, injects x-api-key on Anthropic /v1/* calls, and runs a GCE metadata emulator that exchanges GYRE_CRED_GCP_SA_JSON for Vertex AI tokens. Vertex mode env (CLAUDE_CODE_USE_VERTEX, ANTHROPIC_VERTEX_PROJECT_ID, CLOUD_ML_REGION) is forwarded to the agent container, and a real Vertex AI LLM adapter exists (crates/gyre-adapters/src/llm/rig_vertexai.rs, Claude via rawPredict). Admin also controls LLM routing per function via /api/v1/admin/llm/config. So 'admin controls what agents can authenticate to' with 'Vertex AI for Claude Code' as primary use case is implemented.\n(2) Token refresh for OAuth credentials is real: the secret_rotation_check job (every 5 min) runs a genuine RFC 6749 §6 refresh_token grant against the credential document's token_endpoint for *_refresh_token secrets (including claude_max_refresh_token), re-stores the rotated document and extends expires_at.\n(3) Expired user session cleanup is real: a daily loop deletes sessions expired >30 days via the SessionRepository::delete_expired_before port (revoked rows never deleted).\n(4) The background-job framework is real: a registry with ~20 registered jobs, wall-clock and interval schedulers, manual trigger via POST /admin/jobs/{name}/run, liveness probes, and GET /api/v1/admin/jobs surfaced in the TenantSettings admin UI (Jobs tab with status and recent runs).\n(5) The audit log viewer is largely real: GET /api/v1/audit/events applies agent_id/event_type/workspace_id/user_id/repo_id/resource_type/outcome/since/until/limit filters in SQL against the durable audit_events table (SQLite and PostgreSQL adapters), with SSE live streaming, and the WorkspaceSettings and RepoSettings admin UIs render filterable audit tables from it. Both agent and user actions are recorded (agent_id and user_id fields; server-side events via emit_server_audit_event).\n\nGAPS (why the section stays open):\n(a) 'Secrets management: SOPS as default, with Vault support as an option' is NOT implemented. Secrets are encrypted with AES-256-GCM inside the application database using an auto-generated key persisted in the server's own KV store (or GYRE_SECRET_ENCRYPTION_KEY). A repo-wide case-insensitive search for 'sops' and 'vault' across crates/, web/, scripts/ matches only spec/task documents — there is no SOPS integration and no Vault backend anywhere in production code.\n(b) 'Server logs - accessible from the admin UI' is NOT implemented. The only log routes in the API are per-agent logs (/api/v1/agents/:id/logs and /stream); no endpoint serves gyre-server process/tracing logs, and the TenantSettings admin panel has tabs for Users, Invitations, Compute, Workspaces, Budget, Secrets, Audit, Health, Jobs, Policies, LLM, Analytics, BCP — no server-logs surface.\n(c) 'Global repository maintenance' job does not exist: every registration in jobs.rs start_job_registry and every scheduler loop spawned in main.rs was inspected; none runs git gc/pack/prune-style repository maintenance (closest is mirror_sync, which only fetches refs for mirror repos).\n(d) 'OAuth state cleanup' job does not exist: the server stores no OAuth state at all — oidc.rs only serves openid-configuration and JWKS as an OIDC provider for agents, and there is no OAuth authorization flow whose state rows would need cleanup.\n(e) 'Token refresh (OAuth, API keys, any expiring credentials)' is only partial: ROTATION_HANDLERS contains exactly one handler (OAuthRefreshHandler); API keys and other expiring credentials have no refresh path — expiring secrets without a handler only produce SecretExpiring admin notifications.\n(f) 'Full job history stored and visible in admin UI - status, duration, failures, logs' is partial: JobRun carries status, error and started/finished timestamps (duration derivable, failures visible in the Jobs tab), but there is NO logs field per run, and history is process-memory only (HashMap<String, VecDeque<JobRun>> inside Arc<Mutex<RegistryInner>>, capped at 50 runs per job) — a restart orphans all job history and there is no port/adapter persistence for it. 'Job history cleanup (self-managing)' exists only as this in-memory 50-entry deque trim.\n(g) 'Remote agent cleanup (expired/orphaned instances)' is partial: the stale_agent_detector job marks timed-out agents Dead/Paused and tears down worktrees, task-scoped secrets and interrogation policies, but there is no periodic cleanup of orphaned remote compute-target instances/containers themselves — the process/container kill path (ComputeTarget::kill_process) runs only in the admin kill handler, not in any scheduled maintenance job.\n(h) The tenant-level admin audit endpoint (GET /api/v1/admin/audit, used by the TenantSettings Audit tab) reads only the in-memory TelemetryBuffer (per-workspace ring buffer) rather than the durable audit store, so that particular admin view loses events on restart and shows recent activity telemetry, not the full durable trail. Also, no free-text search over audit detail payloads exists (structured filters only).",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/secrets.rs",
        "quote": "//! `POST   /api/v1/secrets`          — create (or update) a secret\n//! `GET    /api/v1/secrets`          — list secret metadata by scope (never values)\n//! `DELETE /api/v1/secrets/:id`      — delete a secret\n//! `POST   /api/v1/secrets/:id/rotate` — force-rotate a secret's value",
        "detail": "The Admin secrets API exists and is the admin-side control surface for agent credentials: create/list/delete/rotate at tenant/workspace/repo/task scope, Admin-only, values never returned. This is the realized part of 'credential management for agents - admin controls what agents can authenticate to'."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "        // Platform Model §7 Secrets Delivery: resolve scoped secrets from the\n        // repository (tenant → workspace → repo → task, nearest scope wins)\n        // and inject as GYRE_CRED_* env vars. The cred-proxy sidecar reads\n        // GYRE_CRED_* vars, stores in memory, and scrubs them before the agent\n        // process starts — raw values are never in the agent env.",
        "detail": "At spawn, resolve_for_agent (real implementations in crates/gyre-adapters/src/sqlite/secret.rs:318 and crates/gyre-adapters/src/postgres/secret.rs:42) collects scoped secrets and they are injected as GYRE_CRED_* env vars for the agent — the delivery half of admin-controlled agent credentials."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "        // M27: cred-proxy addresses for credential routing.\n        container_env.insert(\n            \"GYRE_CRED_PROXY\".to_string(),\n            \"http://127.0.0.1:8765\".to_string(),\n        );\n        container_env.insert(\n            \"ANTHROPIC_BASE_URL\".to_string(),\n            \"http://127.0.0.1:8765\".to_string(),\n        );",
        "detail": "Agent containers are pointed at the local cred-proxy for LLM calls; the Anthropic SDK key is a placeholder ('proxy-managed') that the sidecar replaces per request — admin-managed credentials reach the agent without ever living in its environment."
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "quote": "        // Vertex AI: forward non-secret Vertex config env vars to the container.",
        "detail": "Vertex AI for Claude Code (the spec's primary use case) is wired: CLAUDE_CODE_USE_VERTEX, ANTHROPIC_VERTEX_PROJECT_ID and CLOUD_ML_REGION (GYRE_VERTEX_LOCATION alias) are forwarded to the agent container, while the GCP service-account secret is delivered via the scoped secrets repository. A real Vertex AI adapter (crates/gyre-adapters/src/llm/rig_vertexai.rs, Claude models via anthropic rawPredict/streamRawPredict) backs the server-side LLM port."
      },
      {
        "file": "docker/gyre-agent/cred-proxy.mjs",
        "quote": " * cred-proxy — M27 Credential Opacity",
        "detail": "The credential sidecar is a real production component (576 lines, with a test file): it loads GYRE_CRED_* vars at startup and scrubs them from process.env, injects x-api-key on /v1/* Anthropic calls, enforces a destination allowlist on POST /proxy, reports token usage back to the server for budget enforcement, and runs a GCE metadata emulator on TCP 8080 that exchanges GYRE_CRED_GCP_SA_JSON for OAuth2 access tokens — the concrete mechanism making 'Vertex AI for Claude Code' the primary credential use case."
      },
      {
        "file": "crates/gyre-server/src/secret_rotation.rs",
        "quote": "/// OAuth 2.0 refresh-token grant handler for secrets whose name ends with\n/// `_refresh_token` (e.g. `oauth_refresh_token`, `claude_max_refresh_token`).",
        "detail": "Token refresh for OAuth credentials (the spec's 'Token refresh (OAuth, ...)' bullet and the Claude Max 'OAuth + token refresh' secondary use case) is implemented as a real RFC 6749 §6 refresh_token grant: the handler posts grant_type=refresh_token to the credential document's token_endpoint, stores the rotated document, and extends expires_at; run by the secret_rotation_check job every 5 minutes. However, ROTATION_HANDLERS in this file contains exactly one handler (&OAUTH_REFRESH), so API keys and other expiring credentials have no automatic refresh — they fall back to SecretExpiring admin notifications, making 'any expiring credentials' only partially covered. There is also no in-product OAuth authorization flow to enroll a Claude Max account: the credential document (token_endpoint/client_id/refresh_token) must be supplied out-of-band by an admin via POST /api/v1/secrets."
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/secret.rs",
        "quote": "fn encrypt_value(key: &LessSafeKey, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {",
        "detail": "The actual secrets-management backend is AES-256-GCM (ring LessSafeKey) encryption inside the application database, with the key auto-generated and persisted in the server's own KV namespace (KEY_KV_KEY = \"encryption_key\") or taken from GYRE_SECRET_ENCRYPTION_KEY. This is a real encrypted-at-rest store, but it is NOT the specced backend: 'Secrets management: SOPS as default, with Vault support as an option' requires SOPS as the default mechanism and Vault as an option, and neither exists — a repo-wide case-insensitive search for 'sops'/'vault' across crates/, web/ and scripts/ matches only spec and task documents, never production source. This bullet is an open gap."
      },
      {
        "file": "crates/gyre-server/src/api/audit.rs",
        "quote": "pub struct QueryAuditParams {\n    pub agent_id: Option<String>,\n    pub event_type: Option<String>,\n    pub workspace_id: Option<String>,",
        "detail": "The audit log viewer's query surface is real and filterable: QueryAuditParams (continuing with user_id, repo_id, resource_type, outcome, since, until, limit) is applied as an AuditQueryFilter by state.audit.query, whose SQLite implementation (crates/gyre-adapters/src/sqlite/audit.rs:141, apply_filters on audit_events ordered by timestamp desc) runs real SQL against the durable audit_events table. AuditEventResponse carries both agent_id and user_id, and server-side events flow through emit_server_audit_event, so the trail covers agent and user actions. The WorkspaceSettings and RepoSettings admin UIs render filterable audit tables from GET /audit/events (web/src/components/WorkspaceSettings.svelte loadAudit, web/src/components/RepoSettings.svelte)."
      },
      {
        "file": "crates/gyre-server/src/api/admin.rs",
        "quote": "    let msgs = state.telemetry_buffer.list_all_since(since_ms, lim);",
        "detail": "Weakness inside the audit-viewer requirement: the tenant-level admin audit endpoint (GET /api/v1/admin/audit, which the TenantSettings Audit tab calls via api.adminAudit) reads only state.telemetry_buffer — an in-memory per-workspace ring buffer (crates/gyre-common/src/message.rs TelemetryBuffer, DashMap<Id, VecDeque<Message>> with eviction caps) — and post-filters by agent_id/event_type in Rust. That view loses all events on server restart and is bounded by the ring cap, unlike the durable /audit/events endpoint used by the workspace/repo views."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "    // Schedulers are NOT spawned here — existing background tasks in main.rs handle\n    // periodic execution. Handlers registered above enable on-demand triggering and\n    // status tracking via POST /admin/jobs/{name}/run and GET /admin/jobs.",
        "detail": "Absence evidence for two specced maintenance jobs: every registry.register call in start_job_registry (this file: merge_processor, stale_agent_detector, retention_cleanup, log compression, spawn_budget_reset, mirror_sync, speculative_merge, abandoned_branch_check, cross_workspace_link_staleness_check, orphaned_workspace_check, trust_suggestion_check, message_expiry, task_scheduler, dep_staleness_check, search_code_index, meta_spec_stale_pin_check, secret_rotation_check, meta_spec_conformance_sweep, impersonation_expiry, email_digest) and every scheduler spawned in main.rs (spawn_job for retention_cleanup, search_code_index, meta_spec_stale_pin_check, secret_rotation_check, meta_spec_conformance_sweep, impersonation_expiry, log compression, trust_suggestion_check, email_digest, plus module loops: task scheduler, stale agent/peer detectors, presence eviction, merge processor, SIEM forwarder, procfs monitor, budget reset, rate-limit cleanup, invitation expiry, session cleanup, message expiry) was inspected. There is NO 'Global repository maintenance' job (no git gc/pack/prune maintenance anywhere; mirror_sync only fetches refs for mirror repos) and NO 'OAuth state cleanup' job (the server persists no OAuth state: crates/gyre-server/src/oidc.rs only serves openid-configuration and JWKS as an OIDC provider, and there is no OAuth authorization flow). Both listed background jobs are unimplemented."
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "quote": "/// Spawn a background task that deletes long-expired user sessions",
        "detail": "'Expired user session cleanup' is implemented: a daily loop calls state.sessions.delete_expired_before(cutoff) (30 days past expiry) through the SessionRepository port, deliberately never deleting revoked rows; spawned from main.rs (spawn_session_cleanup)."
      },
      {
        "file": "crates/gyre-server/src/stale_agents.rs",
        "quote": "//! Stale agent detection: marks agents Dead when heartbeat times out.",
        "detail": "'Remote agent cleanup (expired/orphaned instances)' is only partially implemented. The stale_agent_detector job (30 s loop) marks timed-out agents Dead/Stopped per their disconnected_behavior and performs real teardown (jj/git worktree removal, task blocking, task-scoped secret revocation, interrogation-policy cleanup). But nothing periodically cleans up orphaned remote agent *instances* on compute targets: ComputeTarget::kill_process is invoked only from the admin kill handler (crates/gyre-server/src/api/admin.rs kill_agent) and the spawn paths, not from any scheduled maintenance job, and compute-target tunnel/SSH orphans are left to the OS (crates/gyre-server/src/api/compute.rs relies on ssh exiting when the parent dies)."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "pub struct JobRun {\n    pub id: String,\n    pub job_name: String,\n    pub started_at: u64,\n    pub finished_at: Option<u64>,\n    pub status: String,\n    pub error: Option<String>,\n}",
        "detail": "'Full job history stored and visible in admin UI - status, duration, failures, logs' is partial. JobRun carries status, error and started/finished timestamps (duration derivable, failures visible via GET /api/v1/admin/jobs recent_runs and the TenantSettings Jobs tab), but there is NO logs field — per-run job logs are not captured or stored anywhere — so the 'logs' element of the requirement is unimplemented."
      },
      {
        "file": "crates/gyre-server/src/jobs.rs",
        "quote": "    history: HashMap<String, VecDeque<JobRun>>,",
        "detail": "Job history is process-memory only: RegistryInner (held in Arc<Mutex<...>> on JobRegistry) stores run history as a HashMap of VecDeques capped at MAX_HISTORY_PER_JOB = 50 (const at crates/gyre-server/src/jobs.rs:16, trimmed via pop_front in record_run). No port/adapter persistence for job runs exists (no job_runs table, no KV blob — verified by searching crates/gyre-adapters for job history storage), so a restart orphans all job history and 'stored' in the spec's durable sense is not met. 'Job history cleanup (self-managing)' exists only as this in-memory 50-entry trim."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "        // Agent logs\n        .route(\n            \"/api/v1/agents/:id/logs\",\n            post(agent_logs::append_log).get(agent_logs::get_logs),\n        )",
        "detail": "'Server logs - accessible from the admin UI' is NOT implemented. The complete route table in api/mod.rs was inspected: the only log-access routes are per-agent logs (/api/v1/agents/:id/logs and /logs/stream, backed by the agent_logs store). No endpoint serves gyre-server process/tracing logs, and the admin UI (web/src/components/TenantSettings.svelte) contains tabs only for Users, Invitations, Compute Targets, Workspaces, Budget, Secrets, Audit, Health, Jobs, Policies, LLM Defaults, Analytics and BCP — there is no server-logs surface anywhere in web/src (searched for server log components). This bullet is an open gap."
      }
    ]
  }
]

## Shipped

The eight admin-panel deliverables are implemented on this branch and carried
through the base merges with behavior preserved:

1. **Secrets management — SOPS default, Vault option**
   (`GYRE_SECRET_BACKEND`, default `sops`): the SOPS backend
   (`crates/gyre-adapters/src/sops/`) stores secret values as genuine SOPS
   JSON documents with an age-encrypted data key wrapped for the configured
   identity (`GYRE_SOPS_AGE_KEY` / key file, auto-generated identity
   persisted server-side when unset); decryption is server-side on
   resolve/rotate and pre-change AES-256-GCM values are re-encrypted at first
   access. The Vault backend (`crates/gyre-adapters/src/vault/`) serves
   create/list/delete/rotate/resolve from Vault KV v2 with the server's
   token/AppRole for writes and agent-JWT authentication for agent-scoped
   resolution (agents never see Vault credentials); the HTTP client is
   latency-bounded (30 s) and unreachable-Vault operations fail closed.
2. **Durable job history with logs**: `JobRun` carries a `logs` field
   (run-scoped sink handlers write to); runs persist through the
   `gyre-ports` `JobRunRepository` (SQLite + PostgreSQL + mem parity,
   portable migrations) wired via `store!` in `build_state` — the in-memory
   HashMap is a fronting cache only. `GET /api/v1/admin/jobs` serves
   persisted history with status, derived duration, error and logs; the
   TenantSettings Jobs tab renders them. `job_history_cleanup` prunes runs
   past the configured retention.
3. **`repo_maintenance`** (daily): iterates every stored repository and runs
   real git gc/prune/repack through the server's git execution layer,
   recording per-repo outcomes (failures recorded without aborting the rest).
4. **OAuth state + Claude Max enrollment**: Admin-only initiation generates
   an OAuth 2.0 authorization request (state + PKCE verifier, initiating
   admin/tenant derived server-side, expiry) persisted via the
   `OAuthStateRepository` port; the callback validates the state row, does a
   latency-bounded code exchange at the provider token endpoint and stores
   the scoped `claude_max_refresh_token` secret. `oauth_state_cleanup`
   deletes expired state rows.
5. **Complete token refresh**: rotation dispatch matches credential-document
   shape, not name — the RFC 6749 refresh-token grant applies to any OAuth
   credential document; a GCP service-account handler exchanges SA JSON via
   a real RS256-signed JWT-bearer grant; every expiring secret gets a
   per-secret outcome (refreshed / no-refresh-mechanism / failed: reason) in
   the durable run logs, with SecretExpiring escalation when no refresh
   mechanism exists.
6. **`remote_agent_cleanup`**: spawned instances are recorded durably at
   spawn (port + adapters + portable migration, marked terminated on clean
   exit/verified kill); the sweep kills still-recorded-running instances
   whose agent is Dead/Stopped or absent — including restart orphans — via
   `ComputeTarget::kill_process` with pid-identity verification, and records
   what was reaped.
7. **Server logs**: a bounded ring-buffer tracing capture layer in
   `telemetry.rs` feeds the Admin-only `GET /api/v1/admin/server/logs`
   endpoint (level/since/limit filters, ABAC-registered) and the
   TenantSettings Server Logs tab.
8. **Durable searchable admin audit**: `GET /api/v1/admin/audit` queries the
   durable audit repository; `tenant_id` on `AuditEvent` is derived
   server-side at emit (portable migration backfills from workspace scope);
   `tenant_id` and free-text `q` filters are enforced in SQL by both
   adapters (transparently decompressing zstd details, Unicode case
   folding); the TenantSettings Audit tab renders durable events with the
   full filter set including search.

All new routes are in the ABAC resolver (no exemption growth) with
per-handler authorization on exempt routes; migration numbers 000071–000075
are portable across SQLite and PostgreSQL.

### This round: verification-repair (clippy findings 262ddcc8)

`46473775` — clears the two clippy findings that failed the verification
checks at `cb0c1419` (everything else in that run passed): dropped the
redundant `as u64` on `EXIT_MONITOR_POLL_SECS` in
`gyre-server/src/api/spawn.rs` (already `u64`) and factored a
`DestroyEvent` type alias for the `destroys` field in
`gyre-adapters/src/vault/tests.rs` (`clippy::type_complexity`).
Behavior-preserving changes in test code only.

### Test evidence

- This round: `rustfmt --edition 2021 --check` clean on both touched
  files; `cargo clippy -p gyre-adapters --all-targets` and
  `-p gyre-server --all-targets` re-run in this sandbox via the build
  wrapper — zero matches for the two findings (`vault/tests.rs`,
  `unnecessary_cast`/`spawn.rs:7885`). Remaining local clippy notes
  (spawn.rs:2221/2225/3137 and dependency-crate lint noise) are pre-existing
  base lines outside this branch's diff hunks, confirmed by hunk map and by
  the verifier's own clippy run not flagging them. Probes recorded in
  `/tmp/stage/review-evidence/probes.md`. No cold rebuild of unchanged
  crates for this lint-only repair (stage instruction).
- Prior rounds (carried on this branch, see commits list): `cargo check -p
  gyre-server --all-targets` and `-p gyre-adapters --all-targets` clean;
  focused adapter suites green (`sqlite::audit` 24 passed, `vault::` 12
  passed, `postgres::audit` 6 passed); all mechanical invariant scripts
  (arch, migration versions/portability, mem contracts, in-memory stores,
  ABAC registry, unbounded HTTP, and siblings) passing with no new
  exemptions.
- The verifier must execute the gyre-server test selection:
  `maintenance_jobs::`, `api::admin::tests::` (kill/audit/server-logs),
  `api::oauth::tests::`, and the spawn-path durable-recording tests in
  `api::spawn::tests::`. `cargo test -p gyre-server` cannot run inside
  this sandbox's 4 GiB build-guard — an infrastructure restriction, not a
  code defect.
