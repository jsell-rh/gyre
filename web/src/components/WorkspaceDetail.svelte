<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let { workspace, onBack } = $props();

  let budget = $state(null);
  let repos = $state([]);
  let members = $state([]);
  let teams = $state([]);
  let loading = $state(true);
  let activeTab = $state('budget');

  const tabs = [
    { id: 'budget', label: 'Budget' },
    { id: 'repos', label: 'Repos' },
    { id: 'members', label: 'Members' },
    { id: 'teams', label: 'Teams' },
  ];

  $effect(() => {
    if (workspace?.id) loadAll();
  });

  async function loadAll() {
    loading = true;
    try {
      const [b, r, m, t] = await Promise.allSettled([
        api.workspaceBudget(workspace.id),
        api.workspaceRepos(workspace.id),
        api.workspaceMembers(workspace.id),
        api.workspaceTeams(workspace.id),
      ]);
      if (b.status === 'fulfilled') budget = b.value;
      if (r.status === 'fulfilled') repos = r.value ?? [];
      if (m.status === 'fulfilled') members = m.value ?? [];
      if (t.status === 'fulfilled') teams = t.value ?? [];
    } catch (e) {
      showToast('Failed to load workspace details', { type: 'error' });
    } finally {
      loading = false;
    }
  }

  function pct(used, max) {
    if (!max || max === 0) return 0;
    return Math.min(100, Math.round((used / max) * 100));
  }

  function fmtNum(n) {
    if (n == null) return '—';
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
    if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
    return String(n);
  }

  function fmtCost(n) {
    if (n == null) return '—';
    return '$' + Number(n).toFixed(4);
  }

  function roleColor(role) {
    const r = (role ?? '').toLowerCase();
    if (r === 'admin') return 'danger';
    if (r === 'developer') return 'info';
    if (r === 'agent') return 'warning';
    return 'default';
  }
</script>

