# Meta-Spec Reconciliation

> **Status: Draft.** This spec defines how Gyre safely iterates on the rules that govern agent behavior (personas, design principles, coding standards) without destructively invalidating downstream code.

## Problem

Gyre produces code governed by a stack of rules: specs define **what** to build, meta-specs define **how** to build it (personas, architectural principles, coding standards, process norms). Any change to any layer of this stack potentially invalidates downstream output.

Today, Gyre handles spec changes reactively (spec-lifecycle.md: auto-task creation, approval invalidation, drift detection). But for meta-specs -- the rules that govern agent behavior itself -- there is no tracking, no versioning, no impact analysis, and no safe way to iterate.

This means:
- A persona prompt change has unknown blast radius. Which repos were built under the old prompt? Which code is affected?
- Design principle changes (e.g., "prefer event sourcing over CRUD") have no mechanism to propagate. Code built under old principles stays as-is unless manually identified.
- There is no way to evaluate the effect of a meta-spec change without deploying it and hoping. You can't predict what an LLM will produce under a changed prompt without actually running it.
- After day one, the system calcifies: changing the rules is either reckless (no impact analysis) or impossible (too risky without one).

Without solving this, Gyre is a single-use system. The SDLC is only as good as its ability to evolve its own norms.

## Core Insight

You cannot statically analyze the effect of a prompt change. An agent reading existing code and "checking" it against a new persona is fundamentally different from an agent producing code under that persona. The only way to know what a meta-spec change produces is to run it.

Therefore: **meta-spec iteration is speculative re-implementation.** Fork the world, re-run affected work under new rules, compare the output, decide whether to adopt it.

## Definitions

| Term | Meaning |
|---|---|
| **Spec** | Declares **what** to build. Lives in the spec registry. Subject to approval. |
| **Meta-spec** | Declares **how** to build. Governs agent behavior: personas, design principles, coding standards, architectural norms, process rules. |
| **Meta-spec set** | The bound collection of meta-specs active in a given scope (workspace or repo). Pinned to specific versions. |
| **Provenance** | The record of which meta-spec versions were active when code was produced. |
| **Drift** | Code produced under a superseded meta-spec version. |
| **Reconciliation** | Re-implementing drifted code under the current meta-spec set. |
| **Blast radius** | The set of repos, specs, and code affected by a proposed meta-spec change. |
| **Preview** | A fast, ceremony-free agent run against a real spec under a draft meta-spec. Throwaway output for human evaluation. |

## Design

### 1. Meta-Specs as First-Class Versioned Artifacts

Everything that governs agent behavior is registered in the spec registry with a distinct kind:

| Kind | Examples | Approval mode |
|---|---|---|
| `meta:persona` | Backend developer, security reviewer, orchestrator | `human_only` (already enforced by platform-model.md) |
| `meta:principle` | DDD, event sourcing, hexagonal architecture | `human_only` |
| `meta:standard` | Coding standards, naming conventions, error handling patterns | `human_and_agent` |
| `meta:process` | Ralph loop definition, escalation rules, review norms | `human_only` |

Meta-specs get the same SHA-pinned versioning, approval flow, and link graph that regular specs already have. They appear in the spec registry alongside implementation specs.

The manifest declares them:

```yaml
specs:
  - path: meta/personas/backend-developer.md
    title: Backend Developer Persona
    kind: meta:persona
    owner: user:jsell
    approval:
      mode: human_only
    scope: workspace        # workspace | tenant | repo
    auto_create_tasks: false  # meta-spec changes don't create implementation tasks
    auto_reconcile: true      # meta-spec changes trigger reconciliation (see below)
```

### 2. Meta-Spec Sets: Workspace-Level Binding

A workspace has a **meta-spec set** -- the pinned collection of meta-specs that govern agent behavior in that workspace. This is the equivalent of a Kubernetes ConfigMap mounted into pods.

