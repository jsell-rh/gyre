<script>
  import { getContext } from 'svelte';
  import { api } from '../lib/api.js';
  import Card from '../lib/Card.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  // onSelectWorkspace prop: called for test hooks and old-shell compat.
  // In the new shell (s4-shell), navigate context is the primary mechanism.
  let { onSelectWorkspace = null } = $props();

  const navigate = getContext('navigate');

  let workspaces = $state([]);
  let wsFilter = $state('');
  let loading = $state(true);
  let error = $state(null);
  // Per-workspace enrichment keyed by id
  let enrichment = $state({});

  $effect(() => {
    load();
  });

  async function load() {
    loading = true;
    error = null;
    try {
      workspaces = (await api.workspaces()) ?? [];
    } catch (e) {
      error = e.message;
      showToast('Failed to load workspaces: ' + e.message, { type: 'error' });
      workspaces = [];
    } finally {
      loading = false;
    }
    if (workspaces.length > 0) {
      loadEnrichment(workspaces);
    }
  }

  async function loadEnrichment(wsList) {
    // Process in batches of 4 to avoid hammering the server with N*2 parallel requests
    const BATCH_SIZE = 4;
    const next = {};
    for (let i = 0; i < wsList.length; i += BATCH_SIZE) {
      const batch = wsList.slice(i, i + BATCH_SIZE);
      const results = await Promise.allSettled(
        batch.map(async (ws) => {
          const [budgetResult, reposResult] = await Promise.allSettled([
            api.workspaceBudget(ws.id),
            api.workspaceRepos(ws.id),
          ]);

          const budget = budgetResult.status === 'fulfilled' ? budgetResult.value : null;
          const repos = reposResult.status === 'fulfilled' ? (reposResult.value ?? []) : [];

          let budgetPct = null;
          if (budget?.usage != null && budget?.config != null) {
            const { tokens_used_today, cost_today } = budget.usage;
            const { max_tokens_per_day, max_cost_per_day } = budget.config;
            if (max_tokens_per_day && max_tokens_per_day > 0) {
              budgetPct = Math.min(100, Math.round((tokens_used_today / max_tokens_per_day) * 100));
            } else if (max_cost_per_day && max_cost_per_day > 0) {
              budgetPct = Math.min(100, Math.round((cost_today / max_cost_per_day) * 100));
            }
          }

          const activeAgents = budget?.usage?.active_agents ?? 0;

          return { id: ws.id, repoCount: repos.length, activeAgents, budgetPct };
        })
      );

      results.forEach((r, j) => {
        const id = batch[j].id;
        if (r.status === 'fulfilled') {
          next[id] = r.value;
        } else {
          next[id] = { id, repoCount: 0, activeAgents: 0, budgetPct: null, error: true };
        }
      });
      // Update incrementally so cards render enrichment as batches complete
      enrichment = { ...next };
    }
  }

  function budgetBarColor(pct) {
    if (pct == null) return 'var(--color-border-strong)';
    if (pct >= 95) return 'var(--color-danger)';
    if (pct >= 80) return 'var(--color-warning)';
    return 'var(--color-success)';
  }

  function trustVariant(level) {
    if (!level) return 'muted';
    const l = String(level).toLowerCase();
    if (l === 'autonomous') return 'success';
    if (l === 'guided') return 'info';
    if (l === 'supervised') return 'warning';
    return 'muted';
  }

  const visibleWs = $derived(
    wsFilter.trim()
      ? workspaces.filter(ws => ws.name?.toLowerCase().includes(wsFilter.toLowerCase()))
      : workspaces
  );

  function handleEnter(ws) {
    if (onSelectWorkspace) {
      // Parent handles workspace selection + navigation (old shell / tests)
      onSelectWorkspace(ws);
    } else {
      // New shell: navigate directly via context
      navigate?.('inbox', { type: 'workspace', workspaceId: ws.id });
    }
  }
</script>

