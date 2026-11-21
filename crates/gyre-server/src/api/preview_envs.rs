//! Ephemeral preview environments — the "Try it" links of the activity
//! dashboard (activity-dashboard.md §"Try it" links).
//!
//! For completed work (a merged merge request), a workspace member can spin
//! up a short-lived environment running EXACTLY the commit that landed the
//! work, isolated from production, and reach it over the platform's
//! WireGuard mesh. This is the platform's realization of the spec's
//! "NixOS + Tailscale" ephemeral environment:
//!
//! - **Image**: the NixOS flake-based image path (`docker/gyre-agent/flake.nix`,
//!   `gyre-agent:latest`) — the same reproducible image agents run on.
//! - **Isolation**: the environment checks the artifact's commit out into
//!   its own scratch dir (a temp data dir + detached git worktree) and
//!   never touches the production database/storage.
//! - **Mesh**: the server generates a real X25519 WireGuard keypair for the
//!   environment, registers it as a `NetworkPeer` with an allocated mesh IP
//!   (network.rs `register_peer` semantics, server-side), and hands the
//!   private key + mesh IP to the environment so it can join the encrypted
//!   overlay.
//! - **Lifecycle**: `provisioning` → `active` → `expired`/`failed`, with a
//!   TTL expiry/teardown job on the jobs.rs scheduler
//!   ([`crate::preview_env_gc`]) that terminates the environment and
//!   removes its mesh peer.
//!
//! Analytics: provisioning records `preview.started` (user/workspace/repo/mr
//! properties); teardown records `preview.ended`, TTL expiry
//! `preview.expired`, and failures `preview.failed` — the "what did users
//! actually try?" half of the analytics feed.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use gyre_common::Id;
use gyre_domain::{
    Agent, AgentStatus, AnalyticsEvent, ComputeTargetEntity, ComputeTargetType, MergeRequest,
    MrStatus, NetworkPeer, PreviewEnvironment, PreviewEnvironmentStatus, Repository, UserRole,
};
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};

use crate::auth::AuthenticatedAgent;
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

/// The preview server script shipped in the agent image
/// (`docker/gyre-agent/preview-serve.mjs`). Single source of truth: the
/// image builds copy it to `/gyre/preview-serve.mjs`, and the server writes
/// this same embedded copy into a local-mode environment's scratch dir.
const PREVIEW_SERVE_MJS: &str = include_str!("../../../../docker/gyre-agent/preview-serve.mjs");

/// TTL for preview environments, seconds (GYRE_PREVIEW_ENV_TTL_SECS,
/// default 1 h). Ephemeral by contract: the expiry job tears every
/// environment down at or after this deadline.
pub fn preview_env_ttl_secs() -> u64 {
    std::env::var("GYRE_PREVIEW_ENV_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(3600)
}

/// Deterministic preview port for an environment id: 18000-18999. Recorded
/// on the row so the user (and the UI) can reach the environment without a
/// port-probing round trip.
fn preview_port(env_id: &str) -> u16 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env_id.hash(&mut hasher);
    18000 + (hasher.finish() % 1000) as u16
}

/// Generate a real WireGuard keypair for an environment: 32 random bytes
/// (ring CSPRNG) as the private key, X25519 basepoint multiplication for
/// the public key — the same relation `wg genkey` / `wg pubkey` produce, so
/// the environment can configure wg0 with the private half and the mesh
/// coordinates peers by the public half.
fn generate_wireguard_keypair() -> Result<(String, String), anyhow::Error> {
    use base64::Engine as _;
    use ring::rand::SecureRandom;

    let mut secret_bytes = [0u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut secret_bytes)
        .map_err(|_| anyhow::anyhow!("WireGuard key generation failed"))?;
    let secret = x25519_dalek::StaticSecret::from(secret_bytes);
    let public = x25519_dalek::PublicKey::from(&secret);
    let engine = base64::engine::general_purpose::STANDARD;
    Ok((
        engine.encode(secret_bytes),
        engine.encode(public.as_bytes()),
    ))
}

/// Per-handler authorization for every try-it route: the caller must be a
/// member of the environment's workspace (Admin bypasses). Same-tenant
/// non-members receive 403 BEFORE any side effect — no preview row, no mesh
/// peer, no agent row (the primary enforcement probe).
async fn require_workspace_access(
    state: &AppState,
    auth: &AuthenticatedAgent,
    workspace_id: &Id,
) -> Result<(), ApiError> {
    if auth.roles.contains(&UserRole::Admin) {
        return Ok(());
    }
    let is_member = if let Some(ref user_id) = auth.user_id {
        state
            .workspace_memberships
            .find_by_user_and_workspace(user_id, workspace_id)
            .await
            .map_err(ApiError::Internal)?
            .is_some()
    } else {
        // Agent token: the agent itself must live in this workspace.
        state
            .agents
            .find_by_id(&Id::new(&auth.agent_id))
            .await
            .map_err(ApiError::Internal)?
            .is_some_and(|a| a.workspace_id == *workspace_id)
    };
    if !is_member {
        return Err(ApiError::Forbidden(
            "not a member of this workspace".to_string(),
        ));
    }
    Ok(())
}