```yaml
# Workspace configuration (stored in forge database)
workspace: payments
meta_spec_set:
  personas:
    backend: meta/personas/backend-developer.md@a1b2c3
    security: meta/personas/security-reviewer.md@d4e5f6
    orchestrator: meta/personas/repo-orchestrator.md@789abc
  principles:
    - meta/principles/architecture.md@def012
    - meta/principles/ddd.md@345678
  standards:
    - meta/standards/coding.md@9abcde
    - meta/standards/error-handling.md@f01234
  process:
    - meta/process/ralph-loop.md@567890
    - meta/process/escalation.md@abcdef
```

**Scope resolution** (nearest-scope-wins, same as personas today):
1. Repo-level meta-spec set overrides workspace
2. Workspace-level meta-spec set overrides tenant
3. Tenant-level meta-spec set is the default

**Version pinning:** Meta-spec set entries reference specific SHAs, not "latest." Updating a meta-spec set entry to a new SHA is an explicit action -- the action that triggers reconciliation.

When an agent is spawned in a workspace, the MCP server injects the resolved meta-spec set into the agent's system prompt (extending the existing protocol injection from platform-model.md Section 8).

### 3. Extended Provenance

When an agent produces code, the commit provenance record (already tracked by M13.2) is extended to capture the active meta-spec set:

```rust
pub struct CommitProvenance {
    // Existing fields
    pub agent_id: Id,
    pub task_id: Id,
    pub spawned_by: String,
    pub model_context: Option<String>,

    // New: active meta-spec versions at commit time
    pub meta_spec_set: MetaSpecSnapshot,
}

pub struct MetaSpecSnapshot {
    pub persona: String,           // path@sha
    pub principles: Vec<String>,   // [path@sha, ...]
    pub standards: Vec<String>,
    pub process: Vec<String>,
}
```

This creates the traceable link: "this code was produced under persona v3 + architecture v7 + coding-standards v2." Without this, blast radius computation is impossible.

### 4. Blast Radius Computation

When a meta-spec is modified, the forge computes the blast radius by querying provenance records:

```
Meta-spec change: meta/personas/backend-developer.md  v3 (a1b2c3) -> v4 (x7y8z9)
  Reason: "Added event sourcing guidance for domain aggregates"

Blast radius:
  Tenant: Acme Corp
    Workspace: payments (bound to v3)
      payment-api:     12 specs, 847 commits under v3
      ledger-service:   8 specs, 312 commits under v3
      billing-gateway:  3 specs,  89 commits under v3
    Workspace: onboarding (bound to v3)
      signup-service:   5 specs, 201 commits under v3
      kyc-service:      4 specs, 156 commits under v3
    Workspace: internal-tools (bound to v2, not affected)
      -- skipped, already behind by one version --

  Total: 5 repos, 32 specs, 1605 commits
```

This is a database query over provenance records + workspace meta-spec set bindings. The blast radius is computed, not guessed.

### 5. Preview Mode: The Fast Iteration Loop

Before committing to a meta-spec change, humans need a fast way to see what it actually produces. Reports and static analysis are insufficient -- the only way to know what a changed persona produces is to run it against real code.

**Preview mode strips all ceremony from the Ralph loop.** Same agent, same spec, same repo -- but no gates, no MR, no merge queue, no provenance recording. Just: spawn an agent with a draft meta-spec, point it at a real spec in a real repo, see what it produces. Throwaway branch, garbage-collected after review.

#### The Preview Endpoint

```
POST /api/v1/meta-specs/preview
{
  "draft": {
    "kind": "meta:persona",
    "content": "<full draft persona text -- not yet committed>"
  },
  "targets": [
    { "repo_id": "<uuid>", "spec_path": "specs/system/search.md" },
    { "repo_id": "<uuid>", "spec_path": "specs/system/identity.md" }
  ]
}

// Response 202
{
  "preview_id": "<uuid>",
  "agents": [
    { "agent_id": "<uuid>", "repo_id": "<uuid>", "spec_path": "...", "branch": "preview/<preview_id>/search" },
    { "agent_id": "<uuid>", "repo_id": "<uuid>", "spec_path": "...", "branch": "preview/<preview_id>/identity" }
  ]
}
```

