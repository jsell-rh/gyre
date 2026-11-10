//! System prompts (platform-model.md §4.3 "MCP Prompts (Injected at Agent
//! Startup)").
//!
//! Single source of truth for the four spec-required prompts. The MCP server
//! exposes them via `prompts/list` / `prompts/get`, and the spawn handler
//! injects the same content into the agent's container environment as
//! `GYRE_PROMPT_*` variables so they are part of the agent's initial
//! context — the agent does not need to fetch them.

use gyre_common::Id;
use gyre_domain::{Persona, PersonaApprovalStatus, PersonaScope, Repository, Task};

use crate::AppState;

/// The agent's persona prompt.
pub const PROMPT_PERSONA: &str = "system://persona";
/// Interaction norms, Ralph loop definition, escalation rules.
pub const PROMPT_PROTOCOL: &str = "system://protocol";
/// Current task, spec refs, acceptance criteria, worktree info.
pub const PROMPT_CONTEXT: &str = "system://context";
/// Budget limits, lifetime, scope boundaries.
pub const PROMPT_CONSTRAINTS: &str = "system://constraints";

/// All four system prompts in spec-table order (platform-model.md §4.3).
pub const SYSTEM_PROMPTS: [&str; 4] = [
    PROMPT_PERSONA,
    PROMPT_PROTOCOL,
    PROMPT_CONTEXT,
    PROMPT_CONSTRAINTS,
];

/// The scope a system prompt resolves against: the caller's position in the
/// ownership hierarchy (platform-model.md §1), its current task, and its
/// agent identity.
///
/// `tenant_id` is `None` only on the spawn path when the governing workspace
/// record itself is unresolvable — the tenant is never fabricated, the
/// tenant-scope cascade level is simply skipped there.
#[derive(Debug, Clone)]
pub struct PromptScope {
    pub tenant_id: Option<Id>,
    pub workspace_id: Id,
    pub repo_id: Option<Id>,
    pub task_id: Option<Id>,
    pub agent_id: Option<Id>,
}

/// Prompt resolution result: the error is a caller-facing message.
pub type PromptResult<T> = Result<T, String>;

/// Resolve a persona slug through the nearest-wins scope chain
/// (Repo > Workspace > Tenant) — platform-model.md §2 Scope Resolution.
///
/// Only `Approved` personas resolve: a system prompt injects behavioral
/// definitions, and a Pending persona is a human-unapproved one (persona
/// approval is human-only by spec). Fail closed rather than inject
/// unapproved behavior.
pub async fn resolve_persona_in_chain(
    state: &AppState,
    slug: &str,
    scope: &PromptScope,
) -> PromptResult<Persona> {
    let mut scopes: Vec<PersonaScope> = Vec::new();
    if let Some(repo_id) = &scope.repo_id {
        scopes.push(PersonaScope::Repo(repo_id.clone()));
    }
    scopes.push(PersonaScope::Workspace(scope.workspace_id.clone()));
    if let Some(tenant_id) = &scope.tenant_id {
        scopes.push(PersonaScope::Tenant(tenant_id.clone()));
    }

    for level in &scopes {
        let found = state
            .personas
            .find_by_slug_and_scope(slug, level)
            .await
            .map_err(|e| format!("persona lookup failed: {e}"))?;
        if let Some(persona) = found {
            if persona.approval_status != PersonaApprovalStatus::Approved {
                return Err(format!(
                    "persona '{slug}' resolves at {level:?} scope but is {:?}; \
                     a human must approve it before it can style an agent",
                    persona.approval_status
                ));
            }
            return Ok(persona);
        }
    }
    Err(format!(
        "persona '{slug}' not found in scope chain (repo/workspace/tenant)"
    ))
}

