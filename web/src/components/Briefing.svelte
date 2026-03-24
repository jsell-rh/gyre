<script>
  import { onMount } from 'svelte';
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';

  const LAST_VISIT_KEY = 'gyre_last_visit';
  const BRIEFING_LAST_VISIT_KEY = 'gyre_briefing_last_visit';
  const WORKSPACE_KEY = 'gyre_workspace_id';

  let loading = $state(true);
  let error = $state(null);

  let lastVisit = $state(null);
  let durationLabel = $state('');

  // Workspace briefing (TASK-205)
  let workspaceId = $state(null);
  let workspaceSummary = $state(null);
  let workspaceDeltas = $state([]);

  // Computed data (fallback 4-card layout)
  let agentsCompleted = $state(0);
  let mrsMerged = $state(0);
  let specChanges = $state(0);
  let activeAgents = $state([]);
  let pendingSpecsCount = $state(0);
  let driftedSpecsCount = $state(0);
  let gateFailures = $state([]);

  function computeDuration(since) {
    const diff = Date.now() - since;
    const h = Math.floor(diff / 3600000);
    const m = Math.floor((diff % 3600000) / 60000);
    if (h > 24) return `${Math.floor(h / 24)} days`;
    if (h > 0) return `${h} hour${h !== 1 ? 's' : ''}`;
    return `${m} minute${m !== 1 ? 's' : ''}`;
  }

  function relativeTime(ts) {
    if (!ts) return '';
    const diff = Date.now() - new Date(ts).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 1) return 'just now';
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  async function load() {
    const storedVisit = localStorage.getItem(LAST_VISIT_KEY);
    const visitTs = storedVisit ? parseInt(storedVisit, 10) : Date.now() - 86400000; // default: 24h ago
    lastVisit = visitTs;
    durationLabel = computeDuration(visitTs);

    // Record this visit
    localStorage.setItem(LAST_VISIT_KEY, String(Date.now()));

    // Check for selected workspace (TASK-205)
    const wsId = localStorage.getItem(WORKSPACE_KEY);
    workspaceId = wsId || null;

    if (wsId) {
      // Use briefing-specific last_visit for since param
      const briefingLastVisit = localStorage.getItem(BRIEFING_LAST_VISIT_KEY);
      const sinceEpoch = briefingLastVisit ? parseInt(briefingLastVisit, 10) : Date.now() - 86400000;
      localStorage.setItem(BRIEFING_LAST_VISIT_KEY, String(Date.now()));

      try {
        const briefing = await api.getWorkspaceBriefing(wsId, sinceEpoch);
        workspaceSummary = briefing?.summary ?? null;
        workspaceDeltas = briefing?.deltas ?? [];
        error = null;
      } catch (e) {
        error = e.message;
      } finally {
        loading = false;
      }
      return;
    }

    // No workspace selected — fallback to 4-card layout
    try {
      const [activityRes, agentsRes, pendingRes, driftedRes] = await Promise.allSettled([
        api.activity(100),
        api.agents({ status: 'active' }),
        api.getPendingSpecs(),
        api.getDriftedSpecs(),
      ]);

      if (activityRes.status === 'fulfilled') {
        const events = activityRes.value || [];
        const sinceMs = visitTs;
        const recent = events.filter(e => e.timestamp && new Date(e.timestamp).getTime() >= sinceMs);
        agentsCompleted = recent.filter(e => e.event_type === 'RUN_FINISHED').length;
        mrsMerged = recent.filter(e => e.event_type === 'MrMerged' || e.description?.includes('merged')).length;
        specChanges = recent.filter(e => e.event_type === 'SpecChanged').length;
        gateFailures = recent.filter(e => e.event_type === 'GateFailure').slice(0, 5);
      }

      if (agentsRes.status === 'fulfilled') {
        activeAgents = (agentsRes.value || []).slice(0, 10);
      }

      if (pendingRes.status === 'fulfilled') {
        pendingSpecsCount = (pendingRes.value || []).length;
      }

      if (driftedRes.status === 'fulfilled') {
        driftedSpecsCount = (driftedRes.value || []).length;
      }

      error = null;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function agentDuration(agent) {
    if (!agent.created_at) return '';
    const diff = Date.now() - new Date(agent.created_at).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 60) return `${m}m`;
    return `${Math.floor(m / 60)}h ${m % 60}m`;
  }

  onMount(load);
</script>

<div class="briefing">
  <div class="briefing-header">
    <h1 class="briefing-title">Briefing</h1>
    {#if !loading}
      <span class="briefing-since">Since {durationLabel} ago</span>
    {/if}
  </div>

  {#if loading}
    <div class="cards-grid">
      {#each [1,2,3,4] as _}
        <Skeleton height="140px" />
      {/each}
    </div>
  {:else if error}
    <div class="briefing-error" role="alert">Error loading briefing: {error}</div>
  {:else if workspaceId}
    <!-- Workspace narrative briefing (TASK-205) -->
    {#if workspaceSummary}
      <div class="narrative workspace-narrative" data-testid="workspace-summary">
        {workspaceSummary}
      </div>
    {/if}

    {#if workspaceDeltas.length === 0}
      <EmptyState
        title="No architectural changes"
        description="No changes recorded since your last visit."
      />
    {:else}
      <div class="deltas-section">
        <h2 class="deltas-heading">Architectural Deltas</h2>
        <ul class="deltas-list">
          {#each workspaceDeltas as delta}
            <li class="delta-row">
              <span class="delta-sha" title={delta.commit_sha}>
                {delta.commit_sha ? delta.commit_sha.slice(0, 7) : '—'}
              </span>
              <span class="delta-time">{relativeTime(delta.timestamp)}</span>
              {#if delta.spec_ref}
                <span class="delta-spec" title={delta.spec_ref}>{delta.spec_ref}</span>
              {/if}
              {#if delta.agent_id}
                <span class="delta-agent" title={delta.agent_id}>
                  {delta.agent_id.slice(0, 8)}
                </span>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  {:else}
    <!-- Fallback: no workspace selected — narrative summary -->
    <div class="narrative">
      In the last {durationLabel},
      <strong>{agentsCompleted}</strong> agent{agentsCompleted !== 1 ? 's' : ''} completed task{agentsCompleted !== 1 ? 's' : ''},
      <strong>{mrsMerged}</strong> MR{mrsMerged !== 1 ? 's' : ''} merged,
      and <strong>{specChanges}</strong> spec change{specChanges !== 1 ? 's' : ''} recorded.
    </div>

    <div class="cards-grid">
      <!-- Active agents card -->
      <div class="briefing-card">
        <div class="card-header">
          <span class="card-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
              <rect x="3" y="11" width="18" height="11" rx="2"/>
              <path d="M7 11V7a5 5 0 0110 0v4"/>
              <circle cx="12" cy="16" r="1" fill="currentColor"/>
            </svg>
          </span>
          <span class="card-title">Active Agents</span>
          <span class="card-count">{activeAgents.length}</span>
        </div>
        {#if activeAgents.length === 0}
          <p class="card-empty">No agents currently running.</p>
        {:else}
          <ul class="agent-list">
            {#each activeAgents as agent}
              <li class="agent-row">
                <span class="agent-name">{agent.name}</span>
                <span class="agent-duration">{agentDuration(agent)}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>

      <!-- Spec health card -->
      <div class="briefing-card">
        <div class="card-header">
          <span class="card-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
              <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/>
              <polyline points="14 2 14 8 20 8"/>
              <line x1="16" y1="13" x2="8" y2="13"/>
              <line x1="16" y1="17" x2="8" y2="17"/>
            </svg>
          </span>
          <span class="card-title">Spec Health</span>
        </div>
        {#if pendingSpecsCount === 0 && driftedSpecsCount === 0}
          <p class="card-ok">All specs approved ✓</p>
        {:else}
          <div class="spec-health-rows">
            {#if pendingSpecsCount > 0}
              <div class="spec-health-row warning">
                <span class="spec-health-label">Pending approvals</span>
                <span class="spec-health-val">{pendingSpecsCount}</span>
              </div>
            {/if}
            {#if driftedSpecsCount > 0}
              <div class="spec-health-row danger">
                <span class="spec-health-label">Drifted specs</span>
                <span class="spec-health-val">{driftedSpecsCount}</span>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Recent activity card -->
      <div class="briefing-card">
        <div class="card-header">
          <span class="card-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
              <polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>
            </svg>
          </span>
          <span class="card-title">Since Last Visit</span>
        </div>
        <div class="activity-rows">
          <div class="activity-row">
            <span class="activity-label">Agents completed</span>
            <span class="activity-val">{agentsCompleted}</span>
          </div>
          <div class="activity-row">
            <span class="activity-label">MRs merged</span>
            <span class="activity-val">{mrsMerged}</span>
          </div>
          <div class="activity-row">
            <span class="activity-label">Spec changes</span>
            <span class="activity-val">{specChanges}</span>
          </div>
        </div>
      </div>

      <!-- Gate failures card -->
      <div class="briefing-card">
        <div class="card-header">
          <span class="card-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="18" height="18">
              <circle cx="12" cy="12" r="10"/>
              <line x1="12" y1="8" x2="12" y2="12"/>
              <line x1="12" y1="16" x2="12.01" y2="16"/>
            </svg>
          </span>
          <span class="card-title">Recent Gate Failures</span>
          {#if gateFailures.length > 0}
            <span class="card-count danger">{gateFailures.length}</span>
          {/if}
        </div>
        {#if gateFailures.length === 0}
          <p class="card-ok">No gate failures ✓</p>
        {:else}
          <ul class="failure-list">
            {#each gateFailures as evt}
              <li class="failure-row">
                <span class="failure-desc">{evt.description || 'Gate failure'}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .briefing {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    max-width: 1000px;
  }

  .briefing-header {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
  }

  .briefing-title {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .briefing-since {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .narrative {
    font-size: var(--text-base);
    color: var(--color-text-secondary);
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
    border-left: 3px solid var(--color-primary);
    line-height: 1.6;
  }

  .cards-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: var(--space-4);
  }

  .briefing-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg, var(--radius));
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .card-icon {
    display: flex;
    align-items: center;
    color: var(--color-primary);
    flex-shrink: 0;
  }

  .card-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    flex: 1;
  }

  .card-count {
    font-size: var(--text-xs);
    font-weight: 700;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--color-surface-elevated);
    color: var(--color-text-secondary);
  }

  .card-count.danger {
    background: rgba(220, 38, 38, 0.15);
    color: var(--color-danger);
  }

  .card-empty,
  .card-ok {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .card-ok {
    color: var(--color-success);
  }

  .agent-list,
  .failure-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .agent-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: var(--text-xs);
  }

  .agent-name {
    color: var(--color-text);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 70%;
  }

  .agent-duration {
    color: var(--color-text-muted);
  }

  .activity-rows,
  .spec-health-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .activity-row,
  .spec-health-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: var(--text-sm);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .activity-row:last-child,
  .spec-health-row:last-child {
    border-bottom: none;
  }

  .activity-label,
  .spec-health-label {
    color: var(--color-text-secondary);
  }

  .activity-val,
  .spec-health-val {
    font-weight: 600;
    color: var(--color-text);
  }

  .spec-health-row.warning .spec-health-val {
    color: var(--color-warning, #f59e0b);
  }

  .spec-health-row.danger .spec-health-val {
    color: var(--color-danger);
  }

  .failure-row {
    font-size: var(--text-xs);
    color: var(--color-danger);
    padding: var(--space-1) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .failure-row:last-child {
    border-bottom: none;
  }

  .failure-desc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: block;
  }

  .briefing-error {
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-danger);
    border-radius: var(--radius);
  }

  /* Workspace briefing (TASK-205) */
  .workspace-narrative {
    border-left-color: var(--color-success, #22c55e);
  }

  .deltas-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .deltas-heading {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .deltas-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .delta-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-xs);
    border-bottom: 1px solid var(--color-border);
  }

  .delta-row:last-child {
    border-bottom: none;
  }

  .delta-sha {
    font-family: var(--font-mono);
    color: var(--color-primary);
    flex-shrink: 0;
    min-width: 5ch;
  }

  .delta-time {
    color: var(--color-text-muted);
    flex-shrink: 0;
    min-width: 6ch;
  }

  .delta-spec {
    color: var(--color-text-secondary);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .delta-agent {
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }
</style>