The draft doesn't need to be committed or approved. It's ephemeral -- the human is editing inline, hitting preview, seeing output. Multiple targets run in parallel.

#### What's Skipped in Preview Mode

| Ralph Loop Step | Preview Mode |
|---|---|
| Spec approval check | Skipped -- the spec is already approved; we're testing the meta-spec |
| Quality gates | Skipped -- this is a draft, not production |
| MR creation | Skipped -- no MR overhead |
| Merge queue | Skipped -- nothing to merge |
| Provenance recording | Skipped -- draft meta-spec has no SHA yet |
| Budget accounting | Separate preview budget (configurable, defaults to workspace budget) |
| Token revocation | Normal -- preview agent tokens are short-lived |

#### The Iteration Cycle

```
1. Human drafts a meta-spec change (UI editor or file)
2. Human selects 1-3 real specs from repos they own
3. "Preview" -> agents spawn with draft meta-spec, implement the specs
4. UI shows diff: existing code vs. new code produced under draft
5. Human reviews, adjusts the meta-spec
6. "Preview" again -> new agents, new output, new diff
7. Repeat until satisfied (typically 3-8 iterations)
8. "Publish" -> commit the meta-spec, trigger approval + reconciliation
```

Steps 2-6 are the tight loop. Each iteration is one agent run -- minutes, not hours. The total time to converge on a good meta-spec change is an afternoon, not a week.

#### Why Real Specs, Not Synthetic Tests

The targets must be real specs in real repos. Synthetic "golden specs" are mocks -- they test what the persona does in a controlled environment, not what it does against your actual complexity. The best test is the real thing.

The human picks specs they know well -- specs where they can judge whether the output is better, worse, or equivalent. They pick diverse specs: one simple CRUD service, one domain-heavy aggregate, one with tricky edge cases. But these are specs they own, in repos they understand.

#### Preview Cleanup

Preview branches (`preview/{preview_id}/*`) are ephemeral:
- Auto-deleted after 24 hours (configurable)
- Manually deletable via `DELETE /api/v1/meta-specs/preview/{preview_id}`
- No refs, no provenance, no audit trail (this is scratch work)
- Preview agents are killed on completion (no idle state)

#### UI Integration

The meta-spec editor in the UI combines:
- **Left panel:** inline editor for the meta-spec content (markdown with live preview)
- **Right panel:** target spec selector (browse repos you have access to, pick specs)
- **Bottom panel:** diff viewer showing existing code vs. preview output
- **Action bar:** "Preview" (run), "Clear" (delete preview branches), "Publish" (commit + approve flow)

The diff viewer updates as each preview agent completes. Multiple targets show as tabs.

### 6. Reconciliation: The Slow Rollout

After a meta-spec change is published and approved, reconciliation propagates the change across the blast radius. This is the thorough, budget-constrained, autonomous process -- distinct from the fast preview loop.

Reconciliation re-runs the Ralph loop with the same specs but new rules, on branches, without touching main.

#### Trigger

Reconciliation is triggered when a workspace's meta-spec set is updated to reference a new version of a meta-spec (typically after the preview loop has validated the change):

```
PUT /api/v1/workspaces/{id}/meta-spec-set
{
  "personas": {
    "backend": "meta/personas/backend-developer.md@x7y8z9"  // was @a1b2c3
  }
}
```

#### The Reconciliation Controller

A background job (like the merge processor or stale agent detector) that continuously compares:
- Meta-spec versions bound to each workspace (desired state)
- Meta-spec versions recorded in provenance for existing code (actual state)

When there's drift, the controller creates reconciliation work according to the workspace's rollout policy.

