//! Meta-spec prompt assembly (agent-runtime.md §2).
//!
//! Builds the ordered prompt set injected into every spawned agent:
//!
//! 1. All REQUIRED tenant (Global) meta-specs — ordered by kind
//!    (persona → principle → standard → process)
//! 2. All REQUIRED workspace meta-specs (same kind ordering)
//! 3. Spec-level bindings at their pinned versions
//!
//! Deduplication: a meta-spec that is both required and bound to a spec is
//! included only once, in the required section (the binding is redundant).
//!
//! The assembled set is also the attestation source: `meta_specs_used` records
//! the exact id/kind/content_hash/version/scope of every injected meta-spec,
//! and `set_sha` hashes the canonical serialization for provenance.

use std::collections::HashSet;
use std::sync::Arc;

use gyre_common::Id;
use gyre_domain::meta_spec::{MetaSpec, MetaSpecKind, MetaSpecScope, MetaSpecVersion};
use gyre_domain::{MetaSpecUsed, Task};
use sha2::{Digest, Sha256};

use crate::AppState;

/// One assembled prompt section.
#[derive(Clone, Debug)]
pub struct PromptSection {
    /// Meta-spec name (e.g. "conventional-commits").
    pub name: String,
    /// Kind string, e.g. "meta:persona".
    pub kind: String,
    /// The prompt text injected into agent context.
    pub prompt: String,
    /// "global" (tenant scope) or "workspace".
    pub scope: String,
    /// Which injection band the section came from: "required-tenant",
    /// "required-workspace", or "binding".
    pub source: &'static str,
    /// Attestation record for the merge bundle.
    pub used: MetaSpecUsed,
}

/// The full assembled prompt set for one agent spawn.
#[derive(Clone, Debug)]
pub struct PromptSet {
    pub sections: Vec<PromptSection>,
    /// SHA-256 hex of the canonical section serialization (provenance).
    pub set_sha: String,
}

/// Injection order rank for a kind: persona → principle → standard → process.
fn kind_rank(kind: &MetaSpecKind) -> u8 {
    match kind {
        MetaSpecKind::Persona => 0,
        MetaSpecKind::Principle => 1,
        MetaSpecKind::Standard => 2,
        MetaSpecKind::Process => 3,
    }
}

fn scope_str(scope: &MetaSpecScope) -> &'static str {
    match scope {
        MetaSpecScope::Global => "global",
        MetaSpecScope::Workspace => "workspace",
    }
}

/// Resolve the prompt text for a meta-spec at a specific version.
///
/// For the current version this is `meta_spec.prompt`; for an older pinned
/// version it is looked up in the immutable version history. Returns `None`
/// when the pinned version does not exist (stale/deleted history) — the
/// caller skips that binding rather than injecting the wrong version.
async fn prompt_at_version(
    state: &AppState,
    ms: &MetaSpec,
    version: u32,
) -> Option<(String, String)> {
    if version == ms.version {
        return Some((ms.prompt.clone(), ms.content_hash.clone()));
    }
    let ver: MetaSpecVersion = state
        .meta_specs
        .get_version(&ms.id, version)
        .await
        .ok()
        .flatten()?;
    Some((ver.prompt, ver.content_hash))
}

fn sort_by_kind(specs: &mut Vec<MetaSpec>) {
    specs.sort_by(|a, b| {
        kind_rank(&a.kind)
            .cmp(&kind_rank(&b.kind))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });
}

