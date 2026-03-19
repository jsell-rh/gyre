<script>
  import { api } from '../lib/api.js';
  import StatusBadge from './StatusBadge.svelte';

  let { wsStore } = $props();

  let events = $state([]);
  let filter = $state('');
  let loading = $state(true);
  let error = $state(null);
  let feedEl = $state(null);

  const eventTypes = $derived([...new Set(events.map((e) => e.event_type))].sort());
  const filtered = $derived(filter ? events.filter((e) => e.event_type === filter) : events);

  function formatTs(ts) {
    return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  function formatDate(ts) {
    return new Date(ts).toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  $effect(() => {
    api.activity(200)
      .then((data) => {
        events = data;
        loading = false;
      })
      .catch((err) => {
        error = err.message;
        loading = false;
      });
  });

  $effect(() => {
    if (!wsStore) return;
    return wsStore.onMessage((msg) => {
      if (msg.type === 'ActivityEvent') {
        events = [msg, ...events].slice(0, 500);
      }
    });
  });
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Activity Feed</h2>
    <div class="controls">
      <select bind:value={filter}>
        <option value="">All types</option>
        {#each eventTypes as t}
          <option value={t}>{t}</option>
        {/each}
      </select>
    </div>
  </div>

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else if filtered.length === 0}
    <p class="state-msg muted">No activity yet.</p>
  {:else}
    <div class="feed" bind:this={feedEl}>
      {#each filtered as e (e.event_id)}
        <div class="entry">
          <span class="ts" title={formatDate(e.timestamp)}>{formatTs(e.timestamp)}</span>
          <StatusBadge value={e.event_type} />
          <span class="agent">{e.agent_id}</span>
          <span class="desc">{e.description}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem 1.25rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }

  select {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.3rem 0.6rem;
    font-size: 0.82rem;
    cursor: pointer;
  }

  .feed {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .entry {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    padding: 0.35rem 0.6rem;
    border-radius: 4px;
    font-size: 0.85rem;
    transition: background 0.1s;
  }

  .entry:hover { background: var(--surface-hover); }

  .ts {
    color: var(--text-dim);
    font-size: 0.78rem;
    white-space: nowrap;
    font-family: 'Courier New', monospace;
    min-width: 7rem;
  }

  .agent {
    color: var(--accent);
    font-weight: 500;
    white-space: nowrap;
    min-width: 6rem;
  }

  .desc {
    color: var(--text-muted);
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
