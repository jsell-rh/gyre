<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let summary = $state(null);
  let loading = $state(true);
  let refreshing = $state(false);

  $effect(() => { load(); });

  async function load() {
    if (summary) {
      refreshing = true;
    } else {
      loading = true;
    }
    try {
      summary = await api.budgetSummary();
    } catch (e) {
      showToast('Failed to load budget summary: ' + e.message, { type: 'error' });
    } finally {
      loading = false;
      refreshing = false;
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
</script>

<div class="budget-dash" aria-busy={loading}>
  <span class="sr-only" aria-live="polite">{loading ? "" : "budget data loaded"}</span>
  <div class="dash-header">
    <h2>Budget Dashboard</h2>
    <p class="subtitle">Tenant-wide token and cost consumption</p>
    <button class="btn-refresh" onclick={load} title="Refresh" aria-label="Refresh budget data" disabled={refreshing}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <path d="M1 4v6h6"/><path d="M23 20v-6h-6"/>
        <path d="M20.49 9A9 9 0 005.64 5.64L1 10m22 4l-4.64 4.36A9 9 0 013.51 15"/>
      </svg>
      {refreshing ? 'Refreshing\u2026' : 'Refresh'}
    </button>
  </div>

  {#if loading}
    <div class="content">
      <Skeleton lines={8} />
    </div>
  {:else if !summary}
    <EmptyState message="Budget data unavailable. Admin access may be required." />
  {:else}
    <div class="content">
      <!-- Tenant summary cards -->
      <section class="section">
        <h3 class="section-title">Tenant Summary</h3>
        <div class="stat-cards">
          <div class="stat-card">
            <div class="stat-label">Active Agents</div>
            <div class="stat-value">{summary.usage?.active_agents ?? 0}</div>
            {#if summary.config?.max_concurrent_agents}
              <div class="stat-limit">limit: {summary.config.max_concurrent_agents}</div>
            {/if}
          </div>
          <div class="stat-card">
            <div class="stat-label">Tokens Today</div>
            <div class="stat-value">{fmtNum(summary.usage?.tokens_used_today)}</div>
            {#if summary.config?.max_tokens_per_day}
              <div class="stat-limit">limit: {fmtNum(summary.config.max_tokens_per_day)}</div>
            {/if}
          </div>
          <div class="stat-card">
            <div class="stat-label">Cost Today</div>
            <div class="stat-value">{fmtCost(summary.usage?.cost_today)}</div>
            {#if summary.config?.max_cost_per_day}
              <div class="stat-limit">limit: {fmtCost(summary.config.max_cost_per_day)}</div>
            {/if}
          </div>
          <div class="stat-card">
            <div class="stat-label">Workspaces</div>
            <div class="stat-value">{summary.workspaces?.length ?? 0}</div>
          </div>
        </div>
      </section>

      <!-- Per-workspace breakdown -->
      {#if summary.workspaces?.length > 0}
        <section class="section">
          <h3 class="section-title">Per-Workspace Breakdown</h3>
          <div class="workspace-rows">
            {#each summary.workspaces as ws}
              <div class="ws-row">
                <div class="ws-row-header">
                  <span class="ws-name">{ws.name ?? ws.id}</span>
                  <span class="ws-agents">{ws.usage?.active_agents ?? 0} active agents</span>
                </div>
                {#if ws.config?.max_tokens_per_day}
                  {@const pt = pct(ws.usage?.tokens_used_today, ws.config.max_tokens_per_day)}
                  <div class="bar-row">
                    <span class="bar-lbl">Tokens</span>
                    <div class="bar-track">
                      <div class="bar-fill" class:bar-warn={pt > 75} class:bar-danger={pt > 90} style="width: {pt}%"></div>
                    </div>
                    <span class="bar-txt">{fmtNum(ws.usage?.tokens_used_today)} / {fmtNum(ws.config.max_tokens_per_day)}</span>
                  </div>
                {/if}
                {#if ws.config?.max_cost_per_day}
                  {@const pc = pct(ws.usage?.cost_today, ws.config.max_cost_per_day)}
                  <div class="bar-row">
                    <span class="bar-lbl">Cost</span>
                    <div class="bar-track">
                      <div class="bar-fill" class:bar-warn={pc > 75} class:bar-danger={pc > 90} style="width: {pc}%"></div>
                    </div>
                    <span class="bar-txt">{fmtCost(ws.usage?.cost_today)} / {fmtCost(ws.config.max_cost_per_day)}</span>
                  </div>
                {/if}
                {#if !ws.config?.max_tokens_per_day && !ws.config?.max_cost_per_day}
                  <span class="no-limits">No limits set</span>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/if}
    </div>
  {/if}
</div>

<style>
  .budget-dash { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .dash-header {
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
    padding: var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .dash-header h2 { margin: 0; font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .subtitle { margin: 0; font-size: var(--text-sm); color: var(--color-text-secondary); flex: 1; }

  .btn-refresh {
    display: flex; align-items: center; gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: color var(--transition-fast);
  }
  .btn-refresh:hover { color: var(--color-text); }
  .btn-refresh:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .content { flex: 1; overflow-y: auto; padding: var(--space-6); display: flex; flex-direction: column; gap: var(--space-8); }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-4);
  }

  .stat-cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: var(--space-4); }

  .stat-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
  }

  .stat-label { font-size: var(--text-xs); color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: var(--space-2); }
  .stat-value { font-size: var(--text-2xl); font-weight: 700; color: var(--color-text); font-family: var(--font-mono); }
  .stat-limit { font-size: var(--text-xs); color: var(--color-text-muted); margin-top: var(--space-1); }

  .workspace-rows { display: flex; flex-direction: column; gap: var(--space-4); max-width: 720px; }

  .ws-row {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .ws-row-header { display: flex; justify-content: space-between; align-items: baseline; }
  .ws-name { font-weight: 600; color: var(--color-text); font-size: var(--text-sm); }
  .ws-agents { font-size: var(--text-xs); color: var(--color-text-muted); }

  .bar-row { display: flex; align-items: center; gap: var(--space-2); }
  .bar-lbl { font-size: var(--text-xs); color: var(--color-text-muted); width: 48px; flex-shrink: 0; }
  .bar-track { flex: 1; height: 6px; background: var(--color-surface); border-radius: var(--radius-full); overflow: hidden; }
  .bar-fill { height: 100%; background: var(--color-primary); border-radius: var(--radius-full); transition: width var(--transition-slow); }
  .bar-fill.bar-warn { background: var(--color-warning); }
  .bar-fill.bar-danger { background: var(--color-danger); }
  .bar-txt { font-size: var(--text-xs); color: var(--color-text-muted); font-family: var(--font-mono); min-width: 120px; }
  .no-limits { font-size: var(--text-xs); color: var(--color-text-muted); }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .bar-fill, .btn-refresh { transition: none; }
  }
</style>