<div class="workspace-detail">
  <div class="detail-header">
    <button class="back-btn" onclick={onBack} aria-label="Back to workspaces">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
        <path d="M19 12H5M12 19l-7-7 7-7"/>
      </svg>
      Workspaces
    </button>
    <div class="ws-title">
      <h2>{workspace?.name ?? 'Workspace'}</h2>
      {#if workspace?.description}
        <p class="ws-desc">{workspace.description}</p>
      {/if}
    </div>
  </div>

  <Tabs {tabs} bind:activeTab />

  {#if loading}
    <div class="tab-body">
      <Skeleton lines={5} />
    </div>
  {:else if activeTab === 'budget'}
    <div class="tab-body">
      {#if budget}
        <div class="budget-section">
          <h3 class="section-title">Limits &amp; Usage</h3>
          <div class="budget-bars">
            {#if budget.config?.max_tokens_per_day != null}
              {@const p = pct(budget.usage?.tokens_used_today, budget.config.max_tokens_per_day)}
              <div class="budget-bar-row">
                <div class="bar-label">
                  <span>Tokens / Day</span>
                  <span class="bar-nums">{fmtNum(budget.usage?.tokens_used_today)} / {fmtNum(budget.config.max_tokens_per_day)}</span>
                </div>
                <div class="bar-track">
                  <div class="bar-fill" class:bar-warn={p > 75} class:bar-danger={p > 90} style="width: {p}%"></div>
                </div>
                <span class="bar-pct">{p}%</span>
              </div>
            {/if}
            {#if budget.config?.max_cost_per_day != null}
              {@const p = pct(budget.usage?.cost_today, budget.config.max_cost_per_day)}
              <div class="budget-bar-row">
                <div class="bar-label">
                  <span>Cost / Day</span>
                  <span class="bar-nums">{fmtCost(budget.usage?.cost_today)} / {fmtCost(budget.config.max_cost_per_day)}</span>
                </div>
                <div class="bar-track">
                  <div class="bar-fill" class:bar-warn={p > 75} class:bar-danger={p > 90} style="width: {p}%"></div>
                </div>
                <span class="bar-pct">{p}%</span>
              </div>
            {/if}
            {#if budget.config?.max_concurrent_agents != null}
              {@const p = pct(budget.usage?.active_agents, budget.config.max_concurrent_agents)}
              <div class="budget-bar-row">
                <div class="bar-label">
                  <span>Concurrent Agents</span>
                  <span class="bar-nums">{budget.usage?.active_agents ?? 0} / {budget.config.max_concurrent_agents}</span>
                </div>
                <div class="bar-track">
                  <div class="bar-fill" class:bar-warn={p > 75} class:bar-danger={p > 90} style="width: {p}%"></div>
                </div>
                <span class="bar-pct">{p}%</span>
              </div>
            {/if}
          </div>
          {#if !budget.config?.max_tokens_per_day && !budget.config?.max_cost_per_day && !budget.config?.max_concurrent_agents}
            <EmptyState message="No budget limits configured for this workspace." />
          {/if}
        </div>
      {:else}
        <EmptyState message="No budget data available." />
      {/if}
    </div>

  {:else if activeTab === 'repos'}
    <div class="tab-body">
      {#if repos.length === 0}
        <EmptyState message="No repos in this workspace." />
      {:else}
        <table class="data-table">
          <thead>
            <tr><th>Name</th><th>Default Branch</th><th>Mirror</th></tr>
          </thead>
          <tbody>
            {#each repos as r}
              <tr>
                <td class="mono">{r.name}</td>
                <td>{r.default_branch ?? 'main'}</td>
                <td>{r.is_mirror ? 'Yes' : 'No'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>

  {:else if activeTab === 'members'}
    <div class="tab-body">
      {#if members.length === 0}
        <EmptyState message="No members in this workspace." />
      {:else}
        <table class="data-table">
          <thead>
            <tr><th>User</th><th>Role</th><th>Joined</th></tr>
          </thead>
          <tbody>
            {#each members as m}
              <tr>
                <td>{m.display_name ?? m.user_id ?? m.id}</td>
                <td><Badge variant={roleColor(m.role)} label={m.role ?? 'member'} /></td>
                <td class="muted">{m.joined_at ? new Date(m.joined_at).toLocaleDateString() : '—'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    </div>

  {:else if activeTab === 'teams'}
    <div class="tab-body">
      {#if teams.length === 0}
        <EmptyState message="No teams in this workspace." />
      {:else}
        <div class="teams-grid">
          {#each teams as team}
            <div class="team-card">
              <div class="team-name">{team.name}</div>
              {#if team.description}
                <div class="team-desc">{team.description}</div>
              {/if}
              <div class="team-meta">
                <span class="muted">{team.member_count ?? 0} members</span>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .workspace-detail {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .detail-header {
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: 0;
    margin-bottom: var(--space-2);
  }

  .back-btn:hover { color: var(--color-text); }

  .ws-title h2 {
    margin: 0;
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
  }

  .ws-desc {
    margin: var(--space-1) 0 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .tab-body {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
  }

  .section-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--space-4);
  }

  .budget-bars { display: flex; flex-direction: column; gap: var(--space-5); max-width: 640px; }

  .budget-bar-row { display: flex; flex-direction: column; gap: var(--space-1); }

  .bar-label {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .bar-nums { color: var(--color-text-muted); font-family: var(--font-mono); font-size: var(--text-xs); }

  .bar-track {
    height: 8px;
    background: var(--color-surface-elevated);
    border-radius: 999px;
    overflow: hidden;
  }

  .bar-fill {
    height: 100%;
    background: var(--color-primary);
    border-radius: 999px;
    transition: width 0.4s ease;
  }

  .bar-fill.bar-warn { background: var(--color-warning); }
  .bar-fill.bar-danger { background: var(--color-danger); }

  .bar-pct { font-size: var(--text-xs); color: var(--color-text-muted); align-self: flex-end; }

  .data-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .data-table th {
    text-align: left;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text-muted);
    font-weight: 500;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .data-table td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
  }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }
  .muted { color: var(--color-text-muted); }

  .teams-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: var(--space-4);
  }

  .team-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
  }

  .team-name { font-weight: 600; color: var(--color-text); margin-bottom: var(--space-1); }
  .team-desc { font-size: var(--text-sm); color: var(--color-text-secondary); margin-bottom: var(--space-2); }
  .team-meta { font-size: var(--text-xs); }
</style>