/// Resolve the exact commit a merged MR's environment must run: the
/// recorded merge commit SHA, falling back to the target branch tip (the
/// merge commit's location). Unresolvable → 400 with no side effects.
async fn resolve_merged_commit(mr: &MergeRequest, repo: &Repository) -> Result<String, ApiError> {
    let is_sha = |s: &str| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit());
    if let Some(sha) = &mr.merge_commit_sha {
        if is_sha(sha) {
            return Ok(sha.clone());
        }
    }
    // resolve_ref returns None when the ref does not resolve — there is no
    // valid empty-string SHA, so the failure is surfaced, never defaulted.
    crate::git_refs::resolve_ref(&repo.path, &mr.target_branch)
        .await
        .ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "cannot resolve commit for merge request {} (no merge commit SHA recorded and \
                 branch '{}' does not resolve in repo '{}')",
                mr.id, mr.target_branch, repo.id
            ))
        })
}

/// How a teardown ended — drives the final status, end_reason and the
/// analytics event name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeardownOutcome {
    /// Explicit DELETE by a user.
    TornDown,
    /// TTL expiry job.
    Expired,
    /// Provisioning failed or the process died.
    Failed,
}

/// Tear one environment down: kill its process, remove its mesh peer, stop
/// its ephemeral agent row, release its budget slot and scratch state, and
/// persist the final status. Shared by the DELETE handler, the failed
/// -dispatch path and the TTL/liveness GC job so every exit path reclaims
/// exactly the same resources. The row survives as the audit record of what
/// ran.
pub(crate) async fn teardown_preview_environment(
    state: &AppState,
    mut env: PreviewEnvironment,
    outcome: TeardownOutcome,
) {
    let was_live = env.status.is_live();
    let now = now_secs();

    // 1. Terminate the process. Live registry first; after a server
    //    restart the registry is empty, so fall back to the durable handle
    //    recorded on the row.
    kill_environment_process(state, &env).await;

    // 2. Remove the mesh peer — the environment is no longer reachable.
    if let Some(peer_id) = env.peer_id.clone() {
        if let Err(e) = state.network_peers.delete(&peer_id).await {
            tracing::warn!(env_id = %env.id, "preview mesh peer removal failed: {e:#}");
        }
    }

    // 3. Stop the ephemeral agent row and revoke its token.
    if let Ok(Some(mut agent)) = state.agents.find_by_id(&env.agent_id).await {
        if matches!(agent.status, AgentStatus::Active | AgentStatus::Idle) {
            if let Err(te) = agent.transition_status(AgentStatus::Stopped) {
                tracing::warn!(env_id = %env.id, "preview agent stop transition rejected: {te:?}");
            }
            if let Err(ue) = state.agents.update(&agent).await {
                tracing::warn!(env_id = %env.id, "preview agent stop write failed: {ue:#}");
            }
        }
    }
    if let Err(e) = state
        .kv_store
        .kv_remove("agent_tokens", &env.agent_id.to_string())
        .await
    {
        tracing::warn!(env_id = %env.id, "preview token revocation failed: {e:#}");
    }

    // 4. Release the budget slot the ephemeral agent occupied (only if the
    //    environment still held one — a double teardown must not decrement
    //    a counter twice and permanently bias admission).
    if was_live {
        super::budget::decrement_active_agents(state, env.workspace_id.as_str()).await;
    }

    // 5. Remove the isolated scratch state (local-mode worktree + scratch
    //    dir). Absent paths are not errors — the caller asked for them gone.
    remove_environment_scratch(state, &env).await;

    // 6. Persist the final status. The row is kept: it is the durable
    //    record of what ran, for whom, at which commit.
    env.status = match outcome {
        TeardownOutcome::TornDown | TeardownOutcome::Expired => PreviewEnvironmentStatus::Expired,
        TeardownOutcome::Failed => PreviewEnvironmentStatus::Failed,
    };
    env.ended_at = Some(now);
    env.end_reason = Some(
        match outcome {
            TeardownOutcome::TornDown => "torn-down",
            TeardownOutcome::Expired => "expired",
            TeardownOutcome::Failed => "failed",
        }
        .to_string(),
    );
    if let Err(e) = state.preview_environments.update(&env).await {
        tracing::warn!(env_id = %env.id, "preview environment final status write failed: {e:#}");
    }

    // 7. Analytics — the trial half of the feed: what did users try, and
    //    how did it end.
    let event_name = match outcome {
        TeardownOutcome::TornDown => "preview.ended",
        TeardownOutcome::Expired => "preview.expired",
        TeardownOutcome::Failed => "preview.failed",
    };
    record_preview_analytics(
        state,
        event_name,
        &env,
        serde_json::json!({
            "end_reason": env.end_reason,
        }),
    )
    .await;
}

