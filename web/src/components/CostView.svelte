<script>
  import { onMount } from 'svelte';
  import Skeleton from '../lib/Skeleton.svelte';

  let costs = $state([]);
  let summary = $state(null);
  let agentFilter = $state('');
  let loading = $state(true);
  let error = $state(null);

  // Compute per-agent totals from loaded costs
  let agentTotals = $derived(
    Object.entries(
      costs.reduce((acc, c) => {
        acc[c.agent_id] = (acc[c.agent_id] || 0) + c.amount;
        return acc;
      }, {})
    ).sort((a, b) => b[1] - a[1])
  );

  async function load() {
    loading = true;
    error = null;
    try {
      const since = 0;
      const until = Math.floor(Date.now() / 1000) + 86400;

      // Load summary
      const sumRes = await fetch(`/api/v1/costs/summary?since=${since}&until=${until}`);
      if (sumRes.ok) summary = await sumRes.json();

      // Load costs
      if (agentFilter) {
        const res = await fetch(`/api/v1/costs?agent_id=${encodeURIComponent(agentFilter)}`);
        if (!res.ok) throw new Error(await res.text());
        costs = await res.json();
      } else {
        // Load all by querying a known set — we'll use the agentTotals workaround:
        // Query with a broad time range via summary, then show agent table from summary
        costs = [];
      }
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadAgent(agentId) {
    agentFilter = agentId;
    try {
      const res = await fetch(`/api/v1/costs?agent_id=${encodeURIComponent(agentId)}`);
      if (!res.ok) throw new Error(await res.text());
      costs = await res.json();
    } catch (e) {
      error = e.message;
    }
  }

  function fmtTime(ts) {
    return new Date(ts * 1000).toLocaleString();
  }

  function fmtAmount(n) {
    return n.toLocaleString(undefined, { maximumFractionDigits: 2 });
  }

  onMount(() => load());
</script>

<div class="cost-view" aria-busy={loading}>
  <span class="sr-only" aria-live="polite">{loading ? "" : "cost data loaded"}</span>
  <div class="toolbar">
    <h2>Cost Tracking</h2>
    <div class="actions">
      {#if agentFilter}
        <button onclick={() => { agentFilter = ''; costs = []; load(); }}>← All Agents</button>
      {/if}
      <button onclick={load}>Refresh</button>
    </div>
  </div>

  {#if error}
    <div class="error" role="alert">
      <p>{error}</p>
      <button onclick={load} class="retry-btn">Retry</button>
    </div>
  {:else if loading}
    <Skeleton lines={5} />
  {:else}
    {#if summary}
      <div class="summary-card">
        <div class="summary-label">Total Cost (all time)</div>
        <div class="summary-amount">{fmtAmount(summary.total)}</div>
      </div>
    {/if}

    {#if !agentFilter}
      <!-- Agent breakdown table -->
      <div class="panel">
        <h3>Cost by Agent</h3>
        {#if agentTotals.length === 0}
          <p class="empty">No cost entries recorded yet.</p>
        {:else}
          <table>
            <caption class="sr-only">Cost by agent</caption>
            <thead>
              <tr>
                <th scope="col">Agent ID</th>
                <th scope="col" class="right">Total</th>
                <th scope="col">Bar</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each agentTotals as [agentId, total]}
                {@const max = agentTotals[0][1]}
                <tr>
                  <td class="agent-id">{agentId}</td>
                  <td class="right amount">{fmtAmount(total)}</td>
                  <td class="bar-cell">
                    <div class="bar" style="width: {Math.round((total / max) * 100)}%"></div>
                  </td>
                  <td>
                    <button class="detail-btn" onclick={() => loadAgent(agentId)}>Details</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {:else}
      <!-- Per-agent cost entries -->
      <div class="panel">
        <h3>Entries for {agentFilter}</h3>
        {#if costs.length === 0}
          <p class="empty">No cost entries for this agent.</p>
        {:else}
          <table>
            <caption class="sr-only">Cost entries for agent</caption>
            <thead>
              <tr>
                <th scope="col">Type</th>
                <th scope="col" class="right">Amount</th>
                <th scope="col">Currency</th>
                <th scope="col">Task</th>
                <th scope="col">Time</th>
              </tr>
            </thead>
            <tbody>
              {#each costs as c}
                <tr>
                  <td class="cost-type">{c.cost_type}</td>
                  <td class="right amount">{fmtAmount(c.amount)}</td>
                  <td class="currency">{c.currency}</td>
                  <td class="task-id">{c.task_id ?? '—'}</td>
                  <td class="time">{fmtTime(c.timestamp)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .cost-view {
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
    overflow: auto;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  h2 { font-size: var(--text-lg); font-weight: 600; color: var(--color-text); }
  h3 { font-size: var(--text-base); font-weight: 600; color: var(--color-text); margin-bottom: var(--space-3); }

  .actions { display: flex; gap: var(--space-2); }

  button {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    color: var(--color-text);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
    font-family: var(--font-body);
    transition: background var(--transition-fast);
  }
  button:hover { background: var(--color-surface-elevated); }
  button:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .summary-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4) var(--space-6);
    display: flex;
    align-items: center;
    gap: var(--space-6);
  }

  .summary-label { color: var(--color-text-muted); font-size: var(--text-sm); }
  .summary-amount { font-size: var(--text-3xl); font-weight: 700; color: var(--color-primary); font-family: var(--font-mono); }

  .panel {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    overflow: auto;
  }

  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left;
    padding: var(--space-1) var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    border-bottom: 1px solid var(--color-border);
  }
  th.right { text-align: right; }
  td { padding: var(--space-1) var(--space-2); font-size: var(--text-sm); }

  .agent-id, .cost-type { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--color-text); }
  .right { text-align: right; }
  .amount { color: var(--color-primary); font-family: var(--font-mono); }
  .currency, .task-id, .time { color: var(--color-text-muted); font-size: var(--text-xs); }

  .bar-cell { width: 120px; }
  .bar { height: 10px; background: var(--color-primary); border-radius: var(--radius-sm); min-width: 2px; }

  .detail-btn { font-size: var(--text-xs); padding: var(--space-1); }

  .empty { color: var(--color-text-secondary); font-size: var(--text-sm); }
  .error { background: color-mix(in srgb, var(--color-danger) 10%, transparent); border: 1px solid var(--color-danger); color: var(--color-danger); border-radius: var(--radius); padding: var(--space-3); display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); }
  .error p { margin: 0; }
  .retry-btn {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-primary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-weight: 500;
    padding: var(--space-1) var(--space-3);
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-color: var(--color-primary);
  }
  .retry-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    button, .retry-btn { transition: none; }
  }
</style>
