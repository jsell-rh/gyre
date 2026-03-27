<script>
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  const navigate = getContext('navigate');

  let { workspace, onBack } = $props();

  let budget = $state(null);
  let repos = $state([]);
  let members = $state([]);
  let teams = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let activeTab = $state('budget');

  let budgetFormOpen = $state(false);
  let budgetSaving = $state(false);
  let budgetForm = $state({ max_tokens_per_day: '', max_cost_per_day: '', max_concurrent_agents: '', max_agent_lifetime_secs: '' });

  const TRUST_LEVELS = ['supervised', 'guided', 'autonomous', 'custom'];
  let trustSaving = $state(false);

  async function saveTrustLevel(newLevel) {
    trustSaving = true;
    try {
      await api.updateWorkspace(workspace.id, { trust_level: newLevel });
      workspace.trust_level = newLevel;
      showToast('Trust level updated', { type: 'success' });
    } catch (e) {
      showToast('Failed to update trust level: ' + e.message, { type: 'error' });
    } finally {
      trustSaving = false;
    }
  }

  async function saveBudget() {
    budgetSaving = true;
    try {
      const data = {};
      if (budgetForm.max_tokens_per_day !== '') data.max_tokens_per_day = Number(budgetForm.max_tokens_per_day);
      if (budgetForm.max_cost_per_day !== '') data.max_cost_per_day = Number(budgetForm.max_cost_per_day);
      if (budgetForm.max_concurrent_agents !== '') data.max_concurrent_agents = Number(budgetForm.max_concurrent_agents);
      if (budgetForm.max_agent_lifetime_secs !== '') data.max_agent_lifetime_secs = Number(budgetForm.max_agent_lifetime_secs);
      await api.setWorkspaceBudget(workspace.id, data);
      showToast('Budget updated', { type: 'success' });
      budgetFormOpen = false;
      const b = await api.workspaceBudget(workspace.id);
      budget = b;
    } catch (e) {
      showToast('Failed to save budget: ' + e.message, { type: 'error' });
    } finally {
      budgetSaving = false;
    }
  }

  const tabs = [
    { id: 'budget', label: 'Budget' },
    { id: 'repos', label: 'Repos' },
    { id: 'members', label: 'Members' },
    { id: 'teams', label: 'Teams' },
    { id: 'policies', label: 'Policies' },
  ];

  $effect(() => {
    if (workspace?.id) loadAll();
  });

  async function loadAll() {
    loading = true;
    error = null;
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
      const allFailed = [b, r, m, t].every((p) => p.status === 'rejected');
      if (allFailed) {
        error = 'Failed to load workspace details. All requests failed.';
      }
    } catch (e) {
      error = 'Failed to load workspace details';
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
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5M12 19l-7-7 7-7"/>
      </svg>
      Workspaces
    </button>
    <div class="ws-title">
      <div class="ws-name-row">
        <h2>{workspace?.name ?? 'Workspace'}</h2>
        <div class="trust-selector-group">
          <label class="trust-label" for="trust-select">Trust Level</label>
          <select
            id="trust-select"
            class="trust-select trust-{(workspace?.trust_level ?? 'supervised').toLowerCase()}"
            value={workspace?.trust_level ?? 'supervised'}
            onchange={(e) => saveTrustLevel(e.target.value)}
            disabled={trustSaving}
            aria-label="Workspace trust level"
          >
            {#each TRUST_LEVELS as level}
              <option value={level}>{level}</option>
            {/each}
          </select>
          {#if trustSaving}
            <span class="trust-saving" role="status" aria-live="polite">Saving…</span>
          {/if}
        </div>
      </div>
      {#if workspace?.description}
        <p class="ws-desc">{workspace.description}</p>
      {/if}
    </div>
  </div>

  <Tabs {tabs} bind:active={activeTab} />

  {#if error}
    <div class="error-banner" role="alert">
      <p>{error}</p>
      <button class="btn-retry" onclick={() => { error = null; loadAll(); }}>Retry</button>
    </div>
  {/if}

  <div role="tabpanel" id="tabpanel-{activeTab}" aria-labelledby="tab-{activeTab}" aria-busy={loading}>
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
                  <div class="bar-fill" class:bar-warn={p > 75} class:bar-danger={p > 90} style="width: {p}%" role="progressbar" aria-valuenow={p} aria-valuemin={0} aria-valuemax={100} aria-label="Tokens per day: {p}% used"></div>
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
                  <div class="bar-fill" class:bar-warn={p > 75} class:bar-danger={p > 90} style="width: {p}%" role="progressbar" aria-valuenow={p} aria-valuemin={0} aria-valuemax={100} aria-label="Cost per day: {p}% used"></div>
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
                  <div class="bar-fill" class:bar-warn={p > 75} class:bar-danger={p > 90} style="width: {p}%" role="progressbar" aria-valuenow={p} aria-valuemin={0} aria-valuemax={100} aria-label="Concurrent agents: {p}% of limit used"></div>
                </div>
                <span class="bar-pct">{p}%</span>
              </div>
            {/if}
          </div>
          {#if !budget.config?.max_tokens_per_day && !budget.config?.max_cost_per_day && !budget.config?.max_concurrent_agents}
            <EmptyState title="No budget limits configured for this workspace." description="No budget limits set. Click 'Configure Budget' to add limits." />
          {/if}
          {#if !budgetFormOpen}
            <button class="configure-budget-btn" aria-expanded={budgetFormOpen} aria-controls="budget-form" onclick={() => { budgetFormOpen = true; budgetForm = { max_tokens_per_day: budget.config?.max_tokens_per_day ?? '', max_cost_per_day: budget.config?.max_cost_per_day ?? '', max_concurrent_agents: budget.config?.max_concurrent_agents ?? '', max_agent_lifetime_secs: budget.config?.max_agent_lifetime_secs ?? '' }; }}>Configure Budget</button>
          {:else}
            <form id="budget-form" class="budget-form" onsubmit={(e) => { e.preventDefault(); saveBudget(); }}>
              <h4 class="budget-form-title">Set Budget Limits</h4>
              <div class="budget-form-fields">
                <label class="budget-field">
                  <span>Max Tokens / Day</span>
                  <input type="number" min="0" placeholder="e.g. 1000000" bind:value={budgetForm.max_tokens_per_day} />
                </label>
                <label class="budget-field">
                  <span>Max Cost / Day ($)</span>
                  <input type="number" min="0" step="0.01" placeholder="e.g. 10.00" bind:value={budgetForm.max_cost_per_day} />
                </label>
                <label class="budget-field">
                  <span>Max Concurrent Agents</span>
                  <input type="number" min="0" placeholder="e.g. 5" bind:value={budgetForm.max_concurrent_agents} />
                </label>
                <label class="budget-field">
                  <span>Max Agent Lifetime (secs)</span>
                  <input type="number" min="0" placeholder="e.g. 3600" bind:value={budgetForm.max_agent_lifetime_secs} />
                </label>
              </div>
              <div class="budget-form-actions">
                <button type="submit" class="btn-primary" disabled={budgetSaving}>{budgetSaving ? 'Saving…' : 'Save'}</button>
                <button type="button" class="btn-secondary" onclick={() => budgetFormOpen = false}>Cancel</button>
              </div>
            </form>
          {/if}
        </div>
      {:else}
        <EmptyState title="No budget data available." description="Budget information is not configured for this workspace." />
      {/if}
    </div>

  {:else if activeTab === 'repos'}
    <div class="tab-body">
      {#if repos.length === 0}
        <EmptyState title="No repos in this workspace." description="Add repositories to start tracking work here." />
      {:else}
        <table class="data-table">
          <thead>
            <tr><th scope="col">Name</th><th>Default Branch</th><th>Mirror</th></tr>
          </thead>
          <tbody>
            {#each repos as r}
              <tr>
                <td class="mono">
                  {#if navigate}
                    <button class="repo-link-btn" onclick={() => navigate('repo-detail', { repo: r })}>{r.name}</button>
                  {:else}
                    {r.name}
                  {/if}
                </td>
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
        <EmptyState title="No members in this workspace." description="Members can be added from the admin panel." />
      {:else}
        <table class="data-table">
          <thead>
            <tr><th scope="col">User</th><th scope="col">Role</th><th>Joined</th></tr>
          </thead>
          <tbody>
            {#each members as m}
              <tr>
                <td>{m.display_name ?? m.user_id ?? m.id}</td>
                <td><Badge variant={roleColor(m.role)} value={m.role ?? 'member'} /></td>
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
        <EmptyState title="No teams in this workspace." description="Teams can be added from the admin panel." />
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

  {:else if activeTab === 'policies'}
    <div class="tab-body">
      <EmptyState title="No policies configured" description="ABAC policies will be manageable here." />
    </div>
  {/if}
  </div>
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
  .back-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

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

  .budget-bar-row { display: flex; flex-direction: column; gap: var(--space-2); }

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
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .bar-fill {
    height: 100%;
    background: var(--color-success);
    border-radius: var(--radius-full);
    transition: width var(--transition-normal);
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
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text-muted);
    font-weight: 500;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .data-table td {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
  }

  .mono { font-family: var(--font-mono); font-size: var(--text-xs); }

  .repo-link-btn {
    background: none;
    border: none;
    color: var(--color-link);
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .repo-link-btn:hover { opacity: 0.8; }
  .repo-link-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
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
    padding: var(--space-6);
  }

  .team-name { font-weight: 600; color: var(--color-text); margin-bottom: var(--space-1); }
  .team-desc { font-size: var(--text-sm); color: var(--color-text-secondary); margin-bottom: var(--space-2); }
  .team-meta { font-size: var(--text-xs); }

  .configure-budget-btn {
    margin-top: var(--space-4);
    padding: var(--space-2) var(--space-4);
    background: var(--color-link);
    color: var(--color-text-inverse);
    border: none;
    border-radius: var(--radius);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }
  .configure-budget-btn:hover { background: var(--color-link-hover); }
  .configure-budget-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .budget-form {
    margin-top: var(--space-5);
    max-width: 480px;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-5);
  }
  .budget-form-title { margin: 0 0 var(--space-4); font-size: var(--text-sm); font-weight: 600; color: var(--color-text); }
  .budget-form-fields { display: flex; flex-direction: column; gap: var(--space-3); }
  .budget-field { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); color: var(--color-text-secondary); }
  .budget-field input { padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius); background: var(--color-surface); color: var(--color-text); font-size: var(--text-sm); }
  .budget-field input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-color: var(--color-focus);
  }
  .budget-form-actions { display: flex; gap: var(--space-2); margin-top: var(--space-4); }
  .btn-primary { padding: var(--space-2) var(--space-4); background: var(--color-link); color: var(--color-text-inverse); border: none; border-radius: var(--radius); font-size: var(--text-sm); cursor: pointer; }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-secondary { padding: var(--space-2) var(--space-4); background: transparent; color: var(--color-text-muted); border: 1px solid var(--color-border); border-radius: var(--radius); font-size: var(--text-sm); cursor: pointer; transition: background var(--transition-fast), color var(--transition-fast); }
  .btn-secondary:hover { color: var(--color-text); background: var(--color-surface-elevated); }
  .btn-primary:focus-visible,
  .btn-secondary:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .ws-name-row { display: flex; align-items: center; gap: var(--space-3); }

  .trust-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 2px var(--space-2);
    border-radius: var(--radius-full);
    text-transform: capitalize;
  }
  .trust-selector-group {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .trust-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }
  .trust-select {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 2px var(--space-2);
    border-radius: var(--radius-full);
    text-transform: capitalize;
    border: 1px solid var(--color-border-strong);
    cursor: pointer;
    background: var(--color-surface);
    color: var(--color-text);
  }
  .trust-select:disabled { opacity: 0.5; cursor: not-allowed; }
  .trust-select:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .trust-select.trust-supervised { background: color-mix(in srgb, var(--color-info) 15%, transparent); color: var(--color-blocked); border-color: color-mix(in srgb, var(--color-info) 30%, transparent); }
  .trust-select.trust-guided     { background: color-mix(in srgb, var(--color-info) 15%, transparent); color: var(--color-link); border-color: color-mix(in srgb, var(--color-info) 30%, transparent); }
  .trust-select.trust-autonomous { background: color-mix(in srgb, var(--color-success) 15%, transparent);  color: var(--color-success); border-color: color-mix(in srgb, var(--color-success) 30%, transparent); }
  .trust-select.trust-custom     { background: color-mix(in srgb, var(--color-warning) 15%, transparent); color: var(--color-warning); border-color: color-mix(in srgb, var(--color-warning) 30%, transparent); }
  .trust-saving { font-size: var(--text-xs); color: var(--color-text-muted); }

  .error-banner {
    padding: var(--space-4) var(--space-6);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--color-danger) 25%, transparent);
    color: var(--color-danger);
    font-size: var(--text-sm);
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .error-banner p { margin: 0; }

  .btn-retry {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .btn-retry:hover { background: var(--color-surface-hover); }
  .btn-retry:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  @media (prefers-reduced-motion: reduce) {
    .bar-fill { transition: none; }
  }
</style>