/// Kill the environment's process: through the live registry when the
/// server still owns the handle, else through the durable handle recorded
/// on the row (restart recovery).
async fn kill_environment_process(state: &AppState, env: &PreviewEnvironment) {
    let agent_key = env.agent_id.to_string();
    let handle = state.process_registry.lock().await.remove(&agent_key);
    let backend = state.spawned_backends.lock().await.remove(&agent_key);
    if let Some(handle) = handle {
        let kill_result = match backend.as_deref() {
            Some(b) => ComputeTarget::kill_process(b, &handle).await,
            None => super::spawn::kill_process_handle(&handle).await,
        };
        if let Err(e) = kill_result {
            tracing::warn!(env_id = %env.id, "preview process kill failed: {e:#}");
        }
        return;
    }
    // Restart recovery: the registry is empty but the row carries the
    // handle. Route by the recorded target label — a container handle must
    // not be killed as a bare local pid.
    if let Some(handle_json) = &env.process_handle {
        if let Ok(handle) = serde_json::from_str::<ProcessHandle>(handle_json) {
            let kill_result = match env.compute_target.as_str() {
                "container" => {
                    let image = env
                        .image
                        .clone()
                        .unwrap_or_else(|| "gyre-agent:latest".to_string());
                    let ct = gyre_adapters::compute::ContainerTarget::new(image);
                    ComputeTarget::kill_process(&ct, &handle).await
                }
                "local" => {
                    ComputeTarget::kill_process(&gyre_adapters::compute::LocalTarget, &handle).await
                }
                _ => super::spawn::kill_process_handle(&handle).await,
            };
            if let Err(e) = kill_result {
                tracing::warn!(env_id = %env.id, "preview process kill (durable handle) failed: {e:#}");
            }
        }
    }
}

/// Remove the environment's isolated scratch state: the detached worktree
/// (local mode) and the scratch dir itself. The scratch dir is only ever
/// created under the system temp dir; removal is refused for any path
/// outside it so a corrupted row can never point teardown at production
/// storage.
async fn remove_environment_scratch(state: &AppState, env: &PreviewEnvironment) {
    let data_dir = std::path::PathBuf::from(&env.data_dir);
    let temp_root = std::env::temp_dir();
    let confined = data_dir.is_absolute()
        && (data_dir.starts_with(&temp_root)
            || std::fs::canonicalize(&data_dir)
                .map(|c| c.starts_with(&temp_root))
                .unwrap_or(false));
    if !confined {
        tracing::warn!(
            env_id = %env.id,
            data_dir = %env.data_dir,
            "preview scratch dir is outside the system temp dir; refusing to remove"
        );
        return;
    }

    // Local mode checked the artifact's commit out into a detached worktree
    // under the scratch dir; remove it through git so the production repo's
    // worktree registry stays clean.
    let worktree = data_dir.join("repo");
    if worktree.exists() {
        if let Ok(Some(repo)) = state.repos.find_by_id(&env.repo_id).await {
            if let Err(e) = state
                .git_ops
                .force_remove_worktree(&repo.path, &worktree.to_string_lossy())
                .await
            {
                tracing::warn!(env_id = %env.id, "preview worktree removal failed: {e:#}");
            }
        }
    }
    if let Err(e) = tokio::fs::remove_dir_all(&data_dir).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(env_id = %env.id, "preview scratch dir removal failed: {e:#}");
        }
    }
}