<div class="workspace-cards">
  <div class="cards-header">
    <h1 class="page-title">Explorer</h1>
    <p class="subtitle">Select a workspace to explore its architecture</p>
  </div>

  {#if loading}
    <div class="cards-grid" aria-busy="true" aria-label="Loading workspaces">
      {#each Array(4) as _}
        <div class="card-skeleton"><Skeleton lines={5} /></div>
      {/each}
    </div>

  {:else if error}
    <div class="error-banner" role="alert">
      <p>Failed to load workspaces: {error}</p>
      <button class="retry-btn" onclick={load}>Retry</button>
    </div>

  {:else if workspaces.length === 0}
    <div class="empty-wrap">
      <EmptyState
        title="No workspaces found."
        description="Create a workspace in Admin to get started."
      >
        {#snippet action()}
          <button class="btn-secondary" onclick={() => navigate?.('admin')}>
            Go to Admin
          </button>
        {/snippet}
      </EmptyState>
    </div>

  {:else}
    <div class="ws-filter-wrap">
      <input type="text" bind:value={wsFilter} placeholder="Filter workspaces…" class="ws-filter" aria-label="Filter workspaces" />
      <span class="sr-only" aria-live="polite" role="status">{visibleWs.length} workspace{visibleWs.length === 1 ? '' : 's'} shown</span>
    </div>
    {#if visibleWs.length === 0}
      <div class="empty-wrap">
        <EmptyState title="No results" description="No workspaces match your filter.">
          {#snippet action()}
            <button class="btn-secondary" onclick={() => { wsFilter = ''; }}>Clear filter</button>
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <div class="cards-grid" role="list" aria-label="Workspaces">
        {#each visibleWs as ws (ws.id)}
          {@const info = enrichment[ws.id]}
          <div class="ws-card" role="listitem">
            <Card>
              {#snippet header()}
                <span class="ws-name" title={ws.name}>{ws.name}</span>
                {#if ws.trust_level}
                  <Badge value={ws.trust_level} variant={trustVariant(ws.trust_level)} />
                {:else}
                  <Badge value="Standard" variant="muted" />
                {/if}
              {/snippet}

              <div class="ws-body">
                {#if ws.description}
                  <p class="ws-desc" title={ws.description}>{ws.description}</p>
                {/if}

                <div class="ws-stats">
                  <div class="stat-row">
                    <span class="stat-icon" aria-hidden="true">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14">
                        <path d="M3 3h6l2 3h10a2 2 0 012 2v11a2 2 0 01-2 2H3a2 2 0 01-2-2V5a2 2 0 012-2z"/>
                      </svg>
                    </span>
                    {#if info != null && !info.error}
                      <span class="stat-val">{info.repoCount}</span>
                    {:else if info?.error}
                      <span class="stat-dash">&mdash;</span>
                    {:else}
                      <span class="stat-dash">…</span>
                    {/if}
                    <span class="stat-label">repos</span>
                  </div>

                  <div class="stat-row">
                    <span class="stat-icon" aria-hidden="true">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" width="14" height="14">
                        <rect x="3" y="11" width="18" height="11" rx="2"/>
                        <path d="M7 11V7a5 5 0 0110 0v4"/>
                        <circle cx="12" cy="16" r="1" fill="currentColor"/>
                      </svg>
                    </span>
                    {#if info != null && !info.error}
                      <span class="stat-val">{info.activeAgents}</span>
                    {:else if info?.error}
                      <span class="stat-dash">&mdash;</span>
                    {:else}
                      <span class="stat-dash">…</span>
                    {/if}
                    <span class="stat-label">active agents</span>
                  </div>
                </div>

                <!-- Budget bar -->
                <div class="budget-section">
                  <div class="budget-label-row">
                    <span class="budget-label">Budget</span>
                    {#if info?.budgetPct != null}
                      <span
                        class="budget-pct"
                        style="color: {budgetBarColor(info.budgetPct)}"
                        aria-label="Budget usage: {info.budgetPct}%{info.budgetPct >= 95 ? ' (critical)' : info.budgetPct >= 80 ? ' (warning)' : ''}"
                      >{info.budgetPct}%</span>
                    {:else}
                      <span class="budget-pct budget-unknown">—</span>
                    {/if}
                  </div>
                  <div class="budget-bar-track" role="progressbar" aria-label="Budget usage" aria-busy={info == null} aria-valuenow={info?.budgetPct ?? 0} aria-valuemin="0" aria-valuemax="100" aria-valuetext="{info?.budgetPct ?? 0}%{(info?.budgetPct ?? 0) >= 95 ? ' critical' : (info?.budgetPct ?? 0) >= 80 ? ' warning' : ''}">
                    <div
                      class="budget-bar-fill"
                      style="width: {info?.budgetPct ?? 0}%; background: {budgetBarColor(info?.budgetPct ?? null)}"
                    ></div>
                  </div>
                </div>
              </div>

              {#snippet footer()}
                <button
                  class="enter-btn"
                  onclick={() => handleEnter(ws)}
                  aria-label="Enter workspace {ws.name}"
                >
                  Enter Workspace
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
                    <path d="M9 18l6-6-6-6"/>
                  </svg>
                </button>
              {/snippet}
            </Card>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .workspace-cards {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .cards-header {
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
  }

  .cards-header .page-title {
    margin: 0 0 var(--space-1);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
  }

  .subtitle {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .ws-filter-wrap {
    padding: var(--space-4) var(--space-6) 0;
    flex-shrink: 0;
  }

  .ws-filter {
    width: 100%;
    max-width: 320px;
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    box-sizing: border-box;
    transition: border-color var(--transition-fast);
  }

  .ws-filter:focus:not(:focus-visible) { outline: none; }
  .ws-filter:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  .cards-grid {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: var(--space-4);
    align-content: start;
  }

  .card-skeleton {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-6);
    min-height: 200px;
  }

  .empty-wrap {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Card internals */
  .ws-name {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ws-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .ws-desc {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .ws-stats {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .stat-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
  }

  .stat-icon {
    display: flex;
    align-items: center;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .stat-val {
    font-weight: 700;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--color-text);
    min-width: 20px;
  }

  .stat-dash {
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    min-width: 20px;
  }

  .stat-label {
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
  }

  /* Budget bar */
  .budget-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .budget-label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .budget-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
  }

  .budget-pct {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .budget-unknown {
    color: var(--color-text-muted);
  }

  .budget-bar-track {
    height: 5px;
    background: var(--color-surface-elevated);
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .budget-bar-fill {
    height: 100%;
    border-radius: var(--radius-full);
    transition: width var(--transition-slow) ease, background var(--transition-slow);
  }

  /* Enter button in card footer */
  .enter-btn {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    color: var(--color-link);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    padding: 0;
    transition: color var(--transition-fast);
  }

  .enter-btn:hover {
    color: var(--color-link-hover);
  }

  .btn-secondary {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .btn-secondary:hover {
    border-color: var(--color-text-muted);
  }

  .enter-btn:focus-visible,
  .btn-secondary:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .error-banner {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-6);
    text-align: center;
    color: var(--color-danger);
  }

  .error-banner p {
    margin: 0;
    font-size: var(--text-sm);
  }

  .retry-btn {
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }

  .retry-btn:hover {
    border-color: var(--color-text-muted);
  }

  .retry-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .budget-bar-fill { transition: none; }
    .ws-filter,
    .enter-btn,
    .btn-secondary,
    .retry-btn {
      transition: none;
    }
  }
</style>
