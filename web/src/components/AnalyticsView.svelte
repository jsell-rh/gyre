<script>
  import { onMount } from 'svelte';
  import Skeleton from '../lib/Skeleton.svelte';

  let events = $state([]);
  let topEvents = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let selectedEvent = $state('');
  let eventFilter = $state('');

  const EVENT_NAMES = [
    'task.status_changed',
    'agent.spawned',
    'agent.completed',
    'mr.created',
    'mr.merged',
    'merge_queue.processed',
  ];

  async function load() {
    loading = true;
    error = null;
    try {
      const params = new URLSearchParams({ limit: '200' });
      if (eventFilter) params.set('event_name', eventFilter);
      const res = await fetch(`/api/v1/analytics/events?${params}`);
      if (!res.ok) throw new Error(await res.text());
      events = await res.json();
      computeTopEvents();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function computeTopEvents() {
    const counts = {};
    for (const ev of events) {
      counts[ev.event_name] = (counts[ev.event_name] || 0) + 1;
    }
    topEvents = Object.entries(counts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10);
  }

  function fmtTime(ts) {
    return new Date(ts * 1000).toLocaleString();
  }

  onMount(() => load());
</script>

<div class="analytics-view">
  <div class="toolbar">
    <h2>Analytics Events</h2>
    <div class="filters">
      <select bind:value={eventFilter} onchange={load} aria-label="Filter by event name">
        <option value="">All Events</option>
        {#each EVENT_NAMES as name}
          <option value={name}>{name}</option>
        {/each}
      </select>
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
    <div class="panels">
      <!-- Top Events Summary -->
      <div class="panel">
        <h3>Top Events</h3>
        {#if topEvents.length === 0}
          <p class="empty">No events recorded yet.</p>
        {:else}
          <table>
            <thead>
              <tr><th>Event</th><th>Count</th><th>Bar</th></tr>
            </thead>
            <tbody>
              {#each topEvents as [name, count]}
                {@const max = topEvents[0][1]}
                <tr>
                  <td class="event-name">{name}</td>
                  <td class="count">{count}</td>
                  <td class="bar-cell">
                    <div class="bar" style="width: {Math.round((count / max) * 100)}%"></div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

      <!-- Recent Events -->
      <div class="panel events-panel">
        <h3>Recent Events ({events.length})</h3>
        {#if events.length === 0}
          <p class="empty">No events match the current filter.</p>
        {:else}
          <div class="event-list">
            {#each events as ev}
              <div class="event-row">
                <span class="ev-name">{ev.event_name}</span>
                <span class="ev-agent">{ev.agent_id ?? '—'}</span>
                <span class="ev-time">{fmtTime(ev.timestamp)}</span>
                <button class="ev-detail" onclick={() => selectedEvent = selectedEvent === ev.id ? '' : ev.id}>
                  {selectedEvent === ev.id ? '▲' : '▼'}
                </button>
                {#if selectedEvent === ev.id}
                  <pre class="ev-props">{JSON.stringify(ev.properties, null, 2)}</pre>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .analytics-view {
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
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  h2 { font-size: 1.1rem; font-weight: 600; color: var(--color-text); }
  h3 { font-size: 0.95rem; font-weight: 600; color: var(--color-text); margin-bottom: 0.75rem; }

  .filters { display: flex; gap: 0.5rem; align-items: center; }

  select, button {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    color: var(--color-text);
    border-radius: 4px;
    padding: 0.3rem 0.6rem;
    font-size: 0.85rem;
    cursor: pointer;
  }

  button:hover { background: var(--color-surface-elevated); }

  .panels {
    display: grid;
    grid-template-columns: 360px 1fr;
    gap: 1rem;
    min-height: 0;
  }

  .panel {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 1rem;
    overflow: auto;
  }

  .events-panel { overflow: auto; }

  table { width: 100%; border-collapse: collapse; }
  th { text-align: left; padding: 0.4rem 0.5rem; color: var(--color-text-muted); font-size: 0.8rem; border-bottom: 1px solid var(--color-border); }
  td { padding: 0.4rem 0.5rem; font-size: 0.85rem; }

  .event-name { font-family: monospace; color: var(--color-primary); }
  .count { text-align: right; color: var(--color-text-muted); width: 50px; }

  .bar-cell { width: 120px; padding-left: 0.5rem; }
  .bar { height: 10px; background: var(--color-primary); border-radius: 2px; min-width: 2px; transition: width 0.3s; }

  .event-list { display: flex; flex-direction: column; gap: 0.25rem; }

  .event-row {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.5rem;
    border-radius: 4px;
    background: var(--color-bg, var(--color-surface));
    flex-wrap: wrap;
  }

  .ev-name { font-family: monospace; font-size: 0.82rem; color: var(--color-primary); }
  .ev-agent { font-size: 0.78rem; color: var(--color-text-muted); }
  .ev-time { font-size: 0.75rem; color: var(--color-text-secondary); }
  .ev-detail { font-size: 0.7rem; padding: 0.1rem 0.3rem; }
  .ev-props {
    grid-column: 1 / -1;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 4px;
    padding: 0.5rem;
    font-size: 0.78rem;
    color: var(--color-text-muted);
    overflow: auto;
    max-height: 200px;
  }

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