async fn record_preview_analytics(
    state: &AppState,
    event_name: &str,
    env: &PreviewEnvironment,
    extra_properties: serde_json::Value,
) {
    let mut properties = serde_json::json!({
        "preview_env_id": env.id.to_string(),
        "mr_id": env.mr_id.to_string(),
        "repo_id": env.repo_id.to_string(),
        "commit_sha": env.commit_sha,
        "branch": env.branch,
        "compute_target": env.compute_target,
        "mesh_ip": env.mesh_ip,
    });
    if let (Some(base), Some(extra)) = (properties.as_object_mut(), extra_properties.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    let ev = AnalyticsEvent::new(new_id(), event_name, None, properties, now_secs()).with_scope(
        Some(&env.requested_by),
        None,
        Some(&env.workspace_id),
        Some(&env.repo_id),
    );
    if let Err(e) = state.analytics.record(&ev).await {
        tracing::warn!(env_id = %env.id, "preview analytics record failed: {e:#}");
    }
}

/// POST /api/v1/merge-requests/:id/try-it (alias: /api/v1/mrs/:id/try-it) —
/// provision an ephemeral preview environment for a MERGED merge request's
/// commit.
///
/// Authorization: per-handler workspace-membership check runs FIRST — a
/// same-tenant non-member receives 403 with no side effects (no preview
/// row, no mesh peer, no agent row). Non-merged MRs and unresolvable
/// commits receive 4xx with no side effects.
pub async fn try_it_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<PreviewEnvironmentResponse>), ApiError> {
    let mr_id = Id::new(&id);
    let mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;
    let repo = state
        .repos
        .find_by_id(&mr.repository_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repository {} not found", mr.repository_id)))?;

    // Per-handler authorization BEFORE any side effect (primary probe).
    require_workspace_access(&state, &auth, &mr.workspace_id).await?;

    // Only completed work can be tried: the MR must be merged.
    if mr.status != MrStatus::Merged {
        return Err(ApiError::InvalidInput(format!(
            "merge request {} is '{:?}' — preview environments run merged work \
             (the MR's landed commit)",
            mr.id, mr.status
        )));
    }

    // The exact version of the code this environment runs.
    let commit_sha = resolve_merged_commit(&mr, &repo).await?;

    let workspace = state
        .workspaces
        .find_by_id(&mr.workspace_id)
        .await?
        .ok_or_else(|| {
            // The MR's tenant scope cannot be determined without the
            // workspace record — skip the operation rather than fabricate a
            // "default" identity that could silently re-target it.
            ApiError::NotFound(format!(
                "workspace {} for merge request {} not found",
                mr.workspace_id, mr.id
            ))
        })?;
    let now = now_secs();
    let env_id = new_id();
    let agent_id = new_id();
    let env_id_str = env_id.to_string();
    let short: String = env_id_str.chars().take(8).collect();
    let port = preview_port(&env_id_str);
    let ttl = preview_env_ttl_secs();

    // Isolated scratch state: the environment's own temp data dir under the
    // system temp dir — never the production database/storage.
    let data_dir = std::env::temp_dir().join(format!("gyre-preview-env-{env_id_str}"));
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("create preview scratch dir: {e}")))?;
    // Local mode runs the embedded preview server from the scratch dir.
    tokio::fs::write(data_dir.join("preview-serve.mjs"), PREVIEW_SERVE_MJS)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("write preview server script: {e}")))?;

    // Ephemeral agent row: owns the dispatched process (kill routing,
    // exit bookkeeping, budget occupancy) exactly like meta-spec preview
    // agents. Real scope — never the constructor's bootstrap placeholder.
    let mut agent = Agent::new(agent_id.clone(), format!("preview-env-{short}"), now);
    agent.spawned_by = Some(auth.agent_id.clone());
    agent.workspace_id = mr.workspace_id.clone();
    agent.repo_id = Some(repo.id.clone());
    agent
        .transition_status(AgentStatus::Active)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("preview agent status: {e}")))?;
    state
        .agents
        .create(&agent)
        .await
        .map_err(ApiError::Internal)?;

    // Short-lived agent token: the container bootstrap (clone via git HTTP,
    // heartbeat) authenticates with it. Scope claims derive from the loaded
    // workspace/repo rows, never caller input.
    let token = state
        .agent_signing_key
        .mint(
            &agent_id.to_string(),
            &env_id_str,
            &auth.agent_id,
            &state.base_url,
            state.preview_jwt_ttl_secs,
            workspace.tenant_id.as_str(),
            workspace.id.as_str(),
            repo.id.as_str(),
            &repo.name,
        )
        .map_err(ApiError::BadRequest)?;
    state
        .kv_store
        .kv_set("agent_tokens", &agent_id.to_string(), token.clone())
        .await
        .map_err(ApiError::Internal)?;

    // Mesh peer: real X25519 keypair, allocated mesh IP, registered on the
    // coordination plane so the requesting user can reach the environment
    // over the encrypted overlay. The private half travels to the
    // environment via launch env (same trust model as the SVID key).
    let (wg_priv_b64, wg_pub_b64) = generate_wireguard_keypair().map_err(ApiError::Internal)?;
    let mesh_ip = state.wg_config.allocate_ip();
    let mut peer = NetworkPeer::new(
        new_id(),
        agent_id.clone(),
        wg_pub_b64,
        None,
        mesh_ip
            .as_ref()
            .map(|ip| vec![format!("{ip}/32")])
            .unwrap_or_default(),
        now,
    );
    peer.mesh_ip = mesh_ip.clone();
    state.network_peers.register(&peer).await?;

    // Durable row BEFORE dispatch: the provisioning record survives a
    // crash mid-flight and the TTL job reclaims half-provisioned envs.
    let mut env = PreviewEnvironment::new(
        env_id.clone(),
        workspace.tenant_id.clone(),
        mr.workspace_id.clone(),
        repo.id.clone(),
        mr.id.clone(),
        auth.user_id
            .clone()
            .unwrap_or_else(|| Id::new(&auth.agent_id)),
        commit_sha.clone(),
        mr.target_branch.clone(),
        agent_id.clone(),
        "local",
        data_dir.to_string_lossy().into_owned(),
        port,
        now,
        now + ttl,
    );
    env.mesh_ip = mesh_ip.clone();
    env.peer_id = Some(peer.id.clone());
    state.preview_environments.create(&env).await?;

    // The trial half of the analytics feed: what the user actually TRIED.
    record_preview_analytics(
        &state,
        "preview.started",
        &env,
        serde_json::json!({
            "ttl_secs": ttl,
        }),
    )
    .await;

    // Preview agents are real processes: count them like any other for
    // workspace/tenant occupancy, mirroring the meta-spec preview flow.
    super::budget::increment_active_agents(&state, mr.workspace_id.as_str()).await;

    // ── Dispatch through the compute-target machinery ─────────────────
    //
    // Container/SSH/Kubernetes targets: the platform's single dispatch
    // (`dispatch_agent_launch`) runs the NixOS flake-built agent image; its
    // entrypoint bootstraps (clone of the MR's target branch with the
    // agent token) and then execs our args: checkout the exact merge commit
    // and serve it. No resolved target: a local dispatch on the compute
    // machinery (`ComputeTarget::spawn_process` on `LocalTarget`) with a
    // detached worktree of the commit in the scratch dir — deterministic,
    // still isolated from production.
    let ct_entity: Option<ComputeTargetEntity> =
        if let Some(ct_id) = workspace.compute_target_id.clone() {
            state.compute_targets.get_by_id(&ct_id).await.ok().flatten()
        } else {
            state
                .compute_targets
                .get_default_for_tenant(&workspace.tenant_id)
                .await
                .ok()
                .flatten()
        };
    let target_label = ct_entity
        .as_ref()
        .map(|e| match e.target_type {
            ComputeTargetType::Container => "container",
            ComputeTargetType::Ssh => "ssh",
            ComputeTargetType::Kubernetes => "kubernetes",
        })
        .unwrap_or("local")
        .to_string();
    let resolved_target_config = ct_entity.map(|e| super::compute::ComputeTargetConfig {
        id: e.id.to_string(),
        name: e.name.clone(),
        target_type: target_label.clone(),
        config: e.config.clone(),
    });

    let data_dir_str = data_dir.to_string_lossy().into_owned();
    let mut launch_env: HashMap<String, String> = HashMap::new();
    launch_env.insert("GYRE_SERVER_URL".to_string(), state.base_url.clone());
    launch_env.insert("GYRE_AUTH_TOKEN".to_string(), token.clone());
    launch_env.insert(
        "GYRE_CLONE_URL".to_string(),
        super::spawn::build_clone_url(&state, Some(&workspace), &repo),
    );
    launch_env.insert("GYRE_BRANCH".to_string(), mr.target_branch.clone());
    launch_env.insert("GYRE_AGENT_ID".to_string(), agent_id.to_string());
    launch_env.insert("GYRE_REPO_ID".to_string(), repo.id.to_string());
    launch_env.insert("GYRE_REPO_PATH".to_string(), repo.path.clone());
    launch_env.insert("GYRE_COMMIT_SHA".to_string(), commit_sha.clone());
    launch_env.insert("GYRE_PREVIEW_ENV_ID".to_string(), env_id_str.clone());
    launch_env.insert("GYRE_PREVIEW_PORT".to_string(), port.to_string());
    launch_env.insert("GYRE_PREVIEW_DATA_DIR".to_string(), data_dir_str.clone());
    if let Some(ip) = &mesh_ip {
        launch_env.insert("GYRE_WG_MESH_IP".to_string(), ip.clone());
        launch_env.insert("GYRE_WG_PRIVATE_KEY".to_string(), wg_priv_b64.clone());
    }

    // One script, two contexts: inside the image the entrypoint has already
    // cloned the target branch to /workspace/repo; on a local dispatch the
    // checkout is a detached worktree of the exact commit in the scratch
    // dir (never the production repo's working tree).
    let script = format!(
        "set -e\n\
         if [ -d /workspace/repo ]; then\n\
         \x20 cd /workspace/repo\n\
         \x20 git checkout --detach \"$GYRE_COMMIT_SHA\"\n\
         \x20 exec node /gyre/preview-serve.mjs\n\
         else\n\
         \x20 git -C \"$GYRE_REPO_PATH\" worktree add --detach \
         \"$GYRE_PREVIEW_DATA_DIR/repo\" \"$GYRE_COMMIT_SHA\"\n\
         \x20 cd \"$GYRE_PREVIEW_DATA_DIR/repo\"\n\
         \x20 exec node \"$GYRE_PREVIEW_DATA_DIR/preview-serve.mjs\"\n\
         fi\n"
    );

    let agent_key = agent_id.to_string();
    let launched;
    match &resolved_target_config {
        Some(_) => {
            let dispatch = super::spawn::dispatch_agent_launch(
                &state,
                &agent,
                &resolved_target_config,
                "bash",
                &["-c".to_string(), script.clone()],
                &launch_env,
                &data_dir_str,
            )
            .await?;
            launched = dispatch.launched;
            if launched {
                if let Some(image) = &dispatch.container_image {
                    env.image = Some(image.clone());
                }
            }
        }
        None => {
            let spawn_config = SpawnConfig {
                name: format!("gyre-preview-{short}"),
                command: "bash".to_string(),
                args: vec!["-c".to_string(), script.clone()],
                env: launch_env,
                work_dir: data_dir_str.clone(),
            };
            match ComputeTarget::spawn_process(&gyre_adapters::compute::LocalTarget, &spawn_config)
                .await
            {
                Ok(handle) => {
                    // Register for kill routing — the same registry the
                    // agent dispatch uses, so teardown kills through the
                    // backend that owns the handle.
                    state
                        .process_registry
                        .lock()
                        .await
                        .insert(agent_key.clone(), handle.clone());
                    state.spawned_backends.lock().await.insert(
                        agent_key.clone(),
                        Arc::new(super::spawn::SpawnBackend::Local),
                    );
                    launched = true;
                }
                Err(e) => {
                    tracing::warn!(env_id = %env.id, "preview local spawn failed: {e:#}");
                    launched = false;
                }
            }
        }
    }

    env.compute_target = target_label;
    if launched {
        // Record the durable process handle (restart recovery) — read from
        // the registry the dispatch registered into.
        let handle = state.process_registry.lock().await.get(&agent_key).cloned();
        if let Some(handle) = handle {
            env.process_handle = serde_json::to_string(&handle).ok();
        }
        env.status = PreviewEnvironmentStatus::Active;
        state.preview_environments.update(&env).await?;
    } else {
        // Honest failure: the dispatch was driven but no process started
        // (missing image/runtime). Reclaim the peer, slot and agent row;
        // the durable row records the failure.
        let failed_env = env.clone();
        state.preview_environments.update(&env).await?;
        teardown_preview_environment(&state, failed_env, TeardownOutcome::Failed).await;
        env = state
            .preview_environments
            .find_by_id(&env.id)
            .await?
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("preview environment vanished")))?;
    }

    Ok((
        StatusCode::CREATED,
        Json(PreviewEnvironmentResponse::from(env)),
    ))
}