/// Assemble the prompt set for an agent spawned in the task's workspace,
/// resolving spec-level bindings from `task.spec_path`.
///
/// When the task carries no spec_path, no bindings resolve (only required
/// sections inject).
pub async fn assemble_prompt_set(state: &Arc<AppState>, task: &Task) -> PromptSet {
    let workspace_id = task.workspace_id.to_string();

    // Band 1: required tenant (Global) meta-specs.
    let tenant = state
        .meta_specs
        .list(&gyre_ports::MetaSpecFilter {
            scope: Some(MetaSpecScope::Global),
            required: Some(true),
            ..Default::default()
        })
        .await
        .unwrap_or_default();
    // Band 2: required meta-specs scoped to this workspace.
    let workspace = state
        .meta_specs
        .list(&gyre_ports::MetaSpecFilter {
            scope: Some(MetaSpecScope::Workspace),
            scope_id: Some(workspace_id),
            required: Some(true),
            ..Default::default()
        })
        .await
        .unwrap_or_default();

    let mut tenant = tenant;
    let mut workspace = workspace;
    // MUTATION-REVIEW: ordering disabled
    // sort_by_kind(&mut tenant);
    // sort_by_kind(&mut workspace);

    let mut sections: Vec<PromptSection> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    let mut push_required = |ms: MetaSpec, source: &'static str| {
        if seen_ids.insert(ms.id.to_string()) {
            sections.push(PromptSection {
                name: ms.name.clone(),
                kind: ms.kind.as_str().to_string(),
                prompt: ms.prompt.clone(),
                scope: scope_str(&ms.scope).to_string(),
                source,
                used: MetaSpecUsed {
                    id: ms.id.clone(),
                    kind: ms.kind.as_str().to_string(),
                    content_hash: ms.content_hash.clone(),
                    version: ms.version,
                    required: true,
                    scope: scope_str(&ms.scope).to_string(),
                },
            });
        }
    };

    for ms in tenant {
        push_required(ms, "required-tenant");
    }
    for ms in workspace {
        push_required(ms, "required-workspace");
    }

    // Band 3: spec-level bindings at pinned versions. A binding that
    // references an already-injected (required) meta-spec is skipped —
    // dedup by meta_spec_id per the spec.
    if let Some(spec_path) = task.spec_path.as_deref() {
        let bindings = state
            .meta_spec_bindings
            .list_by_spec_id(spec_path)
            .await
            .unwrap_or_default();
        for binding in bindings {
            if seen_ids.contains(&binding.meta_spec_id.to_string()) {
                continue;
            }
            let Some(ms) = state
                .meta_specs
                .get_by_id(&binding.meta_spec_id)
                .await
                .ok()
                .flatten()
            else {
                continue;
            };
            let Some((prompt, content_hash)) =
                prompt_at_version(state, &ms, binding.pinned_version).await
            else {
                tracing::warn!(
                    meta_spec_id = %binding.meta_spec_id,
                    pinned_version = binding.pinned_version,
                    spec = %spec_path,
                    "skipping binding: pinned version not found"
                );
                continue;
            };
            if !seen_ids.insert(ms.id.to_string()) {
                continue;
            }
            sections.push(PromptSection {
                name: ms.name.clone(),
                kind: ms.kind.as_str().to_string(),
                prompt,
                scope: scope_str(&ms.scope).to_string(),
                source: "binding",
                used: MetaSpecUsed {
                    id: ms.id.clone(),
                    kind: ms.kind.as_str().to_string(),
                    content_hash,
                    version: binding.pinned_version,
                    required: false,
                    scope: scope_str(&ms.scope).to_string(),
                },
            });
        }
    }

    let set_sha = prompt_set_sha(&sections);
    PromptSet { sections, set_sha }
}

