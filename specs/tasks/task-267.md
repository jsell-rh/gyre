---
title: "Publish reviewed spec backlog for specs/system/identity-security.md"
spec_ref: "GOAL.md — real implementations and meaningful verification"
depends_on: []
progress: complete
commits: []
---

## Required behavior

Publish exactly the independently reviewed missing-work plan below. Only create the assigned task documents and add their exact section bindings. Preserve every existing task, spec, registry entry, coverage status, product file and gate. Task creation is not implementation. Re-read the canonical spec and challenge the audit, decomposition, dependencies and duplicate-work analysis before approving.

{
  "document": "specs/system/identity-security.md",
  "plan_id": "8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e",
  "audit": {
    "base": "fb1424fbac55cd23cb08606f4cd667a96f4dbc43",
    "requirements": "b60edce5bc49f0aba6fe21b3e67397ce3238d0be377671508fb1b1d0d5cbcbca",
    "sections": [
      {
        "id": "8d32b6318a8b8abd570c2883",
        "decision": "context",
        "requirement_quote": "# Identity & Security",
        "detail": "Document umbrella heading with no body text of its own; every requirement in the document lives in the child sections (Agent Identity Stack layers, User Identity, SCIM, Access Control, Impersonation), each audited separately below. Nothing to implement at this level.",
        "evidence": []
      },
      {
        "id": "9d623241914634d94371c864",
        "decision": "gap",
        "requirement_quote": "Three layers, each proving something different. All use existing protocols - no custom auth schemes.",
        "detail": "The three-layer structure exists in production (agent JWTs via AgentSigningKey, Sigstore Fulcio/Rekor client, workload attestation records), and Layers 2-3 use existing protocols (JWT/OIDC, Sigstore). However the section's own constraint 'All use existing protocols - no custom auth schemes' is violated at Layer 1: production replaced the specced SPIFFE/SPIRE protocol with a bespoke 'G10 local workload attestation' scheme (PID/hostname/stack-fingerprint record plus a /proc existence probe), whose module header explicitly calls it a 'pragmatic SPIFFE alternative'. The protocol-specific divergence is fully accounted under the Layer 1 child section; this parent records the constraint violation itself. The per-layer requirements are not re-counted here.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "//! G10: Local workload attestation — pragmatic SPIFFE alternative.",
            "detail": "The entire Layer-1 implementation is a bespoke local scheme, not an existing protocol. Its header continues: 'Provides lightweight workload identity for agent processes without requiring an external SPIFFE/SPIRE infrastructure.' The spec's 'no custom auth schemes' constraint is therefore violated in production by the layer that was supposed to use SPIFFE/SPIRE."
          }
        ]
      },
      {
        "id": "16fb4f5c865aa918b828b94e",
        "decision": "gap",
        "requirement_quote": "- **SPIFFE** for cryptographic workload attestation *(Loom's weaver uses SVID for mutual TLS auth - strong reference)*.",
        "detail": "Required: SPIFFE-based cryptographic workload attestation proving the runtime runs on authorized infrastructure, SPIFFE IDs of the form spiffe://gyre.example.com/agent/{role}/session/{id}, and federation via SPIFFE trust bundle exchange between SPIRE servers. Actual production code has none of this. The sole Layer-1 implementation is a local, self-reported metadata record: OS PID, hostname read from /proc/sys/kernel/hostname, a compute-target label, a self-reported stack fingerprint, and optional container id/image digest, recorded by the server at spawn. 'Liveness verification' is a /proc/{pid} existence probe; 'verify_attestation' string-compares a caller-supplied stack string and skips when either side is empty. Repo-wide grep for 'spiffe://' matches only the two spec files (specs/system/identity-security.md, specs/system/supply-chain.md); grep for 'svid'/'mtls' across crates/ returns zero matches; there is no SPIRE agent/socket, no x509 SVID, no mTLS transport (agents authenticate with JWT bearer over HTTP), and no SPIFFE trust-bundle exchange. Nothing cryptographic binds the workload to authorized infrastructure, so 'the runtime proves it's running on authorized infrastructure' is not performed. The 'agents operate in secure-by-default environments' bullet likewise has no enforcing code.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "//! an external SPIFFE/SPIRE infrastructure.",
            "detail": "Full header: 'Provides lightweight workload identity for agent processes without requiring an external SPIFFE/SPIRE infrastructure.' This is an explicit admission that the specced SPIFFE/SPIRE attestation is not implemented; the replacement collects local, self-reported metadata instead of cryptographic proof."
          },
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "std::path::Path::new(&format!(\"/proc/{pid}\")).exists()",
            "detail": "The only 'workload is real' verification is a local filesystem existence probe of the process PID — not cryptographic attestation, and not proof of authorized infrastructure. Inspected call paths: WorkloadAttestation records are created in api/spawn.rs at spawn and only re-checked via this probe plus a string comparison of the stack fingerprint."
          }
        ]
      },
      {
        "id": "91f35f78880c4d70c32342ee",
        "decision": "gap",
        "requirement_quote": "- **Gyre mints OIDC tokens for agents.** Each agent session gets a short-lived JWT:",
        "detail": "Satisfied in part: Gyre does mint a short-lived EdDSA JWT per agent on POST /api/v1/agents/spawn (mint_worker_scoped), serves /.well-known/openid-configuration and /.well-known/jwks.json from a restart-persistent Ed25519 key, and embeds task_id/spawned_by/attestation_level claims. But several requirements of this section are unmet. (1) The stack hash is NOT embedded in the JWT: although a real stack fingerprint is computed at spawn and stored in the workload_attestations KV record, every production mint passes None for wl_stack_hash, so the spec-registry consumer that compares the verified JWT wl_stack_hash claim against a pinned requirement can never match in production (dead enforcement input). (2) No aud claim exists in AgentJwtClaims and validation sets validate_aud=false, so external verifiers cannot do audience validation. (3) persona is minted as None even though the spawn request carries a validated persona. (4) The specced example scope/task binding exists, but task-scoping enforcement is audited under the Task-Scoped JWT Claims child section. Cross-instance OIDC issuer federation is genuinely implemented (see Federation section).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "// No persona input exists on the spawn request; workers run",
            "detail": "Full comment: 'No persona input exists on the spawn request; workers run persona-less until a persona assignment mechanism exists.' The spawn mint unconditionally passes None for persona — contradicting this section's example JWT which carries \"persona\": \"security\", even though req.persona is validated and recorded in the agent_personas KV namespace at spawn time."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "validation.validate_aud = false;",
            "detail": "AgentJwtClaims (auth.rs ~line 229) has no aud field at all, and self-minted agent JWT validation disables audience checking. GYRE_OIDC_AUDIENCE is only read for inbound upstream (Keycloak) user JWT validation, not for Gyre's own minted agent tokens, so the spec's 'aud: gyre (or GYRE_OIDC_AUDIENCE)' claim is not minted or checked."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "None,\n            None,\n            None,\n            None,\n            None,\n            wl_image_hash,",
            "detail": "mint_worker_scoped — the only production spawn mint — passes None for wl_pid, wl_hostname, wl_compute_target, wl_stack_hash, and wl_container_id (only wl_image_hash is resolved, best-effort via docker image inspect). The orchestrator and gate-agent mints are also all-None. Therefore the stack-hash-as-OIDC-claim requirement has no producer: the claim exists in the struct but is never populated by any production mint."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "Some(required) => event.stack_hash.as_deref() == Some(required.as_str()),",
            "detail": "The spec-registry consumer (named in this section: 'verify agent approvals came from the required stack') does read a stack_hash and compare it to a pinned requirement — but its producer (api/specs.rs, 'Stack hash: from the verified JWT wl_stack_hash claim') reads the JWT claim that no production mint ever sets, so a pinned stack_hash approval can never match a production agent token. The enforcement wiring is real; its input is always absent."
          }
        ]
      },
      {
        "id": "5576c3d85b41c8482ee8b1ee",
        "decision": "gap",
        "requirement_quote": "**Task-scoping is a hard security boundary:** The JWT's `task_id` claim is verified on every API call that mutates task-owned resources. An agent cannot transition a task it was not spawned for, write to a repo it was not assigned, or open MRs outside its task scope — even if it holds a valid (non-expired) JWT.",
        "detail": "Several claims-table requirements are satisfied: jti is minted as a UUID; task_id, spawned_by, attestation_level (derived from compute target type: Container=3, Ssh=2, else 1) are set; iss is the server base URL; expiry is enforced; and revocation is real — on successful POST /api/v1/agents/{id}/complete the agent's token is removed from the port-backed agent_tokens KV store, subsequent requests get 401 'Agent token has been revoked', lookup happens before signature/expiry validation, deletion persists across restarts, and tokens cannot be un-revoked. However: (1) sub is minted as the bare agent UUID, not 'agent:<uuid>' (the prefix is re-added downstream by formatters); (2) there is no aud claim (see Layer 2 section); (3) the default TTL is 300s, not the specced 3600s (deliberate M27.5 deviation); (4) persona and wl_pid/wl_hostname/wl_compute_target/wl_stack_hash/wl_container_id are never set by any production mint (only wl_image_hash is, best-effort). (5) The headline requirement — task_id verified on every mutating call — is NOT enforced: the REST task-status transition handler takes no auth extractor at all (only ABAC middleware governs it, which resolves workspace/repo tier, not task tier); git push explicitly allows any agent principal regardless of the token's repo scope list; and MR creation checks repo scope on the MCP path but never the task_id claim. Containment is at repo/workspace tier, so 'an agent cannot transition a task it was not spawned for ... even with a valid non-expired JWT' does not hold. (6) Revocation deletes the whole token record from KV; the specced jti-keyed revocation table exists as SQL (revoked_tokens in the spawn_log migration, implemented in both SQLite and Postgres adapters) but no production call path ever invokes revoke_token/is_token_revoked — it is dead code (the only server reference is an Arc clone in middleware.rs).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/tasks.rs",
            "quote": "pub async fn transition_task_status(\n    State(state): State<Arc<AppState>>,\n    Path(id): Path<String>,\n    Json(req): Json<TransitionStatusRequest>,",
            "detail": "The REST handler that mutates task-owned state (task status transition) takes no auth extractor — it cannot read the caller's task_id claim. Only the ABAC middleware governs this route, and its RouteResourceMapping resolves the task's workspace/repo scope (tier scoping), never comparing the JWT task_id claim to the mutated task. The spec's hard task-tier boundary is not enforced here."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "Agent tokens and other principals without a user identity are allowed:",
            "detail": "The git push path explicitly admits any agent principal: the comment continues 'agents are scoped to their repos at spawn time and their pushes run through the gate chain'. The token's repo:{name}:write scope list is not enforced on push, so a valid non-expired worker JWT can push to repos outside its spawn scope absent a specific ABAC Deny policy (check_repo_abac is permissive when no policies are configured)."
          },
          {
            "file": "crates/gyre-server/src/agent_audit.rs",
            "quote": "state.kv_store.kv_remove(\"agent_tokens\", &agent_id).await;",
            "detail": "Actual revocation on agent completion is a KV delete of the whole token record (namespace agent_tokens, port-backed SQLite/Postgres kv_store table). The jti is only extracted for the AgentTokenRevoked audit event — it is never written to a revocation table as the spec requires. The 401-before-expiry ordering and permanence are nevertheless real (middleware looks up agent_tokens before running signature/expiry validation)."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/spawn_log.rs",
            "quote": "async fn revoke_token(&self, token_hash: &str, agent_id: &str, revoked_at: u64) -> Result<()> {",
            "detail": "A real SQL revocation table (revoked_tokens, keyed by token hash) is implemented for both SQLite and Postgres and wired into AppState.spawn_log — but grep across crates/gyre-server/src shows no production call path ever invokes revoke_token or is_token_revoked (the only reference is 'spawn_log: base.spawn_log.clone(),' in middleware.rs). The specced revocation-table mechanism is dead code; the live mechanism is the KV delete above."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "M27.5: Default reduced from 3600 to 300 s (5 min) to limit JWT exposure window.",
            "detail": "The claims table specifies 'configurable via GYRE_AGENT_JWT_TTL, default 3600 s'; production defaults to 300 s (unwrap_or(300) on GYRE_AGENT_JWT_TTL). A deliberate deviation from the specced default, undocumented in the spec itself."
          }
        ]
      },
      {
        "id": "68b06ebc09a7c8d49aa516d0",
        "decision": "gap",
        "requirement_quote": "- Every commit is cryptographically signed - the signature proves which agent, on which task, spawned by which user, made the commit.",
        "detail": "A genuine Sigstore stack exists: sigstore.rs implements a real Fulcio v2 REST client (ephemeral ECDSA P-256 keypair, proof-of-possession over the JWT sub claim, HTTP POST to /api/v2/signingCert) and real Rekor hashedrekord submission/verification (HTTP POST to /api/v1/log/entries, content-match verification of digest+signature+cert), with upstream wire-conformance tests. However the spec's requirements are not met in production: (1) 'Every commit is cryptographically signed' is false — the only production signing call site is the jj squash endpoint (POST /api/v1/repos/:id/jj/squash); jj new, git HTTP pushes, and merge commits produce no CommitSignature records, and GYRE_SIGNING_MODE=none skips signing entirely. (2) Keyless signing is opt-in only: the default SigningMode is Local (long-lived persistent forge Ed25519 key), not Fulcio. (3) On Fulcio failure the squash path silently falls back to local forge-key signing, and non-JWT callers present an empty bearer token so Fulcio always fails for them. (4) The signature does not prove task/spawned-by: the cryptographic signature covers only the commit SHA and the Fulcio cert binds only the OIDC sub; task_id and spawned_by are detached plaintext DB fields on the CommitSignature row. (5) Rekor non-repudiation is split: fulcio-mode commit signatures are really submitted to Rekor, but merge-attestation bundles — the other signed artifact — are wired to a NoopRekorClient that only warns and returns Ok(None), an explicit TODO stub.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/commit_signatures.rs",
            "quote": "/// Local Ed25519 signing with the forge key (default).",
            "detail": "SigningMode::from_env defaults to Local for any unrecognized GYRE_SIGNING_MODE value ('_ => Self::Local'). Default deployments therefore sign with the long-lived, persisted, operator-rotatable forge key rather than the specced keyless Fulcio path; keyless is opt-in ('fulcio' => Self::Fulcio)."
          },
          {
            "file": "crates/gyre-server/src/api/jj.rs",
            "quote": "jj squash: Fulcio keyless signing failed; falling back to local signing",
            "detail": "The single production signing surface (jj squash) silently degrades from keyless Fulcio signing to the local forge key on any Fulcio error, and presents auth.bearer_token.clone().unwrap_or_default() (an empty JWT for dev-token/API-key callers), so the 'agent with an OIDC identity gets a short-lived signing certificate from Fulcio' path fails closed to non-keyless signing in practice. grep confirms api/jj.rs is the only production caller of sign_commit_keyless/sign_commit_local."
          },
          {
            "file": "crates/gyre-adapters/src/rekor.rs",
            "quote": "TODO (task-144 plan step 4): real Rekor API integration",
            "detail": "The only RekorClient wired in production is NoopRekorClient ('let rekor: Arc<dyn RekorClient> = Arc::new(gyre_adapters::NoopRekorClient);' in lib.rs), whose submit warns 'Rekor transparency log not configured — merge attestation bundle not submitted for non-repudiation' and returns Ok(None). Merge attestation bundles therefore have no transparency-log non-repudiation at all; only opt-in fulcio-mode commit signatures reach a real Rekor."
          }
        ]
      },
      {
        "id": "e9904e2db8f2d58f4288b47c",
        "decision": "context",
        "requirement_quote": "| SPIFFE | Workload is real, running on authorized infra | mTLS / x509 SVIDs | Yes (SPIFFE Federation) |",
        "detail": "Summary table restating the three layers; it contains no requirements beyond those audited in the Layer 1/2/3 child sections (each row is accounted there — the SPIFFE row's mTLS/x509-SVID/federation claims are not realized, see Layer 1; the OIDC row is largely realized with claim-set gaps, see Layer 2; the Sigstore row is partially realized, see Layer 3). No independent implementable requirement at this heading.",
        "evidence": []
      },
      {
        "id": "05e06b05e91abddea8707d41",
        "decision": "gap",
        "requirement_quote": "- SPIFFE federation handles the workload attestation layer independently.",
        "detail": "Three of four bullets are genuinely satisfied: two Gyre instances federate by trusting each other's OIDC issuers — GYRE_TRUSTED_ISSUERS config, validate_federated_jwt performs remote OIDC discovery (GET {issuer}/.well-known/openid-configuration -> jwks_uri -> JWK set) with an SSRF same-origin guard on the jwks_uri and a 5-minute JWKS cache, wired into both the auth extractor and the global middleware; an agent from instance A authenticates to B by presenting its JWT; and no static credentials are exchanged (only issuer URLs configured, keys fetched via discovery). The remaining bullet — 'SPIFFE federation handles the workload attestation layer independently' — has no implementation: there is no SPIFFE federation machinery anywhere (grep for SPIFFE trust bundles matches only the Sigstore Fulcio trust bundle), and federated agents are authenticated purely by JWT, with the wl_* claims in a federated token being whatever the remote instance minted and no cross-instance workload-attestation verification.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "async fn validate_federated_jwt(token: &str, state: &Arc<AppState>) -> Option<AuthenticatedAgent> {",
            "detail": "Real cross-instance agent-JWT federation: validates EdDSA JWTs whose iss matches a configured trusted issuer, fetching the remote instance's JWKS via OIDC discovery (satisfying 'B verifies against A's OIDC discovery endpoint' and 'no static credentials exchanged')."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "// Check federation JWT from trusted remote Gyre instances (G11).",
            "detail": "Federation validation is wired into the production auth middleware path, confirming the OIDC-issuer federation bullets are live. In contrast, no analogous workload-attestation federation exists: nothing verifies a federated agent's workload attestation, and the only trust-bundle code in the repo is the Sigstore Fulcio bundle in sigstore.rs."
          },
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "//! G10: Local workload attestation — pragmatic SPIFFE alternative.",
            "detail": "The workload-attestation layer that this bullet says 'SPIFFE federation handles ... independently' is a local per-instance record with no federation capability of any kind — there is nothing to federate and no federation consumer."
          }
        ]
      },
      {
        "id": "e8678db1d842de41103e68ea",
        "decision": "context",
        "requirement_quote": "- This identity primitive is **reusable as a CI building block** - if agents already have attested identity, CI runners are just another agent workload.",
        "detail": "Architectural-principle section, not an implementable requirement: 'CI runners are just another agent workload' is a conditional vision statement. Inspection confirms no CI-runner workload type exists (compute targets are Local, SSH, Container, Kubernetes only; grep for ci_runner/CiRunner across crates/ finds no production code), and the TrustAnchor 'Addon' variant (external systems such as GitHub Actions OIDC) is a CRUD metadata store only, with no verification path consuming it. The second bullet — all agent actions attributable to a verified identity across all three layers — is an aggregate property of the three layers and fails only insofar as Layer 1 provides no verified (cryptographic) workload identity; that failure is accounted under the Layer 1 child section and is not re-counted here.",
        "evidence": []
      },
      {
        "id": "68c52df88c82f5590f5c6997",
        "decision": "satisfied",
        "requirement_quote": "- **SSO compatible** - integrate with enterprise identity providers.\n- **Keycloak** is the primary target. Architecture must support adding any provider (Okta, Entra ID, etc.).",
        "detail": "This section's unique (non-SCIM) requirements are met. SSO integration is provider-generic: any OIDC issuer configured via GYRE_OIDC_ISSUER (+ optional GYRE_OIDC_AUDIENCE), validated with standard OIDC discovery + JWKS refresh and RS256 issuer/audience validation; Keycloak-shaped realm_access.roles claim mapping is tolerated optionally (empty default), so Okta/Entra ID work as plain OIDC issuers. Full user management exists: profiles carry username (immutable after creation) and display_name; PUT /api/v1/users/me accepts only display_name/timezone/locale/avatar_url/preferences — 'username, external_id, email, and global_role are not editable here ... email is derived from SSO', satisfying 'Email derived from SSO - not user-managed' (email is written only from verified SSO claims or SCIM push). The SCIM-provisioning and group/role-mapping bullets of this section are expanded into the SCIM child sections below, where they are audited (and are gaps); they are not re-counted here.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/main.rs",
            "quote": "let jwt_config = std::env::var(\"GYRE_OIDC_ISSUER\").ok().map(|issuer| {",
            "detail": "Upstream SSO is configured by issuer URL alone — any standards-compliant OIDC provider (Keycloak, Okta, Entra ID) can be plugged in; nothing is hardcoded to a Keycloak endpoint, satisfying the provider-extensibility requirement."
          },
          {
            "file": "crates/gyre-server/src/api/users.rs",
            "quote": "email is derived from SSO",
            "detail": "Full doc: 'username, external_id, email, and global_role are not editable here — username is immutable after creation and email is derived from SSO (§Username vs Display Name).' The self-service profile update request type has no email or username field, satisfying 'profiles with display name, username. Email derived from SSO - not user-managed.'"
          }
        ]
      },
      {
        "id": "e88b0643650de3d8f271012a",
        "decision": "gap",
        "requirement_quote": "SCIM 2.0 (RFC 7643 / RFC 7644) is the standard protocol for enterprise IdP-driven user lifecycle management. Keycloak, Okta, Entra ID, and most enterprise IdPs support SCIM as a provisioning protocol — they push user creates, updates, and deactivations to Gyre automatically.",
        "detail": "SCIM 2.0 user lifecycle support is only partially implemented, so the section's core promise — IdPs push creates, updates, and deactivations automatically — is not fully delivered. What exists: /scim/v2 base path with Users CRUD (list/get/create/update/delete) plus ServiceProviderConfig, Schemas, ResourceTypes discovery endpoints, GYRE_SCIM_TOKEN bearer auth checked per-handler (fail-closed when unset), externalId mapped to external_id with idempotent create, and immutable userName. What is missing (detailed in the child sections): all six /scim/v2/Groups endpoints (grep for /scim/v2/Groups and ScimGroup across crates/ returns zero matches — no group type exists anywhere), PATCH /scim/v2/Users/{id} (explicitly disclaimed 'supported: false'), the deactivation lifecycle (active is parsed but never read; DELETE is a hard delete with no session revocation or spawned_by cleanup), the enterprise schema extension, the group-to-workspace mapping endpoint and behavior, and a production wiring defect: the global require_auth_middleware layered over api_router() does not accept the SCIM token, so a request bearing only GYRE_SCIM_TOKEN is rejected 401 before the handler's own check runs (the SCIM tests bypass this by mounting api_router() directly). Endpoint, schema, and mapping specifics are accounted once under their child sections.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "// ── SCIM 2.0 Provisioning ─────────────────────────────────────────\n        .route(\n            \"/scim/v2/Users\",\n            get(scim::scim_list_users).post(scim::scim_create_user),\n        )",
            "detail": "The complete production SCIM route set: Users CRUD, ServiceProviderConfig, Schemas, ResourceTypes — 8 of the 15 specced endpoints. There are no Group routes and no PATCH anywhere in the router, so the IdP-driven group lifecycle (the section's 'Group-to-workspace-role mapping' motivation) has no receiving surface."
          }
        ]
      },
      {
        "id": "2cf8a1c0fbde5c0d82e85317",
        "decision": "gap",
        "requirement_quote": "**Authentication:** SCIM endpoints accept a dedicated SCIM bearer token (configured via `GYRE_SCIM_TOKEN` env var). This is separate from the agent JWT — SCIM clients are server-to-server, not agent-to-server.",
        "detail": "Endpoint inventory: 8 of the 15 specced endpoints are implemented (GET/POST /scim/v2/Users; GET/PUT/DELETE /scim/v2/Users/{id}; GET ServiceProviderConfig, Schemas, ResourceTypes). Missing: PATCH /scim/v2/Users/{id} (ServiceProviderConfig explicitly declares patch unsupported), and all six /scim/v2/Groups endpoints (grep for '/scim/v2/Groups' and 'ScimGroup' across crates/ returns zero matches — no group handler, type, or storage exists). Authentication is implemented per-handler and is fail-closed and separate from agent JWTs (constant-time compare against GYRE_SCIM_TOKEN; unset token => 403 'GYRE_SCIM_TOKEN not configured'; routes are ABAC-exempt). BUT there is a production wiring defect that defeats it: build_router layers require_auth_middleware over the whole api_router() — including /scim/v2/* — and that middleware only exempts /api/v1/version, accepting only the global auth token, impersonation JWTs, agent tokens, API keys, upstream OIDC JWTs, or federation JWTs. A request bearing only the SCIM bearer token is rejected 401 'Invalid token' before check_scim_auth ever runs; GYRE_SCIM_TOKEN is not referenced anywhere in lib.rs. The SCIM integration tests never catch this because they build api_router().with_state(...) directly, bypassing the middleware stack. The heading's own '(stub — to be implemented)' label does not close the gap; the goal requires real implementation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "fn check_scim_auth(headers: &HeaderMap) -> Result<(), ApiError> {",
            "detail": "Per-handler SCIM auth exists and is fail-closed ('GYRE_SCIM_TOKEN not configured' => Forbidden when the env var is unset; constant-time Bearer compare otherwise), separate from the agent JWT chain — the handler-level half of the requirement is implemented."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "// /api/v1/version is intentionally public.",
            "detail": "require_auth_middleware's only path exemption is /api/v1/version. Its token ladder (global token, impersonation JWT, agent_tokens KV, API keys, upstream OIDC, federation JWT) has no GYRE_SCIM_TOKEN branch — grep for 'scim' in lib.rs returns zero matches."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let api = api::api_router()",
            "detail": "The production router layers abac_middleware, last_seen_middleware, and require_auth_middleware over all of api_router() — which contains the /scim/v2 routes. A production SCIM client presenting only the dedicated SCIM bearer token is therefore rejected 401 by the global gate before the handler-level check_scim_auth runs; the specced authentication model does not work end-to-end in the shipped server."
          },
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "\"patch\": { \"supported\": false }",
            "detail": "ServiceProviderConfig explicitly disclaims PATCH support, confirming PATCH /scim/v2/Users/{id} (specced for partial update e.g. deactivate via active:false) is not implemented."
          }
        ]
      },
      {
        "id": "b2b37ae05e75a44a9117ceca",
        "decision": "gap",
        "requirement_quote": "Gyre implements a subset of the SCIM core schema (RFC 7643 §4.1) plus enterprise extension (RFC 7643 §4.3):",
        "detail": "The core-schema subset is faithfully implemented: schemas, id, externalId (mapped to external_id, the Keycloak sub), userName (immutable after creation — the PUT handler never writes username or external_id), displayName, emails with value/primary/type, active field on the wire, and meta with resourceType/created/lastModified/location ('/scim/v2/Users/{uuid}'); no password fields are exposed. But the enterprise extension is entirely absent: the schemas array contains only the core User schema — grep for 'enterprise:2.0' across crates/ returns zero matches — so organization (tenant-id) and manager.value do not exist in the serialized resource. Worse, the specced Gyre-specific deactivation behavior is not implemented: the response hardcodes active: true ('deactivation tracked via future `active` field on User'), the request's parsed active field is never read by any handler, and SCIM DELETE is a hard store delete rather than the specced deactivation — no session revocation (sessions.revoke_all_for_user is never invoked from the SCIM path) and no removal from active agents' spawned_by tracking (grep for spawned_by in scim.rs: zero matches).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "schemas: vec![\"urn:ietf:params:scim:schemas:core:2.0:User\".to_string()]",
            "detail": "Only the core schema is advertised/serialized; the specced 'urn:ietf:params:scim:schemas:extension:enterprise:2.0:User' extension (organization -> tenant-id, manager.value) appears nowhere in the codebase, so the 'plus enterprise extension' half of the schema requirement is absent."
          },
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "active: true, // deactivation tracked via future `active` field on User",
            "detail": "active is hardcoded true in the serialized resource and the request-side active: Option<bool> is never read by any handler, so the specced 'active: false → Gyre deactivates user, revokes all active sessions, removes from active agents' spawned_by tracking' lifecycle cannot be driven via SCIM at all."
          },
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "// SCIM DELETE = deactivate, not hard delete. We delete from store here;",
            "detail": "Full comment admits the deviation: 'SCIM DELETE = deactivate, not hard delete. We delete from store here; production deployments with SSO should soft-delete via active=false.' The handler performs a hard users.delete plus workspace-ownership cleanup only — no session revocation, no spawned_by cleanup (the machinery for both exists elsewhere, e.g. state.sessions.revoke_all_for_user on the admin deactivate path, but is never invoked from SCIM)."
          }
        ]
      },
      {
        "id": "6273edccbcca379f21b54f97",
        "decision": "gap",
        "requirement_quote": "PUT /api/v1/workspaces/{id}/scim-mappings",
        "detail": "This feature is entirely absent. grep for 'scim-mappings', 'scim_mappings', and 'ScimMapping' across all of crates/ returns zero matches: no route registration, no request type, no handler, no mapping store. Consequently no IdP group push can create or revoke WorkspaceMembership records — there are also no SCIM Group endpoints to push to (see SCIM Endpoints section), so both halves of the behavior ('when a SCIM group push adds a user ... Gyre automatically creates a WorkspaceMembership record with Developer role' and 'when the user is removed from the group, the membership is revoked') have no implementation. The only IdP-to-role translation that exists is JWT-claim based: roles_from_claims maps Keycloak realm_access.roles to global UserRole values at login — global roles, not per-workspace membership driven by admin-configured group mappings.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/workspaces/:id/teams/:team_id/members/:user_id\",",
            "detail": "Absence evidence: the workspace route surface in api/mod.rs contains team-membership routes and every other workspace sub-resource, but no '/scim-mappings' route anywhere (verified by reading the full router and by zero grep hits for scim-mappings/ScimMapping across the repository). The admin-configurable per-workspace IdP-group-to-role mapping store and its automatic WorkspaceMembership effects do not exist."
          }
        ]
      },
      {
        "id": "85cdf1bee61d8201c0b9e692",
        "decision": "satisfied",
        "requirement_quote": "- **ABAC (Attribute-Based Access Control)** - not just roles, attribute-driven policies.",
        "detail": "A real attribute-driven policy engine exists, not a role-only check: policy_engine.rs evaluates conditions (Equals/NotEquals/In/NotIn/GreaterThan/LessThan/Contains/Exists) over an AttributeContext of subject/resource/action attributes (subject.tenant_id, subject.global_role list, agent OIDC claims, resource.workspace_id/repo_id/owner/visibility, membership-sourced workspace_role, subject.stack_hash normalized from the wl_stack_hash claim, etc.), with immutable-deny-first, priority-ordered first-match, and default-deny. Policies are persisted entities (PolicyRepository) plus seeded builtins. Fine-grained filtering on the named resource classes is real: (a) source control — Gyre is the source host; git read requires workspace membership with the per-repo per-user override capping the workspace role (restrict-only), and push additionally runs ABAC policy evaluation with decisions recorded to the audit trail; (b) admin panel sections — every /api/v1/admin/* route maps to the 'admin' resource type with a builtin admin-only deny policy, at per-section granularity; (c) agent capabilities — /api/v1/agents/spawn and sibling routes map to the 'agent' resource with distinct actions, scoped reviewer/validator tokens get route allow-lists, and MCP tools have a per-tool needs_write RBAC gate plus per-call token-scope validation. Extensibility to any resource is structural: every route maps through the RouteResourceMapping registry with a first-match resolve(), mechanically enforced by scripts/check-abac-route-registry.sh in CI. One nuance inspected and accepted: check_repo_abac is permissive when no policies are configured and bypasses for non-JWT principals, but the independent membership/override check in authorize_repo_access gates every user on the git path, so the combination is not fail-open for humans.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/policy_engine.rs",
            "quote": "fn eval_condition(cond: &Condition, ctx: &AttributeContext) -> bool {",
            "detail": "The engine evaluates structured conditions over arbitrary named attributes (with set semantics for list values), not just role equality — the 'attribute-driven, not just roles' requirement. evaluate() applies immutable-deny-first, priority-sorted first-match, and default-deny when no policy matches."
          },
          {
            "file": "crates/gyre-server/src/abac_middleware.rs",
            "quote": "pub fn resolve<'a>(&'a self, matched_pattern: &str) -> Option<&'a RouteResourceMapping> {",
            "detail": "The RouteResourceMapping registry maps every registered route pattern to a resource type and action, making the fine-grained model extensible to any resource; CI (check-abac-route-registry.sh) enforces that every route in api/mod.rs has a mapping, so new resources cannot silently escape policy evaluation."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "effective_repo_role(membership.role, override_role)",
            "detail": "Fine-grained filtering on the source-control surface: per-repo per-user overrides cap the workspace role (restrict-only) on top of workspace membership, so access to a specific repository is filtered per user, per repo — plus ABAC policy evaluation on pushes."
          },
          {
            "file": "crates/gyre-server/src/abac_middleware.rs",
            "quote": "RouteResourceMapping::api(\"/api/v1/agents/spawn\", \"agent\", Some(\"spawn\"))",
            "detail": "Agent capabilities are an ABAC resource: spawn (and complete/heartbeat/status) map to the 'agent' resource type with distinct actions, and admin /api/v1/admin/* routes map to the 'admin' resource gated by a builtin admin-only-deny policy — covering the two other named resource classes."
          },
          {
            "file": "crates/gyre-server/src/mcp.rs",
            "quote": "let needs_write = matches!(",
            "detail": "MCP tool calls — the programmatic agent-capability surface — are gated per tool: write-effecting tools require at least Agent role (PERMISSION_DENIED otherwise, with an rbac_denied audit event), and gate membership is mechanically derived from handler effects per check-mcp-write-tools.sh; per-call validate_tool_scope additionally confines scoped agent tokens to their encoded tenant/workspace/repo."
          }
        ]
      },
      {
        "id": "51d58ea7cdae955f56185003",
        "decision": "satisfied",
        "requirement_quote": "- Target user must **provide a code or explicit approval** before impersonation begins.",
        "detail": "All four requirements are met with production enforcement. (1) Explicit approval enforced: an Admin-only caller creates an impersonation request; the server generates a CSPRNG 8-digit one-time code, stores only its SHA-256 hash, and delivers the raw code only to the target via a persisted notification; the target (and only the target — 'only the impersonation target may approve this request') must approve by presenting the code, compared in constant time ('invalid approval code' => 403 on mismatch). The impersonation JWT is minted only inside the approve path, so an admin cannot start without the target's code. Constraints: no self-impersonation, no impersonating another Admin, one active session per admin, TTL (default 15 min) with background expiry sweep, and ending the session revokes the token before exp. (2) Notification: the target receives both an ImpersonationRequest notification (with the code) and an ImpersonationAlert ('Admin ... is impersonating your account') when impersonation starts — persisted via create_notification and fanned out through dispatch_to_channels (email/webhook per preferences). (3) Full audit trail: ImpersonationStarted/ImpersonationEnded lifecycle events plus a per-call 'impersonation_action' audit event for every API call under the token, each recording impersonator_id, target_user_id, method, path, surface — wired from the ABAC middleware, auth extractors, git HTTP, MCP, TTY, and WS surfaces, persisted through the port-backed AuditRepository (SQLite/Postgres). (4) AP2 pattern: intent = persisted request; signed mandate = an EdDSA JWT (impersonator_id + target sub) signed by the server's persistent Ed25519 key, validated with zero leeway against the active-session store; receipt = the audit events. The mandate is signed by the server's key rather than the target's personal key — the target's consent is the delivered one-time code (a knowledge factor only they receive), which matches the spec's 'inspired by AP2' and 'code or explicit approval' wording.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/impersonation.rs",
            "quote": "only the impersonation target may approve this request",
            "detail": "Only the target user may approve the request (Forbidden otherwise), so approval cannot be granted by the requesting admin or a third party — the consent requirement is enforced at the identity level, not just the knowledge level."
          },
          {
            "file": "crates/gyre-server/src/impersonation.rs",
            "quote": "invalid approval code",
            "detail": "The approval code (CSPRNG-generated, only its SHA-256 stored, raw code delivered solely to the target) is compared in constant time; a mismatch returns Forbidden before any impersonation token is minted. The token is minted only inside approve_request, so impersonation cannot begin without the target's code."
          },
          {
            "file": "crates/gyre-server/src/impersonation.rs",
            "quote": "NotificationType::ImpersonationRequest",
            "detail": "The approval-code delivery notification, persisted via state.create_notification and dispatched through dispatch_to_channels — the target is notified (and receives the consent code) through the production notification pipeline."
          },
          {
            "file": "crates/gyre-server/src/impersonation.rs",
            "quote": "NotificationType::ImpersonationAlert",
            "detail": "A second notification is emitted when impersonation actually starts ('Admin ... is impersonating your account'), satisfying 'Impersonated user is notified when someone impersonates them.'"
          },
          {
            "file": "crates/gyre-server/src/impersonation.rs",
            "quote": "fn record_impersonated_action",
            "detail": "Every API call made under an impersonation token records an audit event carrying impersonator_id, target_user_id, method, path, and surface — the full audit trail clearly attributed to the impersonator acting as the target — wired from the ABAC middleware, auth extractors, git HTTP, MCP, TTY, and WS paths, and persisted through the port-backed AuditRepository."
          }
        ]
      }
    ],
    "gaps": [
      {
        "id": "9d623241914634d94371c864",
        "decision": "gap",
        "requirement_quote": "Three layers, each proving something different. All use existing protocols - no custom auth schemes.",
        "detail": "The three-layer structure exists in production (agent JWTs via AgentSigningKey, Sigstore Fulcio/Rekor client, workload attestation records), and Layers 2-3 use existing protocols (JWT/OIDC, Sigstore). However the section's own constraint 'All use existing protocols - no custom auth schemes' is violated at Layer 1: production replaced the specced SPIFFE/SPIRE protocol with a bespoke 'G10 local workload attestation' scheme (PID/hostname/stack-fingerprint record plus a /proc existence probe), whose module header explicitly calls it a 'pragmatic SPIFFE alternative'. The protocol-specific divergence is fully accounted under the Layer 1 child section; this parent records the constraint violation itself. The per-layer requirements are not re-counted here.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "//! G10: Local workload attestation — pragmatic SPIFFE alternative.",
            "detail": "The entire Layer-1 implementation is a bespoke local scheme, not an existing protocol. Its header continues: 'Provides lightweight workload identity for agent processes without requiring an external SPIFFE/SPIRE infrastructure.' The spec's 'no custom auth schemes' constraint is therefore violated in production by the layer that was supposed to use SPIFFE/SPIRE."
          }
        ]
      },
      {
        "id": "16fb4f5c865aa918b828b94e",
        "decision": "gap",
        "requirement_quote": "- **SPIFFE** for cryptographic workload attestation *(Loom's weaver uses SVID for mutual TLS auth - strong reference)*.",
        "detail": "Required: SPIFFE-based cryptographic workload attestation proving the runtime runs on authorized infrastructure, SPIFFE IDs of the form spiffe://gyre.example.com/agent/{role}/session/{id}, and federation via SPIFFE trust bundle exchange between SPIRE servers. Actual production code has none of this. The sole Layer-1 implementation is a local, self-reported metadata record: OS PID, hostname read from /proc/sys/kernel/hostname, a compute-target label, a self-reported stack fingerprint, and optional container id/image digest, recorded by the server at spawn. 'Liveness verification' is a /proc/{pid} existence probe; 'verify_attestation' string-compares a caller-supplied stack string and skips when either side is empty. Repo-wide grep for 'spiffe://' matches only the two spec files (specs/system/identity-security.md, specs/system/supply-chain.md); grep for 'svid'/'mtls' across crates/ returns zero matches; there is no SPIRE agent/socket, no x509 SVID, no mTLS transport (agents authenticate with JWT bearer over HTTP), and no SPIFFE trust-bundle exchange. Nothing cryptographic binds the workload to authorized infrastructure, so 'the runtime proves it's running on authorized infrastructure' is not performed. The 'agents operate in secure-by-default environments' bullet likewise has no enforcing code.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "//! an external SPIFFE/SPIRE infrastructure.",
            "detail": "Full header: 'Provides lightweight workload identity for agent processes without requiring an external SPIFFE/SPIRE infrastructure.' This is an explicit admission that the specced SPIFFE/SPIRE attestation is not implemented; the replacement collects local, self-reported metadata instead of cryptographic proof."
          },
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "std::path::Path::new(&format!(\"/proc/{pid}\")).exists()",
            "detail": "The only 'workload is real' verification is a local filesystem existence probe of the process PID — not cryptographic attestation, and not proof of authorized infrastructure. Inspected call paths: WorkloadAttestation records are created in api/spawn.rs at spawn and only re-checked via this probe plus a string comparison of the stack fingerprint."
          }
        ]
      },
      {
        "id": "91f35f78880c4d70c32342ee",
        "decision": "gap",
        "requirement_quote": "- **Gyre mints OIDC tokens for agents.** Each agent session gets a short-lived JWT:",
        "detail": "Satisfied in part: Gyre does mint a short-lived EdDSA JWT per agent on POST /api/v1/agents/spawn (mint_worker_scoped), serves /.well-known/openid-configuration and /.well-known/jwks.json from a restart-persistent Ed25519 key, and embeds task_id/spawned_by/attestation_level claims. But several requirements of this section are unmet. (1) The stack hash is NOT embedded in the JWT: although a real stack fingerprint is computed at spawn and stored in the workload_attestations KV record, every production mint passes None for wl_stack_hash, so the spec-registry consumer that compares the verified JWT wl_stack_hash claim against a pinned requirement can never match in production (dead enforcement input). (2) No aud claim exists in AgentJwtClaims and validation sets validate_aud=false, so external verifiers cannot do audience validation. (3) persona is minted as None even though the spawn request carries a validated persona. (4) The specced example scope/task binding exists, but task-scoping enforcement is audited under the Task-Scoped JWT Claims child section. Cross-instance OIDC issuer federation is genuinely implemented (see Federation section).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/spawn.rs",
            "quote": "// No persona input exists on the spawn request; workers run",
            "detail": "Full comment: 'No persona input exists on the spawn request; workers run persona-less until a persona assignment mechanism exists.' The spawn mint unconditionally passes None for persona — contradicting this section's example JWT which carries \"persona\": \"security\", even though req.persona is validated and recorded in the agent_personas KV namespace at spawn time."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "validation.validate_aud = false;",
            "detail": "AgentJwtClaims (auth.rs ~line 229) has no aud field at all, and self-minted agent JWT validation disables audience checking. GYRE_OIDC_AUDIENCE is only read for inbound upstream (Keycloak) user JWT validation, not for Gyre's own minted agent tokens, so the spec's 'aud: gyre (or GYRE_OIDC_AUDIENCE)' claim is not minted or checked."
          },
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "None,\n            None,\n            None,\n            None,\n            None,\n            wl_image_hash,",
            "detail": "mint_worker_scoped — the only production spawn mint — passes None for wl_pid, wl_hostname, wl_compute_target, wl_stack_hash, and wl_container_id (only wl_image_hash is resolved, best-effort via docker image inspect). The orchestrator and gate-agent mints are also all-None. Therefore the stack-hash-as-OIDC-claim requirement has no producer: the claim exists in the struct but is never populated by any production mint."
          },
          {
            "file": "crates/gyre-server/src/spec_registry.rs",
            "quote": "Some(required) => event.stack_hash.as_deref() == Some(required.as_str()),",
            "detail": "The spec-registry consumer (named in this section: 'verify agent approvals came from the required stack') does read a stack_hash and compare it to a pinned requirement — but its producer (api/specs.rs, 'Stack hash: from the verified JWT wl_stack_hash claim') reads the JWT claim that no production mint ever sets, so a pinned stack_hash approval can never match a production agent token. The enforcement wiring is real; its input is always absent."
          }
        ]
      },
      {
        "id": "5576c3d85b41c8482ee8b1ee",
        "decision": "gap",
        "requirement_quote": "**Task-scoping is a hard security boundary:** The JWT's `task_id` claim is verified on every API call that mutates task-owned resources. An agent cannot transition a task it was not spawned for, write to a repo it was not assigned, or open MRs outside its task scope — even if it holds a valid (non-expired) JWT.",
        "detail": "Several claims-table requirements are satisfied: jti is minted as a UUID; task_id, spawned_by, attestation_level (derived from compute target type: Container=3, Ssh=2, else 1) are set; iss is the server base URL; expiry is enforced; and revocation is real — on successful POST /api/v1/agents/{id}/complete the agent's token is removed from the port-backed agent_tokens KV store, subsequent requests get 401 'Agent token has been revoked', lookup happens before signature/expiry validation, deletion persists across restarts, and tokens cannot be un-revoked. However: (1) sub is minted as the bare agent UUID, not 'agent:<uuid>' (the prefix is re-added downstream by formatters); (2) there is no aud claim (see Layer 2 section); (3) the default TTL is 300s, not the specced 3600s (deliberate M27.5 deviation); (4) persona and wl_pid/wl_hostname/wl_compute_target/wl_stack_hash/wl_container_id are never set by any production mint (only wl_image_hash is, best-effort). (5) The headline requirement — task_id verified on every mutating call — is NOT enforced: the REST task-status transition handler takes no auth extractor at all (only ABAC middleware governs it, which resolves workspace/repo tier, not task tier); git push explicitly allows any agent principal regardless of the token's repo scope list; and MR creation checks repo scope on the MCP path but never the task_id claim. Containment is at repo/workspace tier, so 'an agent cannot transition a task it was not spawned for ... even with a valid non-expired JWT' does not hold. (6) Revocation deletes the whole token record from KV; the specced jti-keyed revocation table exists as SQL (revoked_tokens in the spawn_log migration, implemented in both SQLite and Postgres adapters) but no production call path ever invokes revoke_token/is_token_revoked — it is dead code (the only server reference is an Arc clone in middleware.rs).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/tasks.rs",
            "quote": "pub async fn transition_task_status(\n    State(state): State<Arc<AppState>>,\n    Path(id): Path<String>,\n    Json(req): Json<TransitionStatusRequest>,",
            "detail": "The REST handler that mutates task-owned state (task status transition) takes no auth extractor — it cannot read the caller's task_id claim. Only the ABAC middleware governs this route, and its RouteResourceMapping resolves the task's workspace/repo scope (tier scoping), never comparing the JWT task_id claim to the mutated task. The spec's hard task-tier boundary is not enforced here."
          },
          {
            "file": "crates/gyre-server/src/git_http.rs",
            "quote": "Agent tokens and other principals without a user identity are allowed:",
            "detail": "The git push path explicitly admits any agent principal: the comment continues 'agents are scoped to their repos at spawn time and their pushes run through the gate chain'. The token's repo:{name}:write scope list is not enforced on push, so a valid non-expired worker JWT can push to repos outside its spawn scope absent a specific ABAC Deny policy (check_repo_abac is permissive when no policies are configured)."
          },
          {
            "file": "crates/gyre-server/src/agent_audit.rs",
            "quote": "state.kv_store.kv_remove(\"agent_tokens\", &agent_id).await;",
            "detail": "Actual revocation on agent completion is a KV delete of the whole token record (namespace agent_tokens, port-backed SQLite/Postgres kv_store table). The jti is only extracted for the AgentTokenRevoked audit event — it is never written to a revocation table as the spec requires. The 401-before-expiry ordering and permanence are nevertheless real (middleware looks up agent_tokens before running signature/expiry validation)."
          },
          {
            "file": "crates/gyre-adapters/src/sqlite/spawn_log.rs",
            "quote": "async fn revoke_token(&self, token_hash: &str, agent_id: &str, revoked_at: u64) -> Result<()> {",
            "detail": "A real SQL revocation table (revoked_tokens, keyed by token hash) is implemented for both SQLite and Postgres and wired into AppState.spawn_log — but grep across crates/gyre-server/src shows no production call path ever invokes revoke_token or is_token_revoked (the only reference is 'spawn_log: base.spawn_log.clone(),' in middleware.rs). The specced revocation-table mechanism is dead code; the live mechanism is the KV delete above."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "M27.5: Default reduced from 3600 to 300 s (5 min) to limit JWT exposure window.",
            "detail": "The claims table specifies 'configurable via GYRE_AGENT_JWT_TTL, default 3600 s'; production defaults to 300 s (unwrap_or(300) on GYRE_AGENT_JWT_TTL). A deliberate deviation from the specced default, undocumented in the spec itself."
          }
        ]
      },
      {
        "id": "68b06ebc09a7c8d49aa516d0",
        "decision": "gap",
        "requirement_quote": "- Every commit is cryptographically signed - the signature proves which agent, on which task, spawned by which user, made the commit.",
        "detail": "A genuine Sigstore stack exists: sigstore.rs implements a real Fulcio v2 REST client (ephemeral ECDSA P-256 keypair, proof-of-possession over the JWT sub claim, HTTP POST to /api/v2/signingCert) and real Rekor hashedrekord submission/verification (HTTP POST to /api/v1/log/entries, content-match verification of digest+signature+cert), with upstream wire-conformance tests. However the spec's requirements are not met in production: (1) 'Every commit is cryptographically signed' is false — the only production signing call site is the jj squash endpoint (POST /api/v1/repos/:id/jj/squash); jj new, git HTTP pushes, and merge commits produce no CommitSignature records, and GYRE_SIGNING_MODE=none skips signing entirely. (2) Keyless signing is opt-in only: the default SigningMode is Local (long-lived persistent forge Ed25519 key), not Fulcio. (3) On Fulcio failure the squash path silently falls back to local forge-key signing, and non-JWT callers present an empty bearer token so Fulcio always fails for them. (4) The signature does not prove task/spawned-by: the cryptographic signature covers only the commit SHA and the Fulcio cert binds only the OIDC sub; task_id and spawned_by are detached plaintext DB fields on the CommitSignature row. (5) Rekor non-repudiation is split: fulcio-mode commit signatures are really submitted to Rekor, but merge-attestation bundles — the other signed artifact — are wired to a NoopRekorClient that only warns and returns Ok(None), an explicit TODO stub.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/commit_signatures.rs",
            "quote": "/// Local Ed25519 signing with the forge key (default).",
            "detail": "SigningMode::from_env defaults to Local for any unrecognized GYRE_SIGNING_MODE value ('_ => Self::Local'). Default deployments therefore sign with the long-lived, persisted, operator-rotatable forge key rather than the specced keyless Fulcio path; keyless is opt-in ('fulcio' => Self::Fulcio)."
          },
          {
            "file": "crates/gyre-server/src/api/jj.rs",
            "quote": "jj squash: Fulcio keyless signing failed; falling back to local signing",
            "detail": "The single production signing surface (jj squash) silently degrades from keyless Fulcio signing to the local forge key on any Fulcio error, and presents auth.bearer_token.clone().unwrap_or_default() (an empty JWT for dev-token/API-key callers), so the 'agent with an OIDC identity gets a short-lived signing certificate from Fulcio' path fails closed to non-keyless signing in practice. grep confirms api/jj.rs is the only production caller of sign_commit_keyless/sign_commit_local."
          },
          {
            "file": "crates/gyre-adapters/src/rekor.rs",
            "quote": "TODO (task-144 plan step 4): real Rekor API integration",
            "detail": "The only RekorClient wired in production is NoopRekorClient ('let rekor: Arc<dyn RekorClient> = Arc::new(gyre_adapters::NoopRekorClient);' in lib.rs), whose submit warns 'Rekor transparency log not configured — merge attestation bundle not submitted for non-repudiation' and returns Ok(None). Merge attestation bundles therefore have no transparency-log non-repudiation at all; only opt-in fulcio-mode commit signatures reach a real Rekor."
          }
        ]
      },
      {
        "id": "05e06b05e91abddea8707d41",
        "decision": "gap",
        "requirement_quote": "- SPIFFE federation handles the workload attestation layer independently.",
        "detail": "Three of four bullets are genuinely satisfied: two Gyre instances federate by trusting each other's OIDC issuers — GYRE_TRUSTED_ISSUERS config, validate_federated_jwt performs remote OIDC discovery (GET {issuer}/.well-known/openid-configuration -> jwks_uri -> JWK set) with an SSRF same-origin guard on the jwks_uri and a 5-minute JWKS cache, wired into both the auth extractor and the global middleware; an agent from instance A authenticates to B by presenting its JWT; and no static credentials are exchanged (only issuer URLs configured, keys fetched via discovery). The remaining bullet — 'SPIFFE federation handles the workload attestation layer independently' — has no implementation: there is no SPIFFE federation machinery anywhere (grep for SPIFFE trust bundles matches only the Sigstore Fulcio trust bundle), and federated agents are authenticated purely by JWT, with the wl_* claims in a federated token being whatever the remote instance minted and no cross-instance workload-attestation verification.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/auth.rs",
            "quote": "async fn validate_federated_jwt(token: &str, state: &Arc<AppState>) -> Option<AuthenticatedAgent> {",
            "detail": "Real cross-instance agent-JWT federation: validates EdDSA JWTs whose iss matches a configured trusted issuer, fetching the remote instance's JWKS via OIDC discovery (satisfying 'B verifies against A's OIDC discovery endpoint' and 'no static credentials exchanged')."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "// Check federation JWT from trusted remote Gyre instances (G11).",
            "detail": "Federation validation is wired into the production auth middleware path, confirming the OIDC-issuer federation bullets are live. In contrast, no analogous workload-attestation federation exists: nothing verifies a federated agent's workload attestation, and the only trust-bundle code in the repo is the Sigstore Fulcio bundle in sigstore.rs."
          },
          {
            "file": "crates/gyre-server/src/workload_attestation.rs",
            "quote": "//! G10: Local workload attestation — pragmatic SPIFFE alternative.",
            "detail": "The workload-attestation layer that this bullet says 'SPIFFE federation handles ... independently' is a local per-instance record with no federation capability of any kind — there is nothing to federate and no federation consumer."
          }
        ]
      },
      {
        "id": "e88b0643650de3d8f271012a",
        "decision": "gap",
        "requirement_quote": "SCIM 2.0 (RFC 7643 / RFC 7644) is the standard protocol for enterprise IdP-driven user lifecycle management. Keycloak, Okta, Entra ID, and most enterprise IdPs support SCIM as a provisioning protocol — they push user creates, updates, and deactivations to Gyre automatically.",
        "detail": "SCIM 2.0 user lifecycle support is only partially implemented, so the section's core promise — IdPs push creates, updates, and deactivations automatically — is not fully delivered. What exists: /scim/v2 base path with Users CRUD (list/get/create/update/delete) plus ServiceProviderConfig, Schemas, ResourceTypes discovery endpoints, GYRE_SCIM_TOKEN bearer auth checked per-handler (fail-closed when unset), externalId mapped to external_id with idempotent create, and immutable userName. What is missing (detailed in the child sections): all six /scim/v2/Groups endpoints (grep for /scim/v2/Groups and ScimGroup across crates/ returns zero matches — no group type exists anywhere), PATCH /scim/v2/Users/{id} (explicitly disclaimed 'supported: false'), the deactivation lifecycle (active is parsed but never read; DELETE is a hard delete with no session revocation or spawned_by cleanup), the enterprise schema extension, the group-to-workspace mapping endpoint and behavior, and a production wiring defect: the global require_auth_middleware layered over api_router() does not accept the SCIM token, so a request bearing only GYRE_SCIM_TOKEN is rejected 401 before the handler's own check runs (the SCIM tests bypass this by mounting api_router() directly). Endpoint, schema, and mapping specifics are accounted once under their child sections.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "// ── SCIM 2.0 Provisioning ─────────────────────────────────────────\n        .route(\n            \"/scim/v2/Users\",\n            get(scim::scim_list_users).post(scim::scim_create_user),\n        )",
            "detail": "The complete production SCIM route set: Users CRUD, ServiceProviderConfig, Schemas, ResourceTypes — 8 of the 15 specced endpoints. There are no Group routes and no PATCH anywhere in the router, so the IdP-driven group lifecycle (the section's 'Group-to-workspace-role mapping' motivation) has no receiving surface."
          }
        ]
      },
      {
        "id": "2cf8a1c0fbde5c0d82e85317",
        "decision": "gap",
        "requirement_quote": "**Authentication:** SCIM endpoints accept a dedicated SCIM bearer token (configured via `GYRE_SCIM_TOKEN` env var). This is separate from the agent JWT — SCIM clients are server-to-server, not agent-to-server.",
        "detail": "Endpoint inventory: 8 of the 15 specced endpoints are implemented (GET/POST /scim/v2/Users; GET/PUT/DELETE /scim/v2/Users/{id}; GET ServiceProviderConfig, Schemas, ResourceTypes). Missing: PATCH /scim/v2/Users/{id} (ServiceProviderConfig explicitly declares patch unsupported), and all six /scim/v2/Groups endpoints (grep for '/scim/v2/Groups' and 'ScimGroup' across crates/ returns zero matches — no group handler, type, or storage exists). Authentication is implemented per-handler and is fail-closed and separate from agent JWTs (constant-time compare against GYRE_SCIM_TOKEN; unset token => 403 'GYRE_SCIM_TOKEN not configured'; routes are ABAC-exempt). BUT there is a production wiring defect that defeats it: build_router layers require_auth_middleware over the whole api_router() — including /scim/v2/* — and that middleware only exempts /api/v1/version, accepting only the global auth token, impersonation JWTs, agent tokens, API keys, upstream OIDC JWTs, or federation JWTs. A request bearing only the SCIM bearer token is rejected 401 'Invalid token' before check_scim_auth ever runs; GYRE_SCIM_TOKEN is not referenced anywhere in lib.rs. The SCIM integration tests never catch this because they build api_router().with_state(...) directly, bypassing the middleware stack. The heading's own '(stub — to be implemented)' label does not close the gap; the goal requires real implementation.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "fn check_scim_auth(headers: &HeaderMap) -> Result<(), ApiError> {",
            "detail": "Per-handler SCIM auth exists and is fail-closed ('GYRE_SCIM_TOKEN not configured' => Forbidden when the env var is unset; constant-time Bearer compare otherwise), separate from the agent JWT chain — the handler-level half of the requirement is implemented."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "// /api/v1/version is intentionally public.",
            "detail": "require_auth_middleware's only path exemption is /api/v1/version. Its token ladder (global token, impersonation JWT, agent_tokens KV, API keys, upstream OIDC, federation JWT) has no GYRE_SCIM_TOKEN branch — grep for 'scim' in lib.rs returns zero matches."
          },
          {
            "file": "crates/gyre-server/src/lib.rs",
            "quote": "let api = api::api_router()",
            "detail": "The production router layers abac_middleware, last_seen_middleware, and require_auth_middleware over all of api_router() — which contains the /scim/v2 routes. A production SCIM client presenting only the dedicated SCIM bearer token is therefore rejected 401 by the global gate before the handler-level check_scim_auth runs; the specced authentication model does not work end-to-end in the shipped server."
          },
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "\"patch\": { \"supported\": false }",
            "detail": "ServiceProviderConfig explicitly disclaims PATCH support, confirming PATCH /scim/v2/Users/{id} (specced for partial update e.g. deactivate via active:false) is not implemented."
          }
        ]
      },
      {
        "id": "b2b37ae05e75a44a9117ceca",
        "decision": "gap",
        "requirement_quote": "Gyre implements a subset of the SCIM core schema (RFC 7643 §4.1) plus enterprise extension (RFC 7643 §4.3):",
        "detail": "The core-schema subset is faithfully implemented: schemas, id, externalId (mapped to external_id, the Keycloak sub), userName (immutable after creation — the PUT handler never writes username or external_id), displayName, emails with value/primary/type, active field on the wire, and meta with resourceType/created/lastModified/location ('/scim/v2/Users/{uuid}'); no password fields are exposed. But the enterprise extension is entirely absent: the schemas array contains only the core User schema — grep for 'enterprise:2.0' across crates/ returns zero matches — so organization (tenant-id) and manager.value do not exist in the serialized resource. Worse, the specced Gyre-specific deactivation behavior is not implemented: the response hardcodes active: true ('deactivation tracked via future `active` field on User'), the request's parsed active field is never read by any handler, and SCIM DELETE is a hard store delete rather than the specced deactivation — no session revocation (sessions.revoke_all_for_user is never invoked from the SCIM path) and no removal from active agents' spawned_by tracking (grep for spawned_by in scim.rs: zero matches).",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "schemas: vec![\"urn:ietf:params:scim:schemas:core:2.0:User\".to_string()]",
            "detail": "Only the core schema is advertised/serialized; the specced 'urn:ietf:params:scim:schemas:extension:enterprise:2.0:User' extension (organization -> tenant-id, manager.value) appears nowhere in the codebase, so the 'plus enterprise extension' half of the schema requirement is absent."
          },
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "active: true, // deactivation tracked via future `active` field on User",
            "detail": "active is hardcoded true in the serialized resource and the request-side active: Option<bool> is never read by any handler, so the specced 'active: false → Gyre deactivates user, revokes all active sessions, removes from active agents' spawned_by tracking' lifecycle cannot be driven via SCIM at all."
          },
          {
            "file": "crates/gyre-server/src/api/scim.rs",
            "quote": "// SCIM DELETE = deactivate, not hard delete. We delete from store here;",
            "detail": "Full comment admits the deviation: 'SCIM DELETE = deactivate, not hard delete. We delete from store here; production deployments with SSO should soft-delete via active=false.' The handler performs a hard users.delete plus workspace-ownership cleanup only — no session revocation, no spawned_by cleanup (the machinery for both exists elsewhere, e.g. state.sessions.revoke_all_for_user on the admin deactivate path, but is never invoked from SCIM)."
          }
        ]
      },
      {
        "id": "6273edccbcca379f21b54f97",
        "decision": "gap",
        "requirement_quote": "PUT /api/v1/workspaces/{id}/scim-mappings",
        "detail": "This feature is entirely absent. grep for 'scim-mappings', 'scim_mappings', and 'ScimMapping' across all of crates/ returns zero matches: no route registration, no request type, no handler, no mapping store. Consequently no IdP group push can create or revoke WorkspaceMembership records — there are also no SCIM Group endpoints to push to (see SCIM Endpoints section), so both halves of the behavior ('when a SCIM group push adds a user ... Gyre automatically creates a WorkspaceMembership record with Developer role' and 'when the user is removed from the group, the membership is revoked') have no implementation. The only IdP-to-role translation that exists is JWT-claim based: roles_from_claims maps Keycloak realm_access.roles to global UserRole values at login — global roles, not per-workspace membership driven by admin-configured group mappings.",
        "evidence": [
          {
            "file": "crates/gyre-server/src/api/mod.rs",
            "quote": "\"/api/v1/workspaces/:id/teams/:team_id/members/:user_id\",",
            "detail": "Absence evidence: the workspace route surface in api/mod.rs contains team-membership routes and every other workspace sub-resource, but no '/scim-mappings' route anywhere (verified by reading the full router and by zero grep hits for scim-mappings/ScimMapping across the repository). The admin-configurable per-workspace IdP-group-to-role mapping store and its automatic WorkspaceMembership effects do not exist."
          }
        ]
      }
    ],
    "source_evidence": [
      {
        "file": "crates/gyre-server/src/workload_attestation.rs",
        "sha256": "e04fa9700fd2b81c46653f3ffb7dc3a95521ee5a731f500be75ea43dde14711e"
      },
      {
        "file": "crates/gyre-server/src/api/spawn.rs",
        "sha256": "03f98632204e0c91331d34dfb53d5cbe4a71a7045fa9191ef9efeda3d434afe0"
      },
      {
        "file": "crates/gyre-server/src/auth.rs",
        "sha256": "5877b30260e2fa22eb80278049d17d64ce65a7f9a34ec2fc037a811c774cfa69"
      },
      {
        "file": "crates/gyre-server/src/spec_registry.rs",
        "sha256": "187a311bccfff72faea7b86d5225210383ab7031f1c87762fdb1d677c60b5cec"
      },
      {
        "file": "crates/gyre-server/src/api/tasks.rs",
        "sha256": "ca8afd5d13c41a71fd98c8ee3944784213f2e1cc1ce834c2c9114dc2629360b8"
      },
      {
        "file": "crates/gyre-server/src/git_http.rs",
        "sha256": "35dee813aece670bf34875fa6d322ee56de210a9199688e0089f6f696eb4c8ee"
      },
      {
        "file": "crates/gyre-server/src/agent_audit.rs",
        "sha256": "4639c89b56245db6eefa17cc1441afd288e384556c2d9fb2e80577c5eecc3eee"
      },
      {
        "file": "crates/gyre-adapters/src/sqlite/spawn_log.rs",
        "sha256": "48f9bfdb6d2bbd670f67591f2e7ea76a2dd5a8120e5b24a53678e0f4242ec6e8"
      },
      {
        "file": "crates/gyre-server/src/lib.rs",
        "sha256": "1271b02debc19b265b4a0fc4b1a4516e926dd39b15aeef882c39f9611bbe4b3c"
      },
      {
        "file": "crates/gyre-server/src/commit_signatures.rs",
        "sha256": "d3f7a344d7d8a1e3f4016b4c96b83f12e04581537328ceaeda3f7a5f239de8cd"
      },
      {
        "file": "crates/gyre-server/src/api/jj.rs",
        "sha256": "b33891fd0503b996562f72b506941aa93b56d1baa0892121a740a0b2ea5d585c"
      },
      {
        "file": "crates/gyre-adapters/src/rekor.rs",
        "sha256": "b6d3992d2b093019ef631274b942b6db5f812c024fbccfa52802298ed01d8a9f"
      },
      {
        "file": "crates/gyre-server/src/main.rs",
        "sha256": "f4f6948bf1e818ab4437380f391e04e969f1530fb70d5672fe60638dc7a0d82a"
      },
      {
        "file": "crates/gyre-server/src/api/users.rs",
        "sha256": "d465563ff768f46933a1da67b1d20608b4e1074bd5dfa1f57ebebb3c15cf6aa5"
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "sha256": "7cd819be7922b9c62b5e48b6e84cf5c3b75c8015b76f16501439df4afc6219e1"
      },
      {
        "file": "crates/gyre-server/src/api/scim.rs",
        "sha256": "43b9fc1623eae1b31aeaa19038c097298b7b6686728728a3781c867cadb91c71"
      },
      {
        "file": "crates/gyre-server/src/policy_engine.rs",
        "sha256": "ee08d185644c268086bd198cc2a932372e6fe27ddf917659662543b1f2cc96a7"
      },
      {
        "file": "crates/gyre-server/src/abac_middleware.rs",
        "sha256": "4eb50b0ed795f152b2568ed33a980879382fe903831eba64a8a143e916f76635"
      },
      {
        "file": "crates/gyre-server/src/mcp.rs",
        "sha256": "f08bb495c68513b7420d9ceec2a246365f7db8dddf6f0dece6fae17116381f09"
      },
      {
        "file": "crates/gyre-server/src/impersonation.rs",
        "sha256": "2735730541373f1238cc8ea7e237b3e17413878820383bdf7544abc672023708"
      }
    ]
  },
  "plan": {
    "audit": "a7815e9efad1942ce2ba22b548b7b5a577afedbafd62ef483b69d8f38cde6b54",
    "tasks": [
      {
        "key": "spiffe-workload-attestation",
        "title": "SPIFFE workload attestation: X.509-SVIDs replace the G10 local scheme",
        "covers": [
          "9d623241914634d94371c864",
          "16fb4f5c865aa918b828b94e"
        ],
        "required_behavior": "Implement Layer 1 of the identity stack with the specced existing protocol instead of the bespoke G10 local scheme (workload_attestation.rs self-reports PID/hostname/stack fingerprint and 'verifies' via a /proc existence probe plus a string compare that skips when either side is empty — a custom auth scheme the spec's 'All use existing protocols - no custom auth schemes' constraint forbids). Required: (1) X.509-SVIDs per the SPIFFE X.509-SVID specification: each agent session receives an SVID whose leaf certificate carries its SPIFFE ID as a URI SAN in the exact spec format spiffe://{trust-domain}/agent/{role}/session/{id} (trust domain configurable, e.g. GYRE_SPIFFE_TRUST_DOMAIN). When an external SPIRE server / Workload API is configured, SVIDs are obtained from it; otherwise Gyre issues SPIFFE-conformant SVIDs from a trust-domain CA it operates. (2) Cryptographic verification: the workload-identity verification path validates a presented SVID's chain against the trust bundle (SPIFFE bundle format), enforces certificate validity periods, and parses/binds the SPIFFE ID to the agent session — replacing the /proc/{pid} probe and the empty-string-skipping stack compare as the 'workload is real' proof. SVID issuance is gated on the spawn-time authorization evidence (authorized compute target / stack fingerprint), so the runtime proves it runs on authorized infrastructure. (3) Secure-by-default: an agent that cannot present a valid SVID fails workload-identity verification — no bypass path for unattested agents on the verification surface. (4) The server publishes its trust bundle at the standard SPIFFE well-known location so any verifier can check SVIDs. (5) The existing wl_* JWT claim contract is preserved: claims continue to be embedded and now reflect the SVID-backed attestation (stack fingerprint bound to the attested workload, not a free-form self-report). The JWT remains the Layer-2 permission token; the SVID is the Layer-1 workload proof, presented and verified on the attestation path (spawn/attest/heartbeat/verify), not merely recorded as metadata.",
        "acceptance": [
          "A spawned agent session holds an X.509-SVID whose URI SAN is exactly spiffe://{trust-domain}/agent/{role}/session/{session-id} for that session, and the server's verification path validates the chain against the published trust bundle and extracts the SPIFFE ID.",
          "Workload-identity verification rejects, fail-closed: an SVID signed by a key not in the trust bundle, an expired/not-yet-valid SVID, an SVID whose SPIFFE ID names a different agent session or a foreign trust domain, and a request presenting no SVID.",
          "The trust bundle is served in SPIFFE bundle format at the well-known endpoint and is the sole verification anchor (verification never trusts self-reported metadata).",
          "The wl_* JWT claims remain embedded for spawned agents and the stack fingerprint they carry is bound to the SVID-backed attestation record.",
          "The G10 /proc-probe and skip-on-empty string comparison are no longer the workload-identity verification on this path (grep-level: no verification path succeeds on an unattested or tampered workload)."
        ],
        "negative_probe": "Take a spawned agent's SVID and substitute one signed by an unknown key but carrying a well-formed SPIFFE ID for the same session (or tamper with the stack fingerprint between attestation and verification, or present an empty stack string): the workload-identity verification must reject. Against current code this probe passes verification today (verify_attestation skips when either stack string is empty and nothing is cryptographic), so the probe fails on the pre-task code and only passes once real chain validation exists — asserting a 'spiffe' string or struct existing is not sufficient.",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "spiffe-federation",
        "title": "SPIFFE federation: trust-bundle exchange between Gyre instances",
        "covers": [
          "05e06b05e91abddea8707d41"
        ],
        "required_behavior": "Close the one unimplemented bullet of the Federation section: 'SPIFFE federation handles the workload attestation layer independently.' The existing OIDC-issuer federation (GYRE_TRUSTED_ISSUERS, validate_federated_jwt with remote JWKS discovery, SSRF same-origin guard, 5-min cache) stays as-is. Add SPIFFE trust-bundle exchange: a Gyre instance B can register/fetch instance A's SPIFFE trust bundle (per the SPIFFE federation spec — bundle endpoints keyed by trust domain, refresh on interval), and B's workload-identity verification accepts SVIDs from A's trust domain using A's exchanged bundle while still rejecting SVIDs from trust domains whose bundles were never exchanged. Workload-attestation federation is independent of the OIDC layer: a federated agent's Layer-2 JWT is validated via OIDC discovery, and its Layer-1 SVID is validated via the SPIFFE bundle — neither substitutes for the other, and no static credentials (shared keys/secrets) are exchanged between instances, only issuer URLs and public trust bundles.",
        "acceptance": [
          "Instance B configured with A as a federation peer obtains A's SPIFFE trust bundle (fetch or registration) and verifies an A-trust-domain SVID presented by a federated agent, extracting the same SPIFFE ID A's verifiers would.",
          "An SVID from a trust domain whose bundle was never exchanged with B is rejected by B's workload-identity verification, even when the caller's OIDC JWT from that instance's issuer is otherwise trusted.",
          "Bundle refresh/rotation works: after A rotates its bundle, B re-fetches within the configured interval and verification follows the refreshed bundle (old-bundle-only verification of a rotated SVID fails closed rather than silently succeeding).",
          "No static credential material is exchanged: federation configuration consists of issuer/trust-domain endpoints, with all key material fetched via the public bundle/JWKS discovery endpoints."
        ],
        "negative_probe": "Authenticate a federated agent from instance A at instance B in two configurations: (a) B trusts A's OIDC issuer but has NOT exchanged A's SPIFFE bundle — the agent authenticates (Layer 2) but its workload attestation must NOT verify or be reported as attested; (b) B exchanges the bundle — the same SVID verifies. A probe that passes in both configurations (or fails in both) proves the workload layer is not independently federated; today nothing even distinguishes the two cases, so the probe cannot pass pre-task.",
        "dependencies": [
          "spiffe-workload-attestation"
        ],
        "existing_task": null
      },
      {
        "key": "agent-jwt-claims-task-scope",
        "title": "Agent JWT claims completion and hard task-scope boundary (by-effect enforcement sweep)",
        "covers": [
          "91f35f78880c4d70c32342ee",
          "5576c3d85b41c8482ee8b1ee"
        ],
        "required_behavior": "Complete the Layer-2 token contract and enforce the hard security boundary, per the Task-Scoped JWT Claims reference table and Layer 2. Mint side — every agent JWT from POST /api/v1/agents/spawn (and the orchestrator/gate mints where the fact is known) carries: sub as 'agent:<uuid>' (not the bare UUID), iss = server base URL, aud = 'gyre' (or GYRE_OIDC_AUDIENCE if set), exp/iat with default TTL 3600 s via GYRE_AGENT_JWT_TTL (the current 300 s default is an undocumented-in-spec M27.5 deviation; restore the spec default), jti (already minted), task_id, spawned_by, persona populated from the spawn request's validated persona/persona_slug (today unconditionally None even though the request carries and validates a persona through resolve_persona_in_chain), stack_hash AND its wl_stack_hash alias populated with the real SHA-256 stack fingerprint from the workload attestation record (today every production mint passes None, leaving the spec-registry stack-pinning consumer with a dead enforcement input — spec_registry.rs compares event.stack_hash against a pinned requirement that can never match), attestation_level (already derived from compute-target type), and wl_pid/wl_hostname/wl_compute_target/wl_container_id/wl_image_hash populated from the real spawn-time facts whenever those facts exist (only wl_image_hash is resolved today). Validation side — enable audience validation for self-minted agent tokens (validate_aud with the configured audience; AgentJwtClaims currently has no aud field and sets validate_aud=false), keeping federated-issuer validation behavior correct. Enforcement side — task-scoping as a hard boundary, closed by a BY-EFFECT enumeration: sweep every REST and MCP handler whose execution mutates task rows, task-scoped secrets, or creates task-linked MRs — never by route-family enumeration or illustrative subset — and give each surface found either an enforced agent-principal task-scope check or an explicit, justified out-of-scope statement recorded in the task text. The sweep necessarily includes, beyond any further surfaces discovered during implementation, the following (all verified at this base): (1) REST task mutation: PUT /api/v1/tasks/:id (update_task, crates/gyre-server/src/api/tasks.rs:277 — mutates title, description, priority, assigned_to with generation bump, branch, pr_link, labels) and PUT /api/v1/tasks/:id/status (transition_task_status, tasks.rs:323); both currently take NO auth extractor at all (State/Path/Json only, governed only by ABAC middleware tier scoping); both must take an auth extractor and reject agent principals whose task_id claim does not match the target task with 403. (2) REST agent lifecycle that mutates task-owned state: POST /api/v1/agents/:id/complete (complete_agent, crates/gyre-server/src/api/spawn.rs:2632) takes NO auth extractor, yet create_mr_for_completion transitions the target agent's task to Review (spawn.rs:2581-2591) and opens the task's completion MR; the route maps to RouteResourceMapping ('agent', 'complete') and the seeded builtin 'agent-scoped-access' (priority 700) ALLOWS the 'complete' action for Agent-role subjects on resource type '*', so an agent JWT spawned for task A can complete task B's agent — transitioning task B and opening its MR — under DEFAULT configuration (the MCP twin already enforces self-only completion for agent JWTs, mcp.rs:5006-5010; the REST handler has no equivalent check). The handler must take an auth extractor and, for agent principals, enforce the same boundary the MCP twin enforces: an agent JWT may only complete its own agent / a task covered by its task_id claim. (3) POST /api/v1/agents/:id/stop (spawn.rs:2928) and POST /api/v1/agents/:id/fail (spawn.rs:3067): both take NO auth extractor and sit in the frozen ABAC route-registry exemption file (scripts/abac-route-registry-exemptions.txt), so NO middleware ABAC and NO per-handler authorization governs them at all; both terminate the target agent and revoke the target task's Task-scoped secrets via secret_rotation::revoke_agent_task_secrets. They must take an auth extractor (per-handler authorization is mandated for ABAC-exempt routes by the standing invariants) and, for agent principals, be restricted to the caller's own agent — an agent may terminate itself, never another task's workload or secrets. (4) POST /api/v1/agents/spawn (spawn_agent_core reassigns task.assigned_to, bumps the generation, and auto-advances Backlog→InProgress, spawn.rs:1206-1215): currently contained only by the absence of 'spawn' from the Agent-role builtin allow — policy default, not code. For agent principals, spawning onto a target task must be authorized by the token's own claims — the target task matching the caller's task_id claim, or the token carrying an explicit spawn scope covering the task's repo/workspace (the same claim-level right the MCP gyre_spawn_worker path already validates via validate_tool_scope) — 403 otherwise, so the task-claim boundary holds regardless of policy configuration. (5) MCP task-mutation surface, wherever an agent principal can be the caller, covers BOTH tools dispatched behind the tier-only check_task_target arm (mcp.rs, gyre_update_task/gyre_decompose_spec scope validation resolves the task's repo/workspace tier and never compares the task_id claim): gyre_update_task (handle_update_task — must reach the handler's full mutation set title/description/priority/assigned_to/branch/pr_link/status, not a subset) and gyre_decompose_spec (mutates the parent task's subtask linkage via parent_task_id); each must compare the caller's task_id claim against the target (or parent) task and return PERMISSION_DENIED on mismatch. (6) Repo/MR boundary per the same spec paragraph: (b) write to a repo it was not assigned — git push authorization (git_http.rs authorize_repo_access currently returns Ok for every principal with user_id.is_none()) must require agent principals to hold the target repo in their token's scope list; (c) open MRs outside its task scope — MR creation (MCP gyre_create_mr and the REST path) must check the task_id claim, not only repo scope. Human/system/orchestrator principals keep their existing rights; the new checks target agent task-scoping. Revocation side — wire the existing dead jti-keyed revocation table into production: on successful POST /api/v1/agents/{id}/complete write the token's jti (token hash) via SpawnLogRepository::revoke_token (implemented in SQLite and Postgres adapters, currently never called from any production path), check is_token_revoked during agent-token validation before the expiry check, return 401 for revoked jti, and make revocation persist across restarts (the current KV map-removal may remain for lookup but the jti table is the durable record); tokens cannot be un-revoked.",
        "acceptance": [
          "Decoding a spawn-minted agent JWT shows sub='agent:<uuid>', aud matching 'gyre'/GYRE_OIDC_AUDIENCE, jti, task_id, spawned_by, persona when a persona was requested at spawn, stack_hash and wl_stack_hash equal to the stored workload-attestation stack fingerprint, attestation_level, and wl_pid/wl_hostname/wl_compute_target/wl_container_id whenever the corresponding spawn fact is known; default TTL is 3600 s.",
          "A valid, non-expired agent JWT spawned for task A gets 403 (or MCP PERMISSION_DENIED) on: PUT /api/v1/tasks/:id for task B (content update of title/description/priority/assigned_to/branch/pr_link/labels — not only the status transition), task-status transition of task B, gyre_update_task on task B, gyre_decompose_spec targeting task B as parent — and a git push to a repo not in its token's repo scope is rejected for the agent principal, as is MR creation outside its task scope.",
          "A valid, non-expired agent JWT spawned for task A gets 403 on POST /api/v1/agents/{task-B-agent-id}/complete, with task B NOT transitioned to Review and no completion MR created; the same boundary holds for POST /api/v1/agents/:id/stop and /fail against another task's agent (target agent status unchanged, its Task-scoped secrets not revoked), and for POST /api/v1/agents/spawn onto task B when the caller's token carries neither a matching task_id claim nor a spawn scope covering task B's repo/workspace.",
          "A token minted with a wrong/absent aud fails agent-token validation (audience validation is live, not disabled).",
          "After POST /api/v1/agents/{id}/complete, a request replaying the same jti receives 401 even though exp is in the future; the revocation is present in the SQL revoked_tokens table, survives a server restart, and revocation lookup precedes expiry validation.",
          "The spec-registry stack-pinning path is exercisable end-to-end: an approval event carrying the JWT-embedded stack hash matches a pinned requirement of the same hash and fails to match a different pinned hash.",
          "Non-agent principals (users, orchestrators, system) are not locked out of the task/repo/MR/agent-lifecycle surfaces they could previously use — enforcement is scoped to agent task-boundaries; any by-effect surface left unenforced carries an explicit, justified out-of-scope statement in the task text."
        ],
        "negative_probe": "Spawn an agent for task A with a valid, non-expired JWT and call, against task B or task B's agent: PUT /api/v1/tasks/:id (content update), PUT /api/v1/tasks/:id/status, MCP gyre_update_task, MCP gyre_decompose_spec with parent_task_id = B, POST /api/v1/agents/{task-B-agent}/complete, POST /api/v1/agents/{task-B-agent}/stop, POST /api/v1/agents/{task-B-agent}/fail, and POST /api/v1/agents/spawn onto task B without a covering spawn scope; also push to an unrelated repo and open an MR for another task: every call must be rejected. Against current code ALL of these succeed: update_task, transition_task_status, complete_agent, stop_agent, and fail_agent take no auth extractor at all (complete is additionally reachable under DEFAULT configuration because the seeded 'agent-scoped-access' builtin allows the 'complete' action for Agent-role subjects on resource type '*'), spawn is contained only by ABAC policy default, the MCP task tools are tier-only (check_task_target never compares the task_id claim), and the push succeeds (authorize_repo_access returns Ok for any user_id.is_none() principal) — so the probe fails on pre-task code, proving the boundary is enforced per-call by a by-effect sweep of every task-mutating surface rather than by tier-level ABAC alone, by route-family enumeration, or by illustrative subset. Second probe: complete the agent, then replay a request with its token while exp is still future — must 401 via the jti revocation table (today only a KV map removal happens and the SQL table is dead code).",
        "dependencies": [
          "task-130"
        ],
        "existing_task": null
      },
      {
        "key": "sigstore-layer3-completion",
        "title": "Sigstore layer-3 completion: every commit signed, keyless by default, no silent fallback, real Rekor for attestation bundles",
        "covers": [
          "68b06ebc09a7c8d49aa516d0"
        ],
        "required_behavior": "Narrowly scoped delta on the completed task-107 Sigstore stack (real Fulcio v2 client and Rekor hashedrekord submission/verification already exist in sigstore.rs) — do not replay task-107. Close the remaining Layer-3 shortfalls: (1) 'Every commit is cryptographically signed': extend signing beyond the single jj squash call site to all commit-producing surfaces — jj new, git HTTP push processing, and MR merge-commit creation — each producing a persisted CommitSignature record; GYRE_SIGNING_MODE=none remains only an explicit operator opt-in and must never be the default. (2) Keyless by default: the default SigningMode becomes Fulcio (spec: keyless signing, 'no long-lived GPG keys to manage or rotate'); local forge-key signing remains available only as explicit configuration. (3) No silent degradation: on Fulcio failure the commit path must fail loudly (explicit error or operator-configured behavior surfaced in the response/audit), never silently fall back to local forge-key signing while presenting the commit as signed; non-JWT callers must not present an empty bearer token to Fulcio (today the empty token guarantees failure and fallback) — either resolve a real identity for the caller or refuse keyless signing with an explicit error. (4) The signature proves which agent, on which task, spawned by which user: bind task_id and spawned_by into the signed/verified material — e.g. sign a canonical payload containing commit SHA + task_id + spawned_by and/or carry them in the Fulcio certificate request and Rekor entry — so that verification cryptographically checks them, rather than relying solely on detached plaintext DB columns. (5) Non-repudiation for all signed artifacts: replace the NoopRekorClient wired into merge-attestation bundle storage (rekor.rs, TODO task-144 step 4) with the real Rekor client — merge attestation bundles get real Rekor entries (entry UUID stored) when Rekor is configured, and a Rekor failure must not be reported as successful transparency-log recording.",
        "acceptance": [
          "With default configuration, commits created through every Gyre commit surface (jj new, jj squash, git HTTP push, MR merge) each have a persisted CommitSignature record, and the existing verification endpoint validates them.",
          "Default GYRE_SIGNING_MODE (unset) selects Fulcio keyless signing; local key signing requires explicit configuration.",
          "Fulcio unreachable (or returning an error) with keyless mode active: the commit operation fails with an explicit error — no record signed with the local forge key is produced for that operation, and the audit/response identifies the actual signing mode used.",
          "Verification of a signature whose task_id or spawned_by attribution does not match the expected values fails; the attribution is covered by the cryptographic signature/Rekor entry, not only by plaintext DB fields.",
          "Merge-attestation bundles are submitted to the configured Rekor instance with the stored entry UUID; NoopRekorClient is no longer wired into any production path, and a Rekor outage produces an explicit failure/warning state, not a silent Ok(None)."
        ],
        "negative_probe": "Point GYRE_FULCIO_URL at an unreachable endpoint with default (keyless) configuration and drive a commit through any surface: the operation must fail loudly — today jj squash logs 'Fulcio keyless signing failed; falling back to local signing' and produces a local-key record, so a probe asserting 'no local-signed record and no success response' kills the silent-fallback bug. Second probe: take a stored CommitSignature, tamper the task_id attribution (or swap it with another task's), and run verification — it must fail, proving attribution is cryptographically bound rather than a plaintext column a mismatched test could mirror.",
        "dependencies": [
          "task-107"
        ],
        "existing_task": null
      },
      {
        "key": "scim-users-lifecycle",
        "title": "SCIM user lifecycle completion: PATCH, deactivation semantics, enterprise extension, production wiring",
        "covers": [
          "e88b0643650de3d8f271012a",
          "2cf8a1c0fbde5c0d82e85317",
          "b2b37ae05e75a44a9117ceca"
        ],
        "required_behavior": "Complete the SCIM 2.0 user-lifecycle half of the User Identity section so IdPs can push creates, updates, and deactivations automatically. (1) Fix the production wiring defect: require_auth_middleware (lib.rs) currently layers over all of api_router() including /scim/v2/* and has no GYRE_SCIM_TOKEN branch, so a request bearing only the dedicated SCIM bearer token is rejected 401 before check_scim_auth runs (the existing tests bypass this by mounting api_router() directly). Exempt /scim/v2/* from the global auth middleware (handlers keep enforcing check_scim_auth: fail-closed 403 when GYRE_SCIM_TOKEN is unset, constant-time bearer compare, separate from agent JWTs), keeping the ABAC-exempt-handler and route-registry gates green. New tests must go through the full build_router() stack. (2) Implement PATCH /scim/v2/Users/{id} (RFC 7644 partial update) including the deactivation path active:false, and update ServiceProviderConfig to advertise patch supported. (3) Deactivation lifecycle per spec: active:false deactivates the user, revokes all active sessions (revoke_all_for_user — never currently invoked from SCIM), and removes the deactivated user from active agents' spawned_by tracking; SCIM DELETE deprovisions by marking the user inactive (no hard store delete — the current handler hard-deletes); the serialized resource's active field reflects the stored user state instead of the hardcoded true. (4) Enterprise extension (RFC 7643 §4.3): serialize and accept urn:ietf:params:scim:schemas:extension:enterprise:2.0:User with organization (tenant-id) and manager.value on create/PUT/PATCH. (5) Preserve the existing core-schema behavior: userName immutable after creation, externalId↔external_id mapping, idempotent create, emails, meta, no password fields.",
        "acceptance": [
          "Through the production build_router() stack, a request bearing only Authorization: Bearer $GYRE_SCIM_TOKEN reaches the SCIM handlers and succeeds; with the token unset or wrong, every /scim/v2 request is rejected (403/401) — verified end-to-end, not via a directly mounted api_router().",
          "PATCH /scim/v2/Users/{id} with {\"active\": false} deactivates the user: the stored record is inactive, the serialized SCIM resource reports active:false, all of that user's active sessions are revoked (a subsequent authenticated request with a pre-deactivation session token gets 401), and active agents' spawned_by tracking no longer attributes to the deactivated user.",
          "SCIM DELETE marks the user inactive (deprovision) — the user row remains resolvable (not a hard delete), with the same session-revocation and spawned_by effects as active:false.",
          "User resources include the enterprise extension schema in schemas and carry organization (tenant id) and manager.value; PUT/PATCH accept and persist them.",
          "PUT/PATCH attempting to change userName or externalId is rejected/ignored (immutable), and ServiceProviderConfig advertises patch as supported."
        ],
        "negative_probe": "Through build_router(): (a) send GET /scim/v2/Users with only the SCIM bearer token — against current code this gets 401 'Invalid token' from require_auth_middleware before check_scim_auth ever runs, so the positive path failing pre-task proves the wiring fix is real; (b) with GYRE_SCIM_TOKEN unset, the same request must be rejected 403 — proving the exemption did not open an unauthenticated hole; (c) PATCH active:false on a user holding a live session, then replay that session — must 401, proving deactivation is a lifecycle event (sessions revoked), not a serialized flag flip (today active is hardcoded true and the parsed field is never read).",
        "dependencies": [],
        "existing_task": null
      },
      {
        "key": "scim-groups-workspace-mapping",
        "title": "SCIM groups and group-to-workspace role mapping",
        "covers": [
          "6273edccbcca379f21b54f97"
        ],
        "required_behavior": "Implement the SCIM Groups surface and the admin-configured group-to-workspace mapping so IdP group pushes drive Gyre workspace membership. (1) All six /scim/v2/Groups endpoints (GET list with startIndex/count pagination, GET {id}, POST create, PUT replace membership, PATCH partial update add/remove members, DELETE) per RFC 7643/7644 Group schema (schemas, id, externalId, displayName, members with value/display), authenticated by the same dedicated GYRE_SCIM_TOKEN bearer (fail-closed), persisted via a gyre-ports trait + SQLite/Postgres adapters with the next unused 6-digit migration sequence (no in-memory AppState store — restart-orphaned state is prohibited by the standing invariants). (2) PUT /api/v1/workspaces/{id}/scim-mappings (workspace-admin authorized, registered in the ABAC RouteResourceMapping) storing per-workspace mappings [{idp_group, workspace_role}] durably. (3) Mapping effects: when a SCIM group push (PUT/PATCH /scim/v2/Groups or group create with members) adds a user to a group that a workspace maps to a role, Gyre automatically creates the WorkspaceMembership record with that role; when the user is removed from the group, the membership is revoked. Unmapped groups have no membership effect. Idempotent: re-pushing the same membership does not duplicate records.",
        "acceptance": [
          "All six /scim/v2/Groups endpoints behave per RFC 7644 through the production router: list pagination, 404 on unknown id, create/replace/patch/delete with the Group schema, SCIM-token auth enforced on each.",
          "PUT /api/v1/workspaces/{id}/scim-mappings persists mappings [{idp_group, workspace_role}] (survive a server restart) and is rejected for non-admin callers.",
          "Adding user U to group G (via group PUT or PATCH) where G is mapped to role Developer in workspace W creates U's WorkspaceMembership in W with the Developer role — observable by U's subsequent workspace access at that role; removing U from G revokes that membership (the role-gated access subsequently fails).",
          "Group state (groups and memberships) is persisted in real storage behind a port with SQLite and PostgreSQL adapters, using the next unused migration sequence number.",
          "Adding a user to an unmapped group creates no WorkspaceMembership anywhere."
        ],
        "negative_probe": "Map group G to Developer in workspace W, then SCIM-push U into G and back out: after removal, U's role-gated workspace action (the one that succeeded while a member) must be denied — proving the 'membership is revoked' half of the behavior, not just the create half. Control case: push U into an unmapped group and assert no WorkspaceMembership row appears anywhere for U — proving mapping-driven effects rather than blanket membership creation. Pre-task, both probes trivially fail in the 'no effect' direction because no Group endpoint, mapping store, or membership effect exists at all (grep scim-mappings/ScimGroup: zero matches).",
        "dependencies": [
          "scim-users-lifecycle"
        ],
        "existing_task": null
      }
    ]
  },
  "assigned_names": {
    "spiffe-workload-attestation": "task-261",
    "spiffe-federation": "task-262",
    "agent-jwt-claims-task-scope": "task-263",
    "sigstore-layer3-completion": "task-264",
    "scim-users-lifecycle": "task-265",
    "scim-groups-workspace-mapping": "task-266",
    "_publication": "task-267",
    "_base": "321615eced877c4839ccb98b78fae10778559933"
  }
}