/// `system://persona` — the agent's persona prompt.
///
/// Resolution: an explicit `slug` argument wins (nearest-scope-wins chain,
/// repo → workspace → tenant); without one, the persona bound to the agent
/// record (`Agent.persona_id`) is used. Fails when neither resolves — an
/// agent with no bound persona has no persona prompt, which is reported
/// rather than papered over with generic text.
pub async fn persona_prompt(
    state: &AppState,
    scope: &PromptScope,
    slug: Option<&str>,
) -> PromptResult<String> {
    let persona = match slug.filter(|s| !s.is_empty()) {
        Some(slug) => resolve_persona_in_chain(state, slug, scope).await?,
        None => {
            let agent_id = scope.agent_id.as_ref().ok_or_else(|| {
                "no persona slug given and no agent identity to resolve a bound persona from"
                    .to_string()
            })?;
            let agent = state
                .agents
                .find_by_id(agent_id)
                .await
                .map_err(|e| format!("agent lookup failed: {e}"))?
                .ok_or_else(|| format!("agent '{agent_id}' not found"))?;
            let persona_id = agent.persona_id.clone().ok_or_else(|| {
                format!("agent '{agent_id}' has no persona bound; pass a slug to select one")
            })?;
            let persona = state
                .personas
                .find_by_id(&persona_id)
                .await
                .map_err(|e| format!("persona lookup failed: {e}"))?
                .ok_or_else(|| {
                    format!("persona '{persona_id}' bound to agent '{agent_id}' not found")
                })?;
            if persona.approval_status != PersonaApprovalStatus::Approved {
                return Err(format!(
                    "persona '{}' bound to agent '{agent_id}' is {:?}; \
                     a human must approve it before it can style an agent",
                    persona.slug, persona.approval_status
                ));
            }
            persona
        }
    };
    Ok(persona.system_prompt)
}

/// `system://protocol` — interaction norms, Ralph loop definition,
/// escalation rules (platform-model.md §3/§4). Static platform norms,
/// identical for every agent: a built-in template.
pub fn protocol_prompt() -> &'static str {
    r#"# Gyre Agent Protocol

## Interaction norms
- All interaction with the Gyre platform goes through the MCP server (tools, resources, prompts). Do not call platform REST endpoints directly.
- The server validates your token's scope (repo/workspace) on every call; stay inside it.
- Signal liveness with `gyre_agent_heartbeat` while you work.
- When your task is done, commit to your branch, push, and signal completion with `gyre_agent_complete` — this creates a merge request and moves your task to review. Never merge your own work: merging is decided by the merge queue and its gates.

## Ralph loop
Your work runs inside the repo orchestrator's loop — observe, plan, dispatch, monitor, reconcile:
1. **Observe** — read the current state: your task (`system://context`), the spec it implements, the code it touches.
2. **Plan** — decide the smallest real change that moves the task toward its acceptance criteria.
3. **Act** — implement, run the repo's quality gates, commit.
4. **Monitor** — verify the result against the acceptance criteria and the spec.
5. **Reconcile** — repeat until the task is genuinely done, then signal completion.

A task is done when its acceptance criteria hold — not when the code compiles.

## Escalation rules
- Blocked on another repo, a missing spec, or an ambiguity you cannot resolve? Escalate to your orchestrator via `gyre_message_send` — never work around a spec.
- Cross-repo needs are routed by the workspace orchestrator; cross-repo spec changes always require human approval.
- A spec you disagree with is a design decision: propose a change through the spec lifecycle, never silently implement around it.
- Budget or scope exhaustion: stop and escalate. `system://constraints` defines your limits."#
}

/// Worktree info rendered into the context prompt.
pub struct WorktreeInfo {
    pub branch: String,
    pub path: String,
}

/// Render the context prompt from resolved entities (shared by the
/// `prompts/get` and spawn-injection paths so both serve identical content).
fn format_context_prompt(
    task: Option<&Task>,
    repo: Option<&Repository>,
    worktree: Option<&WorktreeInfo>,
) -> String {
    let mut out = String::from("# Agent Context\n");

    match task {
        Some(t) => {
            out.push_str("\n## Current Task\n");
            out.push_str(&format!("- ID: {}\n", t.id));
            out.push_str(&format!("- Title: {}\n", t.title));
            out.push_str(&format!("- Status: {:?}\n", t.status));
            out.push_str(&format!("- Priority: {:?}\n", t.priority));
            if let Some(task_type) = &t.task_type {
                out.push_str(&format!("- Type: {task_type:?}\n"));
            }
            if let Some(spec_path) = &t.spec_path {
                out.push_str(&format!("- Spec ref: {spec_path}\n"));
            }
            if !t.labels.is_empty() {
                out.push_str(&format!("- Labels: {}\n", t.labels.join(", ")));
            }
            if !t.depends_on.is_empty() {
                let deps: Vec<String> = t.depends_on.iter().map(|d| d.to_string()).collect();
                out.push_str(&format!("- Depends on: {}\n", deps.join(", ")));
            }
            if let Some(description) = &t.description {
                out.push_str("\n### Description / Acceptance Criteria\n");
                out.push_str(description);
                out.push('\n');
            }
        }
        None => out.push_str("\n## Current Task\n\nNo task is bound to this agent.\n"),
    }

    match worktree {
        Some(wt) => {
            out.push_str("\n## Worktree\n");
            out.push_str(&format!("- Branch: {}\n", wt.branch));
            out.push_str(&format!("- Path: {}\n", wt.path));
        }
        None => out.push_str("\n## Worktree\n\nNo worktree is bound to this agent.\n"),
    }

    if let Some(r) = repo {
        out.push_str("\n## Repository\n");
        out.push_str(&format!("- Name: {}\n", r.name));
        out.push_str(&format!("- Path: {}\n", r.path));
        out.push_str(&format!("- Default branch: {}\n", r.default_branch));
    }

    out
}