This is the Kubernetes reconciliation pattern: declare desired state, the controller converges.

#### Flow

```
1. Human updates meta-spec content (push to specs/)
   -> Spec registry records new SHA, approval resets to pending
   -> Human approves new version

2. Human (or policy) updates workspace meta-spec set to reference new SHA
   -> Reconciliation controller detects drift

3. Controller computes blast radius for this workspace
   -> Affected repos and specs identified via provenance query

4. Controller creates reconciliation tasks per affected spec:
   {
     title: "Reconcile: {spec_path} under {meta_spec}@{new_sha}"
     labels: ["reconciliation", "auto-created"]
     spec_ref: "{spec_path}@{current_sha}"  // same spec, new rules
     meta_spec_ref: "{meta_spec_path}@{new_sha}"
     reconciliation_id: "<uuid>"  // groups all tasks in this reconciliation
   }

5. Repo orchestrator spawns agents for reconciliation tasks
   -> Agents are spawned with the NEW meta-spec set
   -> Agents work on reconciliation branches: reconcile/{reconciliation-id}/{spec}
   -> Agents re-implement the spec from scratch (or refactor existing code)
   -> Agents push to their branch, go through normal gates

6. MRs are opened automatically (normal Ralph loop completion)
   -> MR diff shows: existing code (old rules) vs new code (new rules)
   -> Gates validate new code against spec (same spec, so gates work normally)

7. MRs merge through normal merge queue
   -> Provenance records the new meta-spec version
   -> Drift resolved for this spec in this repo
```

#### What "Re-implement" Means

The reconciliation agent receives:
- The spec (unchanged -- the spec is what to build)
- The new meta-spec set (the updated rules for how to build)
- The existing code (for context -- the agent can refactor rather than rewrite)
- A directive: "This code was produced under [old persona]. You are operating under [new persona]. Update the implementation to conform to your current guidelines."

The agent decides whether to:
- Refactor the existing code to conform to new rules
- Rewrite from scratch if the approach is fundamentally different
- Produce a no-op MR if the existing code already conforms (possible -- not all code is affected by every rule change)

No-op MRs (empty diff) are valid outcomes. They mean "this code was evaluated under new rules and found compliant." The provenance is updated to record the new meta-spec version, resolving the drift without changing code.

### 7. Rollout Policy

Each workspace has a rollout policy that governs how reconciliation proceeds:

```rust
pub struct RolloutPolicy {
    pub strategy: RolloutStrategy,
    pub max_concurrent_agents: u32,
    pub priority: ReconciliationPriority,
    pub auto_merge: bool,
}

pub enum RolloutStrategy {
    /// Create all reconciliation tasks immediately.
    /// Budget limits still apply -- tasks queue if budget exhausted.
    Immediate,

    /// Create tasks in batches. Wait for each batch to complete
    /// before starting the next. Batch size = max_concurrent_agents.
    Rolling,

    /// Create tasks but leave them in Backlog.
    /// Reconciliation competes with feature work for agent time.
    /// Repo orchestrator decides priority.
    Background,
}

pub enum ReconciliationPriority {
    /// Reconciliation tasks are processed before feature work.
    High,

    /// Reconciliation tasks are interleaved with feature work.
    /// Repo orchestrator decides ordering.
    Normal,

    /// Reconciliation tasks are processed only when no feature work is pending.
    Low,
}
```

**Default:** `Rolling` strategy, `Normal` priority, `auto_merge: false` (MRs require gate passage but merge automatically if gates pass).

**Budget interaction:** Reconciliation agents consume the same workspace budget as feature agents. The budget system (platform-model.md Section 5) applies uniformly. If the workspace budget is exhausted, reconciliation tasks queue until budget is available. Reconciliation does not get a blank check.

### 8. Tenant-Scope Meta-Spec Changes

A tenant-level meta-spec change affects all workspaces in the tenant. This is the "3803 teams" problem.