## Acceptance Criteria

- [ ] Every planned task faithfully implements its assigned canonical requirements.
- [ ] No duplicate active assignment or missing dependency is introduced.
- [ ] No product behavior, spec requirement, coverage status or enforcement is changed.
- [ ] New tasks remain not-started until this backlog publication merges.

## Shipped

Published the independently reviewed missing-work plan for `specs/system/identity-security.md` (plan `8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e`): task definitions and explicit section bindings only — no product implementation claimed.

### What was created

- `specs/tasks/task-261.md` (spiffe-workload-attestation) — X.509-SVIDs replacing the G10 local scheme; covers sections 9d623241914634d94371c864, 16fb4f5c865aa918b828b94e.
- `specs/tasks/task-262.md` (spiffe-federation) — SPIFFE trust-bundle exchange; depends on task-261; covers 05e06b05e91abddea8707d41.
- `specs/tasks/task-263.md` (agent-jwt-claims-task-scope) — claims completion plus by-effect task-scope enforcement sweep; depends on task-130; covers 91f35f78880c4d70c32342ee, 5576c3d85b41c8482ee8b1ee.
- `specs/tasks/task-264.md` (sigstore-layer3-completion) — every commit signed, keyless default, no silent fallback, real Rekor; depends on task-107; covers 68b06ebc09a7c8d49aa516d0.
- `specs/tasks/task-265.md` (scim-users-lifecycle) — PATCH, deactivation, enterprise extension, middleware wiring; covers e88b0643650de3d8f271012a, 2cf8a1c0fbde5c0d82e85317, b2b37ae05e75a44a9117ceca.
- `specs/tasks/task-266.md` (scim-groups-workspace-mapping) — Groups endpoints and group-to-workspace role mapping; depends on task-265; covers 6273edccbcca379f21b54f97.
- `specs/task-spec-inputs.json` — exact section bindings for all six tasks; all 10 audited gap sections of the document are covered exactly once; the 5 context/satisfied sections are intentionally unbound.