/// `system://context` for an MCP caller: resolves the task (explicit
/// `task_id` argument wins, then the task bound to the caller's identity),
/// the caller's worktree, and the governing repo, then renders the prompt.
///
/// Containment (repo invariant): the loaded task must live inside the
/// caller's scope — workspace mismatch always denies, and a repo mismatch
/// denies when both the task and the caller carry a repo — mirroring
/// `handle_spawn_worker`'s PERMISSION_DENIED check in mcp.rs. Without it,
/// an explicit `task_id` argument would disclose any task's title,
/// description, and acceptance criteria across workspace/tenant boundaries.
pub async fn context_prompt_for_caller(
    state: &AppState,
    scope: &PromptScope,
    task_id: Option<&str>,
) -> PromptResult<String> {
    let task_id = match task_id.filter(|t| !t.is_empty()) {
        Some(tid) => Some(Id::new(tid)),
        None => scope.task_id.clone(),
    };
    let task = match &task_id {
        Some(tid) => {
            let t = state
                .tasks
                .find_by_id(tid)
                .await
                .map_err(|e| format!("task lookup failed: {e}"))?;
            let t = t.ok_or_else(|| format!("task '{tid}' not found"))?;
            if t.workspace_id != scope.workspace_id {
                return Err(format!("task '{tid}' belongs to a different workspace"));
            }
            if let Some(caller_repo) = &scope.repo_id {
                if !t.repo_id.to_string().is_empty() && t.repo_id != *caller_repo {
                    return Err(format!("task '{tid}' belongs to a different repo"));
                }
            }
            Some(t)
        }
        None => None,
    };

    // Worktree: the calling agent's worktree (branch + path).
    let mut worktree: Option<WorktreeInfo> = None;
    if let Some(agent_id) = &scope.agent_id {
        let worktrees = state
            .worktrees
            .find_by_agent(agent_id)
            .await
            .map_err(|e| format!("worktree lookup failed: {e}"))?;
        worktree = worktrees.first().map(|wt| WorktreeInfo {
            branch: wt.branch.clone(),
            path: wt.path.clone(),
        });
    }

    // Repo: the scope's repo, else the task's repo.
    let repo_id = scope
        .repo_id
        .clone()
        .or_else(|| task.as_ref().map(|t| t.repo_id.clone()));
    let repo = match repo_id {
        Some(rid) if !rid.to_string().is_empty() => {
            let r = state
                .repos
                .find_by_id(&rid)
                .await
                .map_err(|e| format!("repo lookup failed: {e}"))?;
            r
        }
        _ => None,
    };

    Ok(format_context_prompt(
        task.as_ref(),
        repo.as_ref(),
        worktree.as_ref(),
    ))
}

/// `system://context` for a spawn: the task being assigned, the repo it
/// lives in, and the worktree the spawn just created. Interrogation agents
/// have no worktree — pass `None`.
pub fn context_prompt_for_spawn(
    task: &Task,
    repo: &Repository,
    worktree: Option<(&str, &str)>,
) -> String {
    format_context_prompt(
        Some(task),
        Some(repo),
        worktree
            .map(|(branch, path)| WorktreeInfo {
                branch: branch.to_string(),
                path: path.to_string(),
            })
            .as_ref(),
    )
}

/// `system://context` for an orchestrator launch: orchestrators are not
/// task-bound and have no worktree — the prompt records that honestly plus
/// the repo (repo tier) and its default branch.
pub fn context_prompt_for_orchestrator(repo: Option<&Repository>) -> String {
    format_context_prompt(None, repo, None)
}

