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

<div class="analytics-view" aria-busy={loading}>
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

  <span class="sr-only" aria-live="polite">{loading ? '' : 'Analytics loaded'}</span>

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
              <tr><th scope="col">Event</th><th scope="col">Count</th><th scope="col">Bar</th></tr>
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
                <button class="ev-detail" onclick={() => selectedEvent = selectedEvent === ev.id ? '' : ev.id} aria-expanded={selectedEvent === ev.id} aria-label="{selectedEvent === ev.id ? 'Collapse' : 'Expand'} event details">
                  <span aria-hidden="true">{selectedEvent === ev.id ? '▲' : '▼'}</span>
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
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  h2 { font-size: var(--text-lg); font-weight: 600; color: var(--color-text); }
  h3 { font-size: var(--text-base); font-weight: 600; color: var(--color-text); margin-bottom: var(--space-3); }

  .filters { display: flex; gap: var(--space-2); align-items: center; }

  select, button {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    color: var(--color-text);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-2);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  button:hover { background: var(--color-surface-elevated); }

  select:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .panels {
    display: grid;
    grid-template-columns: 360px 1fr;
    gap: var(--space-4);
    min-height: 0;
  }

  .panel {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-4);
    overflow: auto;
  }

  .events-panel { overflow: auto; }

  table { width: 100%; border-collapse: collapse; }
  th { text-align: left; padding: var(--space-1) var(--space-2); color: var(--color-text-muted); font-size: var(--text-xs); border-bottom: 1px solid var(--color-border); }
  td { padding: var(--space-1) var(--space-2); font-size: var(--text-sm); }

  .event-name { font-family: var(--font-mono); color: var(--color-text); }
  .count { text-align: right; color: var(--color-text-muted); width: 50px; }

  .bar-cell { width: 120px; padding-left: var(--space-2); }
  .bar { height: 10px; background: var(--color-primary); border-radius: var(--radius-sm); min-width: 2px; transition: width var(--transition-normal); }

  .event-list { display: flex; flex-direction: column; gap: var(--space-1); }

  .event-row {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-bg, var(--color-surface));
    flex-wrap: wrap;
  }

  .ev-name { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--color-text); }
  .ev-agent { font-size: var(--text-xs); color: var(--color-text-muted); }
  .ev-time { font-size: var(--text-xs); color: var(--color-text-secondary); }
  .ev-detail { font-size: var(--text-xs); padding: var(--space-1); }
  .ev-props {
    grid-column: 1 / -1;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    overflow: auto;
    max-height: 200px;
  }

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

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0,0,0,0);
    white-space: nowrap;
    border: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .bar,
    select,
    button,
    .retry-btn { transition: none; }
  }
</style>