Each task doc carries the reviewed required behavior, acceptance criteria (unticked), verification intent (negative probe), and discovery evidence verbatim; all six remain `progress: not-started` per the acceptance criteria.

### Merge resolution

The branch history includes a merge of the concurrently merged task-260 publication (source-control backlog, 25497b9e) which conflicted in `specs/task-spec-inputs.json` where both publications appended distinct backlog entries. Resolved as the union: both the identity-security and source-control entries are present, the file is valid JSON, and the resolved key set equals the union of both parents' key sets (verified programmatically; no entry lost from either side).

### Verification evidence

- Programmatic comparison of each task doc against the embedded reviewed plan: title, required behavior, every acceptance criterion, negative probe, dependency frontmatter, and `not-started` progress all match exactly.
- All `backlog/{section-id}` entries resolve to existing headings in `specs/system/identity-security.md`.
- Duplicate-work scan: every prior task touching SPIFFE/SCIM/Sigstore surfaces is `complete` (task-107 Sigstore stack and task-130 token scoping are declared dependencies, not duplicates); no other active task covers these sections.
- Diff vs base 321615ec touches only `specs/tasks/task-26*.md` and `specs/task-spec-inputs.json` — no product file, coverage status, spec requirement, or gate changed.
- `scripts/test_pipeline_spec_backlog.py` (40 tests) and `scripts/test_pipeline_requirements.py` (23 tests) pass against the updated registry.
- Evidence artifacts saved under `/tmp/stage/review-evidence/`.