/// `system://constraints` — budget limits, remaining budget, agent lifetime
/// cap, and scope boundaries (platform-model.md §5).
///
/// Budget limits come from the same store spawn-time enforcement reads
/// (`budget_configs` / `budget_usages`, keyed `workspace:{id}` /
/// `tenant:global`), so the prompt never disagrees with what is enforced.
pub async fn constraints_prompt(state: &AppState, scope: &PromptScope) -> String {
    let ws_key = format!("workspace:{}", scope.workspace_id);
    let ws_config = state
        .budget_configs
        .get_config(&ws_key)
        .await
        .ok()
        .flatten();
    let ws_usage = state.budget_usages.get_usage(&ws_key).await.ok().flatten();
    let tenant_config = state
        .budget_configs
        .get_config("tenant:global")
        .await
        .ok()
        .flatten();

    let mut out = String::from("# Agent Constraints\n\n## Scope Boundaries\n");
    if let Some(tenant_id) = &scope.tenant_id {
        out.push_str(&format!("- Tenant: {tenant_id}\n"));
    }
    out.push_str(&format!("- Workspace: {}\n", scope.workspace_id));
    if let Some(repo_id) = &scope.repo_id {
        out.push_str(&format!("- Repo: {repo_id}\n"));
    }
    out.push_str(
        "\nYour agent token is scoped to the entities above. Work only inside your \
         repo's worktree; cross-repo or cross-workspace needs must be escalated to \
         your orchestrator, never fetched directly.\n",
    );

    out.push_str("\n## Budget Limits (workspace)\n");
    match (&ws_config, &ws_usage) {
        (Some(config), usage) => {
            let used = |u: Option<u64>| u.unwrap_or(0);
            let usage = usage.as_ref();
            let tokens = usage.map(|u| u.tokens_used_today);
            let cost = usage.map(|u| u.cost_today);
            let active = usage.map(|u| u.active_agents);
            if let Some(max) = config.max_tokens_per_day {
                let u = used(tokens);
                out.push_str(&format!(
                    "- max_tokens_per_day: {max} (used today: {u}, remaining: {})\n",
                    max.saturating_sub(u)
                ));
            }
            if let Some(max) = config.max_cost_per_day {
                let u = cost.unwrap_or(0.0);
                out.push_str(&format!(
                    "- max_cost_per_day_usd: {max:.4} (spent today: ${u:.4}, remaining: ${:.4})\n",
                    (max - u).max(0.0)
                ));
            }
            if let Some(max) = config.max_concurrent_agents {
                out.push_str(&format!(
                    "- max_concurrent_agents: {max} ({} active)\n",
                    active.unwrap_or(0)
                ));
            }
            if let Some(secs) = config.max_agent_lifetime_secs {
                out.push_str(&format!("- max_agent_lifetime_secs: {secs}\n"));
            }
            if config.max_tokens_per_day.is_none()
                && config.max_cost_per_day.is_none()
                && config.max_concurrent_agents.is_none()
                && config.max_agent_lifetime_secs.is_none()
            {
                out.push_str("\nNo budget limits are configured for this workspace.\n");
            }
        }
        (None, _) => out.push_str("\nNo budget limits are configured for this workspace.\n"),
    }

    if let Some(tenant_config) = &tenant_config {
        let has_any = tenant_config.max_tokens_per_day.is_some()
            || tenant_config.max_cost_per_day.is_some()
            || tenant_config.max_concurrent_agents.is_some()
            || tenant_config.max_agent_lifetime_secs.is_some();
        if has_any {
            out.push_str("\n## Budget Limits (tenant)\n");
            if let Some(max) = tenant_config.max_tokens_per_day {
                out.push_str(&format!("- max_tokens_per_day: {max}\n"));
            }
            if let Some(max) = tenant_config.max_cost_per_day {
                out.push_str(&format!("- max_cost_per_day_usd: {max:.4}\n"));
            }
            if let Some(max) = tenant_config.max_concurrent_agents {
                out.push_str(&format!("- max_concurrent_agents: {max}\n"));
            }
            if let Some(secs) = tenant_config.max_agent_lifetime_secs {
                out.push_str(&format!("- max_agent_lifetime_secs: {secs}\n"));
            }
        }
    }

    if let Some(secs) = ws_config.as_ref().and_then(|c| c.max_agent_lifetime_secs) {
        out.push_str(&format!(
            "\n## Lifetime\n\nYour agent lifetime is capped at {secs} seconds; \
             the platform reaps agents that exceed it. Finish and signal completion \
             (`gyre_agent_complete`) before the cap.\n"
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompts_are_the_four_spec_names_in_order() {
        assert_eq!(
            SYSTEM_PROMPTS,
            [
                "system://persona",
                "system://protocol",
                "system://context",
                "system://constraints"
            ]
        );
    }

    #[test]
    fn protocol_prompt_covers_norms_loop_and_escalation() {
        let p = protocol_prompt();
        assert!(p.contains("Ralph loop"));
        assert!(p.contains("Escalation rules"));
        assert!(p.contains("gyre_agent_complete"));
        assert!(p.contains("Interaction norms"));
    }
}