**The system does not auto-reconcile at tenant scope.** Instead:

1. Human updates the tenant-level meta-spec
2. System computes blast radius across all workspaces
3. System presents a **rollout plan**: an ordered list of workspaces with blast radius per workspace
4. Human approves the plan (one touchpoint)
5. System updates workspace meta-spec sets in the plan order
6. Each workspace reconciles autonomously per its own rollout policy

The rollout plan can be:
- **All at once:** update all workspace bindings simultaneously. Each workspace reconciles per its own policy.
- **Staged:** update workspace bindings in waves (e.g., 1 workspace first, then 5, then all). Wait for each wave to complete before starting the next.
- **Selective:** update only specific workspaces. Others stay on the old version indefinitely.

```
POST /api/v1/tenant/meta-spec-rollout
{
  "meta_spec_path": "meta/personas/backend-developer.md",
  "new_sha": "x7y8z9",
  "plan": {
    "strategy": "staged",
    "waves": [
      { "workspaces": ["payments"], "wait_for_completion": true },
      { "workspaces": ["onboarding", "kyc"], "wait_for_completion": true },
      { "workspaces": ["*"], "wait_for_completion": false }
    ]
  }
}
```

After the human approves the plan, execution is autonomous. No further human touchpoints.

### 9. Merge Gate Behavior

Code produced under a superseded meta-spec version encounters a **drift warning** in the merge queue:

```
MR #142: "Add payment retry logic"
  Spec: specs/system/payment-retry.md@abc123 (Approved)
  Meta-spec drift: produced under persona v3 (current: v4)

  Gate results:
    [PASS] cargo test
    [PASS] cargo clippy
    [PASS] accountability review
    [WARN] meta-spec-drift: persona backend-developer v3 -> v4
```

**Default behavior: warn, don't block.** The MR merges. The drift is recorded. The reconciliation controller will pick it up.

**Opt-in strict mode (per workspace):**

```rust
pub struct MetaSpecPolicy {
    /// Warn on MRs produced under outdated meta-specs.
    /// Default: true. Always on in strict mode.
    pub warn_on_drift: bool,

    /// Block MRs produced under outdated meta-specs.
    /// Default: false. Only enable for high-assurance workspaces.
    pub block_on_drift: bool,

    /// How many versions behind before blocking (if block_on_drift is true).
    /// Default: 1 (block only if more than 1 version behind).
    /// 0 = block on any drift.
    pub drift_tolerance: u32,
}
```

`block_on_drift` with `drift_tolerance: 0` means every meta-spec change blocks all in-flight MRs. This is appropriate for security-critical workspaces where behavioral guarantees matter. It is not appropriate for most workspaces.

The opinionated default: **warn always, block never, reconcile in the background.** Velocity over strict consistency, with full visibility into drift state.

### 10. Conformance Sweeps (Steady State)

A background job (like speculative merging) continuously checks for meta-spec drift across all repos:

```
Conformance sweep (runs every N hours, configurable):
  For each workspace:
    For each repo in workspace:
      For each spec implemented in repo:
        Compare provenance meta-spec versions against workspace meta-spec set
        If drifted: ensure a reconciliation task exists (create if missing)
```

This catches drift that the reconciliation controller might miss (e.g., a workspace binding was updated while agents were offline, or a reconciliation task was cancelled).

The sweep is cheap (database queries only, no agent spawns). It ensures the system converges even after disruptions.

### 11. Observability

**Domain events:**

| Event | When |
|---|---|
| `MetaSpecChanged` | Meta-spec content updated in spec registry |
| `MetaSpecSetUpdated` | Workspace meta-spec set binding changed |
| `ReconciliationStarted` | Reconciliation controller created tasks for a workspace |
| `ReconciliationCompleted` | All reconciliation tasks for a workspace are done (emitted as `MessageKind::ReconciliationCompleted` Event-tier message per `message-bus.md`) |
| `MetaSpecDriftDetected` | MR or conformance sweep found code under superseded meta-spec |
| `MetaSpecDriftResolved` | Reconciliation MR merged, provenance updated |

