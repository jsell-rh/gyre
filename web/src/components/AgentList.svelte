<script>
  import { api } from '../lib/api.js';
  import StatusBadge from './StatusBadge.svelte';

  let agents = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filter = $state('');
  let selected = $state(null);

  const statuses = ['Idle', 'Active', 'Blocked', 'Error', 'Dead'];
  const filtered = $derived(filter ? agents.filter((a) => a.status === filter) : agents);

  function formatTime(ts) {
    if (!ts) return '—';
    return new Date(ts * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  $effect(() => {
    api.agents()
      .then((data) => { agents = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
  });

  function selectAgent(a) {
    selected = selected?.id === a.id ? null : a;
  }
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Agents</h2>
    <div class="controls">
      <select bind:value={filter}>
        <option value="">All statuses</option>
        {#each statuses as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </div>
  </div>

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else if filtered.length === 0}
    <p class="state-msg muted">No agents found.</p>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>Status</th>
            <th>Task</th>
            <th>Last Heartbeat</th>
            <th>Spawned</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as a}
            <tr class:selected={selected?.id === a.id} onclick={() => selectAgent(a)}>
              <td class="name">{a.name}</td>
              <td><StatusBadge value={a.status} /></td>
              <td class="dim">{a.current_task_id ?? '—'}</td>
              <td class="dim">{formatTime(a.last_heartbeat)}</td>
              <td class="dim">{formatTime(a.spawned_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>

      {#if selected}
        <div class="detail">
          <h3>Agent Detail: {selected.name}</h3>
          <dl>
            <dt>ID</dt><dd>{selected.id}</dd>
            <dt>Status</dt><dd><StatusBadge value={selected.status} /></dd>
            <dt>Parent</dt><dd>{selected.parent_id ?? '—'}</dd>
            <dt>Current Task</dt><dd>{selected.current_task_id ?? '—'}</dd>
            <dt>Budget (s)</dt><dd>{selected.lifetime_budget_secs ?? '—'}</dd>
            <dt>Spawned</dt><dd>{formatTime(selected.spawned_at)}</dd>
            <dt>Last Heartbeat</dt><dd>{formatTime(selected.last_heartbeat)}</dd>
          </dl>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }
  h3 { margin: 0 0 0.75rem; font-size: 0.9rem; color: var(--text); }

  select {
    background: var(--surface); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.3rem 0.6rem; font-size: 0.82rem; cursor: pointer;
  }

  .scroll { flex: 1; overflow-y: auto; padding: 0.75rem 1.25rem; }

  table { width: 100%; border-collapse: collapse; font-size: 0.85rem; }

  th {
    text-align: left; padding: 0.4rem 0.6rem;
    color: var(--text-dim); font-weight: 500; font-size: 0.78rem;
    border-bottom: 1px solid var(--border); text-transform: uppercase; letter-spacing: 0.04em;
  }

  td { padding: 0.45rem 0.6rem; border-bottom: 1px solid var(--border-subtle); vertical-align: middle; }

  tr { cursor: pointer; transition: background 0.1s; }
  tr:hover { background: var(--surface-hover); }
  tr.selected { background: var(--accent-muted); }

  .name { color: var(--text); font-weight: 500; }
  .dim { color: var(--text-muted); font-size: 0.82rem; }

  .detail {
    margin-top: 1.5rem; padding: 1rem; background: var(--surface);
    border: 1px solid var(--border); border-radius: 6px;
  }

  dl { display: grid; grid-template-columns: 8rem 1fr; gap: 0.35rem 0.75rem; font-size: 0.85rem; }
  dt { color: var(--text-dim); }
  dd { margin: 0; color: var(--text-muted); }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
