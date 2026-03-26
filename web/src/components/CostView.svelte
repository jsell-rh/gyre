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

<div class="cost-view">
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
            <thead>
              <tr>
                <th>Agent ID</th>
                <th class="right">Total</th>
                <th>Bar</th>
                <th></th>
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
            <thead>
              <tr>
                <th>Type</th>
                <th class="right">Amount</th>
                <th>Currency</th>
                <th>Task</th>
                <th>Time</th>
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
    padding: 1.5rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    height: 100%;
    overflow: auto;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  h2 { font-size: 1.1rem; font-weight: 600; color: var(--color-text); }
  h3 { font-size: 0.95rem; font-weight: 600; color: var(--color-text); margin-bottom: 0.75rem; }

  .actions { display: flex; gap: 0.5rem; }

  button {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    color: var(--color-text);
    border-radius: 4px;
    padding: 0.3rem 0.6rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  button:hover { background: var(--color-surface-elevated); }
  button:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 2px; }

  .summary-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 1rem 1.5rem;
    display: flex;
    align-items: center;
    gap: 1.5rem;
  }

  .summary-label { color: var(--color-text-muted); font-size: 0.85rem; }
  .summary-amount { font-size: 1.8rem; font-weight: 700; color: var(--color-primary); font-family: monospace; }

  .panel {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 1rem;
    overflow: auto;
  }

  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left;
    padding: 0.4rem 0.5rem;
    color: var(--color-text-muted);
    font-size: 0.8rem;
    border-bottom: 1px solid var(--color-border);
  }
  th.right { text-align: right; }
  td { padding: 0.4rem 0.5rem; font-size: 0.85rem; }

  .agent-id, .cost-type { font-family: monospace; font-size: 0.82rem; color: var(--color-text); }
  .right { text-align: right; }
  .amount { color: var(--color-primary); font-family: monospace; }
  .currency, .task-id, .time { color: var(--color-text-muted); font-size: 0.8rem; }

  .bar-cell { width: 120px; }
  .bar { height: 10px; background: var(--color-primary); border-radius: 2px; min-width: 2px; }

  .detail-btn { font-size: 0.75rem; padding: 0.15rem 0.4rem; }

  .empty { color: var(--color-text-secondary); font-size: 0.85rem; }
  .error { background: color-mix(in srgb, var(--color-danger) 10%, transparent); border: 1px solid var(--color-danger); color: var(--color-danger); border-radius: 6px; padding: 0.75rem; display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; }
  .error p { margin: 0; }
  .retry-btn {
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: 4px;
    color: var(--color-primary);
    cursor: pointer;
    font-size: 0.75rem;
    font-weight: 500;
    padding: 0.25rem 0.75rem;
    white-space: nowrap;
  }
  .retry-btn:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
    border-color: var(--color-primary);
  }
  .retry-btn:focus-visible { outline: 2px solid var(--color-primary); outline-offset: 2px; }
</style>