**Dashboard:**

- **Workspace detail view:** meta-spec set with version indicators (current/drifted), reconciliation progress bar, drift count per repo.
- **Tenant admin view:** rollout plan status, per-workspace reconciliation progress, total drift count.
- **Repo detail view:** per-spec meta-spec version provenance, drift badges on commits.

**Metrics (Prometheus):**

- `gyre_meta_spec_drift_total{workspace, repo}` -- count of specs with meta-spec drift
- `gyre_reconciliation_tasks_total{workspace, status}` -- reconciliation task counts
- `gyre_reconciliation_duration_seconds{workspace}` -- time from trigger to completion

## Relationship to Existing Specs

**Extends:**
- **platform-model.md** -- meta-spec set is a new workspace-level concept; MCP protocol injection (Section 8) gains meta-spec set resolution
- **spec-registry.md** -- `kind` field added to manifest schema for meta-spec types; meta-specs use the same approval flow and ledger
- **spec-lifecycle.md** -- reconciliation is a new trigger type alongside add/modify/delete/rename; the reconciliation controller extends the existing auto-task-creation pattern

**Depends on:**
- **spec-registry.md** -- versioned specs, approval ledger, manifest
- **platform-model.md** -- workspace scoping, budget cascade, persona model
- **forge-advantages.md** -- zero-latency feedback (reconciliation tasks created same-tick), rich provenance (meta-spec versions in commit records)

**Analogies:**
- Spec lifecycle creates tasks when a spec changes. Meta-spec reconciliation creates tasks when the rules change.
- Speculative merging continuously checks for branch conflicts. Conformance sweeps continuously check for meta-spec drift.
- Budget cascade (tenant -> workspace -> repo) governs resource limits. Meta-spec scope cascade governs behavioral norms.

## Kubernetes Parallels

This design borrows deliberately from Kubernetes:

| Kubernetes | Gyre |
|---|---|
| Desired state (Deployment spec) | Meta-spec set (pinned versions bound to workspace) |
| Actual state (running pods) | Code produced under meta-spec versions (provenance) |
| Controller reconciliation loop | Reconciliation controller (background job) |
| Rolling update strategy | Rollout policy (immediate / rolling / background) |
| Admission controller | Merge gate drift warning/block |
| PodDisruptionBudget | Workspace budget + max_concurrent_agents |
| Namespace | Workspace |
| ConfigMap mounted into pod | Meta-spec set injected into agent prompt |
| Canary deployment | Staged tenant rollout (one workspace first, then expand) |
| `kubectl rollout status` | Reconciliation progress in dashboard |

The key difference: in Kubernetes, reconciliation is cheap (restart a pod). In Gyre, reconciliation is expensive (re-run an implementation). This is why rollout policies, budget constraints, and prioritization matter more here than in Kubernetes.

## What This Does NOT Do

- **Does not auto-update meta-spec sets.** Updating a meta-spec's content does not automatically update workspace bindings. The binding update is a separate, explicit action (or triggered by a tenant rollout plan). This prevents surprise reconciliation.
- **Does not replace spec iteration.** Spec changes (what to build) use the existing spec lifecycle. Meta-spec changes (how to build) use this reconciliation system. They are complementary.
- **Does not require re-implementation of everything.** Only specs with provenance records under the old meta-spec version are candidates. Unaffected repos are skipped. Agents may produce no-op MRs for code that already conforms.
- **Does not block feature work.** Reconciliation competes for budget alongside feature work. The rollout policy controls priority, but the default is `Normal` -- reconciliation is interleaved, not dominant.
- **Does not handle production concerns.** This spec covers the SDLC from spec to merge. Deployment, monitoring, and incident response are out of scope (per the user's framing).