/// Canonical serialization hashed for provenance: one line per section,
/// `kind\tname\tscope\tversion\tcontent_hash`. Order is significant — the
/// injection order is part of the prompt configuration being attested.
fn prompt_set_sha(sections: &[PromptSection]) -> String {
    let mut hasher = Sha256::new();
    for s in sections {
        hasher.update(s.kind.as_bytes());
        hasher.update(b"\t");
        hasher.update(s.name.as_bytes());
        hasher.update(b"\t");
        hasher.update(s.scope.as_bytes());
        hasher.update(b"\t");
        hasher.update(s.used.version.to_string().as_bytes());
        hasher.update(b"\t");
        hasher.update(s.used.content_hash.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Render the assembled sections as the system-prompt text block injected
/// into the agent environment (GYRE_META_SPEC_PROMPT).
pub fn render_system_prompt(sections: &[PromptSection]) -> String {
    let mut out = String::new();
    for s in sections {
        out.push_str(&format!(
            "# {} — {} (scope: {}, v{})\n{}\n\n",
            s.kind, s.name, s.scope, s.used.version, s.prompt
        ));
    }
    out
}

/// Stored prompt-set record (kv namespace "agent_meta_spec_sets"), keyed by
/// agent id. The merge processor reads this at merge time to populate
/// `MergeAttestation.meta_specs_used`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentPromptSetRecord {
    pub agent_id: Id,
    pub task_id: Id,
    pub spec_path: Option<String>,
    pub set_sha: String,
    pub assembled_at: u64,
    pub meta_specs_used: Vec<MetaSpecUsed>,
}

pub const PROMPT_SET_NAMESPACE: &str = "agent_meta_spec_sets";

/// Persist the assembled prompt set for an agent so the merge attestation
/// can later record exactly which meta-specs the agent ran under.
pub async fn store_prompt_set_record(state: &AppState, record: &AgentPromptSetRecord) {
    match serde_json::to_string(record) {
        Ok(json) => {
            if let Err(e) = state
                .kv_store
                .kv_set(PROMPT_SET_NAMESPACE, record.agent_id.as_str(), json)
                .await
            {
                tracing::warn!(
                    agent_id = %record.agent_id,
                    "failed to store meta-spec prompt set record: {e}"
                );
            }
        }
        Err(e) => {
            tracing::warn!(agent_id = %record.agent_id, "failed to serialize prompt set record: {e}")
        }
    }
}

/// Load the stored prompt-set record for an agent (merge-time attestation input).
pub async fn load_prompt_set_record(
    state: &AppState,
    agent_id: &str,
) -> Option<AgentPromptSetRecord> {
    let json = state
        .kv_store
        .kv_get(PROMPT_SET_NAMESPACE, agent_id)
        .await
        .ok()
        .flatten()?;
    serde_json::from_str(&json).ok()
}

// ---------------------------------------------------------------------------
// Stale pin detection (agent-runtime.md §2 Stale Pin Detection)
// ---------------------------------------------------------------------------

/// Run one pass of stale-pin detection: for every spec-level binding, compare
/// its `pinned_version` against the meta-spec's current `version`. On mismatch,
/// create a priority-6 `MetaSpecDrift` notification for the workspace's
/// Admin/Owner members ("Meta-spec drift alert": review and update the pin).
///
/// Idempotent per (spec, meta-spec): a dedup key in the kv store gates repeat
/// notifications, cleared when the pin is updated to the current version.
pub async fn detect_stale_pins(state: &Arc<AppState>) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let bindings = state.meta_spec_bindings.list_all().await?;
    if bindings.is_empty() {
        return Ok(());
    }

    let mut stale_count = 0u32;
    for binding in bindings {
        let Some(ms) = state
            .meta_specs
            .get_by_id(&binding.meta_spec_id)
            .await?
        else {
            // Referenced meta-spec deleted — the delete guard blocks this for
            // new deletes, but historical rows may exist. Nothing to compare.
            continue;
        };
        if binding.pinned_version >= ms.version {
            continue; // current (or ahead — future-proof against version resets)
        }

        let dedup_key = format!("stale_pin:{}:{}", binding.spec_id, ms.id.as_str());
        let already_notified = state
            .kv_store
            .kv_get("meta_spec_stale_pins", &dedup_key)
            .await?
            .is_some();
        if already_notified {
            continue;
        }

        notify_stale_pin(state, &binding, &ms, now).await;
        state
            .kv_store
            .kv_set("meta_spec_stale_pins", &dedup_key, now.to_string())
            .await?;
        stale_count += 1;
    }

    if stale_count > 0 {
        tracing::info!(
            stale_pins = stale_count,
            "stale pin detection: created drift notifications"
        );
    }
    Ok(())
}

/// Create the "Meta-spec drift alert" notification for the workspace that owns
/// the bound spec (resolved via the spec ledger), addressed to Admin/Owner
/// members. Falls back to the meta-spec's own workspace when the spec ledger
/// has no workspace recorded.
async fn notify_stale_pin(
    state: &Arc<AppState>,
    binding: &gyre_domain::MetaSpecBinding,
    ms: &MetaSpec,
    now: u64,
) {
    // Resolve the workspace that owns the bound spec.
    let ledger_ws = state
        .spec_ledger
        .find_by_path(&binding.spec_id)
        .await
        .ok()
        .flatten()
        .and_then(|e| e.workspace_id);
    let workspace_id = match ledger_ws {
        Some(ws) => Id::new(ws),
        None => match &ms.scope_id {
            // Workspace-scoped meta-spec: notify its own workspace.
            Some(ws) => Id::new(ws.clone()),
            // Global meta-spec bound by a spec with no ledger workspace —
            // no addressable workspace, skip (logged).
            None => {
                tracing::warn!(
                    spec = %binding.spec_id,
                    meta_spec = %ms.name,
                    "stale pin: cannot resolve workspace for notification"
                );
                return;
            }
        },
    };

    // Tenant for the notification record.
    let tenant_id = match state.workspaces.find_by_id(&workspace_id).await {
        Ok(Some(ws)) => ws.tenant_id.to_string(),
        _ => return,
    };

    let members = match state
        .workspace_memberships
        .list_by_workspace(&workspace_id)
        .await
    {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("stale pin detection: failed to list members: {e}");
            return;
        }
    };

    for member in members {
        if !matches!(
            member.role,
            gyre_domain::WorkspaceRole::Admin | gyre_domain::WorkspaceRole::Owner
        ) {
            continue;
        }
        let mut notif = gyre_common::Notification::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            workspace_id.clone(),
            member.user_id.clone(),
            gyre_common::NotificationType::MetaSpecDrift,
            format!(
                "Meta-spec drift alert: '{}' uses {} v{}, but v{} is available. Review and update pin.",
                binding.spec_id, ms.name, binding.pinned_version, ms.version
            ),
            &tenant_id,
            now as i64,
        );
        notif.body = Some(
            serde_json::json!({
                "spec_id": binding.spec_id,
                "meta_spec_id": ms.id.as_str(),
                "meta_spec_name": ms.name,
                "kind": ms.kind.as_str(),
                "pinned_version": binding.pinned_version,
                "current_version": ms.version,
            })
            .to_string(),
        );
        notif.entity_ref = Some(binding.spec_id.clone());
        if let Err(e) = state.notifications.create(&notif).await {
            tracing::warn!(
                user = %member.user_id,
                "stale pin detection: failed to create notification: {e}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;
    use gyre_domain::meta_spec::{MetaSpecApprovalStatus, MetaSpecBinding};

    fn make_ms(
        id: &str,
        name: &str,
        kind: MetaSpecKind,
        required: bool,
        scope: MetaSpecScope,
        scope_id: Option<&str>,
    ) -> MetaSpec {
        MetaSpec {
            id: Id::new(id),
            kind,
            name: name.to_string(),
            scope,
            scope_id: scope_id.map(String::from),
            prompt: format!("prompt for {name}"),
            version: 1,
            content_hash: format!("hash-{name}"),
            required,
            approval_status: MetaSpecApprovalStatus::Approved,
            approved_by: Some("system".to_string()),
            approved_at: Some(1),
            created_by: "system".to_string(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn make_task(spec_path: Option<&str>) -> Task {
        let mut t = Task::new(Id::new("t1"), "T", 1);
        t.workspace_id = Id::new("ws-1");
        t.spec_path = spec_path.map(String::from);
        t
    }

    #[tokio::test]
    async fn injection_order_tenant_before_workspace_before_bindings() {
        let state = test_state();
        // Required tenant: standard + persona (deliberately unsorted insert order).
        state.meta_specs.create(&make_ms("ms-std", "test-coverage", MetaSpecKind::Standard, true, MetaSpecScope::Global, None)).await.unwrap();
        state.meta_specs.create(&make_ms("ms-per", "default-worker", MetaSpecKind::Persona, true, MetaSpecScope::Global, None)).await.unwrap();
        // Required workspace.
        state.meta_specs.create(&make_ms("ws-per", "team-persona", MetaSpecKind::Persona, true, MetaSpecScope::Workspace, Some("ws-1"))).await.unwrap();
        // Optional, bound.
        state.meta_specs.create(&make_ms("opt-sec", "security", MetaSpecKind::Persona, false, MetaSpecScope::Global, None)).await.unwrap();

        state
            .meta_spec_bindings
            .create(&MetaSpecBinding {
                id: Id::new("b1"),
                spec_id: "specs/foo.md".to_string(),
                meta_spec_id: Id::new("opt-sec"),
                pinned_version: 1,
                created_at: 1,
            })
            .await
            .unwrap();

        let set = assemble_prompt_set(&state, &make_task(Some("specs/foo.md"))).await;
        let names: Vec<&str> = set.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["default-worker", "test-coverage", "team-persona", "security"],
            "tenant (kind-ordered) → workspace → bindings"
        );
        assert_eq!(set.sections[0].source, "required-tenant");
        assert_eq!(set.sections[2].source, "required-workspace");
        assert_eq!(set.sections[3].source, "binding");
        assert_eq!(set.sections[3].used.version, 1);
    }

    #[tokio::test]
    async fn kind_ordering_within_band() {
        let state = test_state();
        // Insert in reverse kind order.
        state.meta_specs.create(&make_ms("p4", "proc", MetaSpecKind::Process, true, MetaSpecScope::Global, None)).await.unwrap();
        state.meta_specs.create(&make_ms("p3", "std", MetaSpecKind::Standard, true, MetaSpecScope::Global, None)).await.unwrap();
        state.meta_specs.create(&make_ms("p2", "principle", MetaSpecKind::Principle, true, MetaSpecScope::Global, None)).await.unwrap();
        state.meta_specs.create(&make_ms("p1", "persona", MetaSpecKind::Persona, true, MetaSpecScope::Global, None)).await.unwrap();

        let set = assemble_prompt_set(&state, &make_task(None)).await;
        let kinds: Vec<&str> = set.sections.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(kinds, vec!["meta:persona", "meta:principle", "meta:standard", "meta:process"]);
    }

    #[tokio::test]
    async fn required_meta_spec_in_bindings_deduped() {
        let state = test_state();
        state.meta_specs.create(&make_ms("r1", "req-persona", MetaSpecKind::Persona, true, MetaSpecScope::Global, None)).await.unwrap();
        state
            .meta_spec_bindings
            .create(&MetaSpecBinding {
                id: Id::new("b1"),
                spec_id: "specs/foo.md".to_string(),
                meta_spec_id: Id::new("r1"),
                pinned_version: 1,
                created_at: 1,
            })
            .await
            .unwrap();

        let set = assemble_prompt_set(&state, &make_task(Some("specs/foo.md"))).await;
        assert_eq!(set.sections.len(), 1, "required meta-spec bound by spec must appear once");
        assert_eq!(set.sections[0].source, "required-tenant");
    }

    #[tokio::test]
    async fn pinned_old_version_uses_historical_content() {
        let state = test_state();
        let mut ms = make_ms("v-ms", "sec", MetaSpecKind::Persona, false, MetaSpecScope::Global, None);
        state.meta_specs.create(&ms).await.unwrap();
        // Edit: v2.
        ms.prompt = "v2 prompt".to_string();
        ms.content_hash = "hash-v2".to_string();
        ms.version = 2;
        ms.updated_at = 2;
        state.meta_specs.update(&ms).await.unwrap();

        state
            .meta_spec_bindings
            .create(&MetaSpecBinding {
                id: Id::new("b1"),
                spec_id: "specs/foo.md".to_string(),
                meta_spec_id: Id::new("v-ms"),
                pinned_version: 1,
                created_at: 1,
            })
            .await
            .unwrap();

        let set = assemble_prompt_set(&state, &make_task(Some("specs/foo.md"))).await;
        assert_eq!(set.sections.len(), 1);
        assert_eq!(set.sections[0].prompt, "prompt for sec", "pinned v1 content, not current v2");
        assert_eq!(set.sections[0].used.version, 1);
        assert_eq!(set.sections[0].used.content_hash, "hash-sec");
    }

    #[tokio::test]
    async fn missing_pinned_version_skips_binding() {
        let state = test_state();
        state.meta_specs.create(&make_ms("v-ms", "sec", MetaSpecKind::Persona, false, MetaSpecScope::Global, None)).await.unwrap();
        state
            .meta_spec_bindings
            .create(&MetaSpecBinding {
                id: Id::new("b1"),
                spec_id: "specs/foo.md".to_string(),
                meta_spec_id: Id::new("v-ms"),
                pinned_version: 99, // never existed
                created_at: 1,
            })
            .await
            .unwrap();

        let set = assemble_prompt_set(&state, &make_task(Some("specs/foo.md"))).await;
        assert!(set.sections.is_empty(), "unresolvable pin must be skipped, not wrong-version injected");
    }

    #[tokio::test]
    async fn workspace_required_scoped_to_other_workspace_excluded() {
        let state = test_state();
        state.meta_specs.create(&make_ms("other-ws", "other-team", MetaSpecKind::Persona, true, MetaSpecScope::Workspace, Some("ws-OTHER"))).await.unwrap();

        let set = assemble_prompt_set(&state, &make_task(None)).await;
        assert!(set.sections.is_empty(), "required meta-spec of another workspace must not inject");
    }

    #[tokio::test]
    async fn set_sha_changes_with_content_and_order() {
        let s1 = PromptSection {
            name: "a".into(),
            kind: "meta:persona".into(),
            prompt: "x".into(),
            scope: "global".into(),
            source: "required-tenant",
            used: MetaSpecUsed { id: Id::new("1"), kind: "meta:persona".into(), content_hash: "h1".into(), version: 1, required: true, scope: "global".into() },
        };
        let mut s2 = s1.clone();
        s2.used.content_hash = "h2".into();
        assert_ne!(prompt_set_sha(&[s1.clone()]), prompt_set_sha(&[s2]), "hash must reflect content_hash");

        let mut s3 = s1.clone();
        s3.name = "b".into();
        assert_ne!(prompt_set_sha(&[s1.clone(), s3.clone()]), prompt_set_sha(&[s3, s1]), "hash must reflect order");
    }

    #[tokio::test]
    async fn record_round_trip() {
        let state = test_state();
        let rec = AgentPromptSetRecord {
            agent_id: Id::new("ag-1"),
            task_id: Id::new("t1"),
            spec_path: Some("specs/foo.md".to_string()),
            set_sha: "abc".to_string(),
            assembled_at: 42,
            meta_specs_used: vec![MetaSpecUsed { id: Id::new("1"), kind: "meta:persona".to_string(), content_hash: "h".to_string(), version: 3, required: true, scope: "global".to_string() }],
        };
        store_prompt_set_record(&state, &rec).await;
        let loaded = load_prompt_set_record(&state, "ag-1").await.expect("record must load");
        assert_eq!(loaded.set_sha, "abc");
        assert_eq!(loaded.meta_specs_used.len(), 1);
        assert_eq!(loaded.meta_specs_used[0].version, 3);
    }
}