/// GET /api/v1/merge-requests/:id/try-it (alias: /api/v1/mrs/:id/try-it) —
/// the MR's preview environments, newest first (status/list surface).
pub async fn list_mr_environments(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<Vec<PreviewEnvironmentResponse>>, ApiError> {
    let mr_id = Id::new(&id);
    let mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;
    require_workspace_access(&state, &auth, &mr.workspace_id).await?;
    let envs = state.preview_environments.find_by_mr(&mr.id).await?;
    Ok(Json(
        envs.into_iter()
            .map(PreviewEnvironmentResponse::from)
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct ListEnvironmentsQuery {
    pub workspace_id: Option<String>,
}

/// GET /api/v1/preview-environments?workspace_id=W — environments in a
/// workspace (membership enforced per-handler).
pub async fn list_environments(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Query(params): Query<ListEnvironmentsQuery>,
) -> Result<Json<Vec<PreviewEnvironmentResponse>>, ApiError> {
    let ws_id = params
        .workspace_id
        .ok_or_else(|| ApiError::InvalidInput("workspace_id query param required".to_string()))?;
    let ws_id = Id::new(&ws_id);
    require_workspace_access(&state, &auth, &ws_id).await?;
    let envs = state.preview_environments.find_by_workspace(&ws_id).await?;
    Ok(Json(
        envs.into_iter()
            .map(PreviewEnvironmentResponse::from)
            .collect(),
    ))
}

/// GET /api/v1/preview-environments/:id — one environment's status.
pub async fn get_environment(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<Json<PreviewEnvironmentResponse>, ApiError> {
    let env = state
        .preview_environments
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("preview environment {id} not found")))?;
    require_workspace_access(&state, &auth, &env.workspace_id).await?;
    Ok(Json(PreviewEnvironmentResponse::from(env)))
}

/// DELETE /api/v1/preview-environments/:id — teardown: terminate the
/// environment and remove its mesh peer. The row survives (status
/// `expired`, end_reason `torn-down`) as the audit record.
pub async fn teardown_environment_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let env = state
        .preview_environments
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("preview environment {id} not found")))?;
    require_workspace_access(&state, &auth, &env.workspace_id).await?;
    if !env.status.is_live() {
        // Already ended — teardown is idempotent, but say so.
        return Err(ApiError::Conflict(format!(
            "preview environment {id} already {}",
            env.status.as_str()
        )));
    }
    teardown_preview_environment(&state, env, TeardownOutcome::TornDown).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct PreviewEnvironmentResponse {
    pub id: String,
    pub tenant_id: String,
    pub workspace_id: String,
    pub repo_id: String,
    pub mr_id: String,
    pub requested_by: String,
    pub commit_sha: String,
    pub branch: String,
    pub status: &'static str,
    pub mesh_ip: Option<String>,
    pub peer_id: Option<String>,
    pub agent_id: String,
    pub compute_target: String,
    pub image: Option<String>,
    pub port: u16,
    pub created_at: u64,
    pub expires_at: u64,
    pub ended_at: Option<u64>,
    pub end_reason: Option<String>,
    /// Where the requesting user reaches the preview: the mesh IP when
    /// WireGuard is enabled, else the server host.
    pub preview_url: String,
}

impl From<PreviewEnvironment> for PreviewEnvironmentResponse {
    fn from(env: PreviewEnvironment) -> Self {
        let host = env
            .mesh_ip
            .clone()
            .unwrap_or_else(|| "localhost".to_string());
        Self {
            id: env.id.to_string(),
            tenant_id: env.tenant_id.to_string(),
            workspace_id: env.workspace_id.to_string(),
            repo_id: env.repo_id.to_string(),
            mr_id: env.mr_id.to_string(),
            requested_by: env.requested_by.to_string(),
            commit_sha: env.commit_sha,
            branch: env.branch,
            status: env.status.as_str(),
            mesh_ip: env.mesh_ip,
            peer_id: env.peer_id.map(|p| p.to_string()),
            agent_id: env.agent_id.to_string(),
            compute_target: env.compute_target,
            image: env.image,
            port: env.port,
            created_at: env.created_at,
            expires_at: env.expires_at,
            ended_at: env.ended_at,
            end_reason: env.end_reason,
            preview_url: format!("http://{host}:{}", env.port),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::Id;
    use gyre_domain::{
        MergeRequest, MrStatus, Repository, User, Workspace, WorkspaceMembership, WorkspaceRole,
    };
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(state: std::sync::Arc<crate::AppState>) -> Router {
        crate::api::api_router().with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Seed a workspace in tenant "default" plus a user with an API key,
    /// optionally granting workspace membership. Returns the raw key.
    async fn seed_user(
        state: &std::sync::Arc<crate::AppState>,
        workspace_id: &str,
        user_id: &str,
        member: bool,
    ) -> String {
        let ws = Workspace::new(
            Id::new(workspace_id),
            Id::new("default"),
            workspace_id,
            workspace_id,
            0,
        );
        // Idempotent: a second seed (e.g. the outsider in a 403 probe)
        // must not trip the (tenant, slug) uniqueness contract.
        if state.workspaces.find_by_id(&ws.id).await.unwrap().is_none() {
            state.workspaces.create(&ws).await.unwrap();
        }

        let user = User {
            tenant_id: Some(Id::new("default")),
            ..User::new(Id::new(user_id), format!("ext-{user_id}"), user_id, 0)
        };
        state.users.create(&user).await.unwrap();
        if member {
            let membership = WorkspaceMembership::new(
                Id::new(format!("mem-{user_id}-{workspace_id}")),
                user.id.clone(),
                ws.id.clone(),
                WorkspaceRole::Viewer,
                Id::new("test-inviter"),
                0,
            );
            state
                .workspace_memberships
                .create(&membership)
                .await
                .unwrap();
        }
        let raw_key = format!("key-{user_id}");
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &user.id, "test-key")
            .await
            .unwrap();
        raw_key
    }

    /// Seed a merged MR (with its recorded merge commit) plus its repo.
    async fn seed_merged_mr(
        state: &std::sync::Arc<crate::AppState>,
        workspace_id: &str,
        mr_id: &str,
        status: MrStatus,
        merge_commit: Option<&str>,
    ) -> gyre_domain::MergeRequest {
        let repo = Repository::new(
            Id::new(format!("repo-{mr_id}")),
            Id::new(workspace_id),
            format!("repo-{mr_id}"),
            format!("/tmp/gyre-test-repos/repo-{mr_id}"),
            0,
        );
        state.repos.create(&repo).await.unwrap();
        let mut mr = MergeRequest::new(
            Id::new(mr_id),
            repo.id.clone(),
            Id::new(workspace_id),
            format!("mr {mr_id}"),
            "feature",
            "main",
            0,
        );
        mr.status = status;
        mr.merge_commit_sha = merge_commit.map(String::from);
        state.merge_requests.create(&mr).await.unwrap();
        mr
    }

    async fn try_it(app: &Router, mr_id: &str, token: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/mrs/{mr_id}/try-it"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        if status == StatusCode::NO_CONTENT {
            return (status, serde_json::Value::Null);
        }
        (status, body_json(resp).await)
    }

    /// The primary enforcement probe: a same-tenant user with NO membership
    /// in W POSTs try-it for a merged MR in W — 403, and a direct
    /// repository query finds zero preview rows and no newly registered
    /// mesh peer.
    #[tokio::test]
    async fn try_it_non_member_gets_403_with_no_side_effects() {
        let state = test_state();
        seed_merged_mr(&state, "ws-try", "mr-try-1", MrStatus::Merged, Some(&sha())).await;
        let non_member_key = seed_user(&state, "ws-try", "u-nonmember", false).await;

        let peers_before = state.network_peers.list().await.unwrap().len();

        let (status, json) = try_it(&app(state.clone()), "mr-try-1", &non_member_key).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");

        // Direct repository queries: no preview row, no mesh peer.
        let envs = state
            .preview_environments
            .find_by_mr(&Id::new("mr-try-1"))
            .await
            .unwrap();
        assert!(envs.is_empty(), "no preview row may exist after a 403");
        let peers_after = state.network_peers.list().await.unwrap().len();
        assert_eq!(
            peers_after, peers_before,
            "no mesh peer may be created after a 403"
        );
        // And no analytics trial event was recorded.
        assert_eq!(count_preview_events(&state, "preview.started").await, 0);
    }

    /// The canonical route must behave identically (the alias is a
    /// convenience, not a separate authorization surface).
    #[tokio::test]
    async fn try_it_canonical_route_enforces_the_same_membership() {
        let state = test_state();
        seed_merged_mr(&state, "ws-try", "mr-try-2", MrStatus::Merged, Some(&sha())).await;
        let non_member_key = seed_user(&state, "ws-try", "u-nonmember2", false).await;

        let resp = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests/mr-try-2/try-it")
                    .header("authorization", format!("Bearer {non_member_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let envs = state
            .preview_environments
            .find_by_mr(&Id::new("mr-try-2"))
            .await
            .unwrap();
        assert!(envs.is_empty());
    }

    /// Unresolvable MR → 404, non-merged MR → 400, both with no side
    /// effects.
    #[tokio::test]
    async fn try_it_unresolvable_and_unmerged_rejected_without_side_effects() {
        let state = test_state();
        let member_key = seed_user(&state, "ws-try", "u-member-a", true).await;

        // Unresolvable MR id.
        let (status, json) = try_it(&app(state.clone()), "mr-does-not-exist", &member_key).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "body: {json}");

        // Open MR (not completed work).
        seed_merged_mr(&state, "ws-try", "mr-open", MrStatus::Open, None).await;
        let (status, json) = try_it(&app(state.clone()), "mr-open", &member_key).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("merged"),
            "body should name the merged-only rule: {json}"
        );

        // Merged MR whose commit cannot be resolved (no recorded SHA, repo
        // path has no such branch on disk).
        seed_merged_mr(&state, "ws-try", "mr-nosha", MrStatus::Merged, None).await;
        let (status, json) = try_it(&app(state.clone()), "mr-nosha", &member_key).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {json}");

        for mr in ["mr-open", "mr-nosha"] {
            let envs = state
                .preview_environments
                .find_by_mr(&Id::new(mr))
                .await
                .unwrap();
            assert!(envs.is_empty(), "{mr}: no preview row may exist");
        }
        assert_eq!(count_preview_events(&state, "preview.started").await, 0);
    }

    /// Member happy path: durable row carrying the resolved commit SHA,
    /// mesh peer registered, status exposed, analytics recorded, teardown
    /// removes the peer.
    #[tokio::test]
    async fn try_it_member_provisions_durable_environment() {
        let state = test_state();
        let commit = sha();
        seed_merged_mr(&state, "ws-try", "mr-ok", MrStatus::Merged, Some(&commit)).await;
        let member_key = seed_user(&state, "ws-try", "u-member-b", true).await;

        let (status, json) = try_it(&app(state.clone()), "mr-ok", &member_key).await;
        assert_eq!(status, StatusCode::CREATED, "body: {json}");
        assert_eq!(
            json["commit_sha"], commit,
            "row carries the resolved SHA: {json}"
        );
        assert_eq!(json["mr_id"], "mr-ok");
        assert!(json["mesh_ip"].is_string() || json["mesh_ip"].is_null());
        let env_status = json["status"].as_str().unwrap();
        assert!(
            env_status == "active" || env_status == "failed",
            "status must be exposed honestly: {json}"
        );

        // Durable row is queryable directly.
        let envs = state
            .preview_environments
            .find_by_mr(&Id::new("mr-ok"))
            .await
            .unwrap();
        assert_eq!(envs.len(), 1);
        let env = &envs[0];
        assert_eq!(env.commit_sha, commit);

        // Mesh peer registered for the environment's agent.
        let peer = state
            .network_peers
            .find_by_agent(&env.agent_id)
            .await
            .unwrap();
        assert!(
            peer.is_some(),
            "a mesh peer must be registered for the environment"
        );
        let peer = peer.unwrap();
        assert_eq!(peer.mesh_ip, env.mesh_ip);
        assert_eq!(
            env.peer_id.as_ref().map(|id| id.as_str()),
            Some(peer.id.as_str())
        );

        // Trial analytics recorded with user/workspace/repo/mr scope.
        assert_eq!(count_preview_events(&state, "preview.started").await, 1);

        // DELETE teardown: terminates and removes the peer.
        let resp = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/preview-environments/{}", env.id))
                    .header("authorization", format!("Bearer {member_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let envs = state
            .preview_environments
            .find_by_mr(&Id::new("mr-ok"))
            .await
            .unwrap();
        let env = &envs[0];
        assert_eq!(env.status, gyre_domain::PreviewEnvironmentStatus::Expired);
        assert_eq!(env.end_reason.as_deref(), Some("torn-down"));
        let peer = state
            .network_peers
            .find_by_agent(&env.agent_id)
            .await
            .unwrap();
        assert!(peer.is_none(), "teardown must remove the mesh peer");
        assert_eq!(count_preview_events(&state, "preview.ended").await, 1);
    }

    /// GET surfaces: MR-scoped list and per-environment status enforce the
    /// same membership boundary.
    #[tokio::test]
    async fn try_it_get_surfaces_enforce_membership() {
        let state = test_state();
        seed_merged_mr(&state, "ws-try", "mr-g", MrStatus::Merged, Some(&sha())).await;
        let member_key = seed_user(&state, "ws-try", "u-member-c", true).await;
        let outsider_key = seed_user(&state, "ws-try", "u-outsider-c", false).await;

        let (status, _json) = try_it(&app(state.clone()), "mr-g", &member_key).await;
        assert_eq!(status, StatusCode::CREATED);

        // MR-scoped list: member 200, outsider 403.
        for (token, expect) in [
            (&member_key, StatusCode::OK),
            (&outsider_key, StatusCode::FORBIDDEN),
        ] {
            let resp = app(state.clone())
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/merge-requests/mr-g/try-it")
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), expect);
        }

        // Collection + status + teardown: outsider 403 on all of them.
        let envs = state
            .preview_environments
            .find_by_mr(&Id::new("mr-g"))
            .await
            .unwrap();
        let env_id = envs[0].id.to_string();
        for uri in [
            format!("/api/v1/preview-environments?workspace_id=ws-try"),
            format!("/api/v1/preview-environments/{env_id}"),
        ] {
            let resp = app(state.clone())
                .oneshot(
                    Request::builder()
                        .uri(&uri)
                        .header("authorization", format!("Bearer {outsider_key}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN, "uri: {uri}");
        }
        let resp = app(state.clone())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/preview-environments/{env_id}"))
                    .header("authorization", format!("Bearer {outsider_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        // The outsider's teardown attempts changed nothing.
        let envs = state
            .preview_environments
            .find_by_mr(&Id::new("mr-g"))
            .await
            .unwrap();
        assert!(envs[0].status.is_live());
    }

    fn sha() -> String {
        "0123456789abcdef0123456789abcdef01234567".to_string()
    }

    async fn count_preview_events(state: &std::sync::Arc<crate::AppState>, name: &str) -> usize {
        let filter = gyre_ports::AnalyticsQueryFilter {
            event_name: Some(name.to_string()),
            ..Default::default()
        };
        let events = state.analytics.query_filtered(&filter).await.unwrap();
        events.len()
    }
}
