<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';

  let { wsStore } = $props();

  let events = $state([]);
  let activeFilters = $state(new Set());
  let loading = $state(true);
  let error = $state(null);

  const eventTypes = $derived([...new Set(events.map((e) => e.event_type))].sort());

  const filtered = $derived(
    activeFilters.size === 0
      ? events
      : events.filter((e) => activeFilters.has(e.event_type))
  );

  function toggleFilter(type) {
    const next = new Set(activeFilters);
    if (next.has(type)) next.delete(type);
    else next.add(type);
    activeFilters = next;
  }

  function relativeTime(ts) {
    const diff = Date.now() - new Date(ts).getTime();
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return `${secs}s ago`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    if (hrs < 24) return `${hrs}h ago`;
    return `${Math.floor(hrs / 24)}d ago`;
  }

  function eventIcon(type) {
    if (type?.startsWith('Agent')) return '⚡';
    if (type?.startsWith('Task')) return '✓';
    if (type?.startsWith('Mr') || type?.startsWith('Merge')) return '⌥';
    if (type?.startsWith('Queue')) return '⏳';
    return '◉';
  }

  function eventVariant(type) {
    if (type?.startsWith('Agent')) return 'info';
    if (type?.startsWith('Task')) return 'success';
    if (type?.startsWith('Mr') || type?.startsWith('Merge')) return 'purple';
    if (type?.includes('Error') || type?.includes('Failed')) return 'danger';
    return 'muted';
  }

  function loadActivity() {
    loading = true;
    error = null;
    api.activity(200)
      .then((data) => { events = data; loading = false; })
      .catch((err) => { error = err.message; loading = false; });
  }

  $effect(() => {
    loadActivity();
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

<div class="page">
  <div class="page-hdr">
    <div>
      <h1 class="page-title" id="activity-title">Activity Feed</h1>
      <p class="page-desc">Real-time event stream from agents and system components</p>
    </div>
  </div>

  {#if eventTypes.length > 0}
    <div class="filter-bar" role="group" aria-label="Filter events by type">
      <button
        class="pill"
        class:active={activeFilters.size === 0}
        onclick={() => (activeFilters = new Set())}
        aria-pressed={activeFilters.size === 0}
      >
        All
      </button>
      {#each eventTypes as type}
        <button
          class="pill"
          class:active={activeFilters.has(type)}
          onclick={() => toggleFilter(type)}
          aria-pressed={activeFilters.has(type)}
        >
          {type}
        </button>
      {/each}
    </div>
  {/if}

  {#if loading}
    <div class="timeline" aria-busy="true" aria-label="Loading activity feed">
      {#each Array(6) as _}
        <div class="timeline-item">
          <div class="timeline-node">
            <div class="skeleton-dot"></div>
            <div class="timeline-line"></div>
          </div>
          <div class="timeline-content">
            <div class="skel-row">
              <Skeleton width="80px" height="1.1rem" />
              <Skeleton width="100px" height="0.875rem" />
            </div>
            <Skeleton lines={2} height="0.875rem" />
          </div>
        </div>
      {/each}
    </div>
  {:else if error}
    <div class="error-msg" role="alert">
      <p>Error: {error}</p>
      <button class="btn-retry" onclick={() => { loadActivity(); }}>Retry</button>
    </div>
  {:else if filtered.length === 0}
    <EmptyState
      title="No events found"
      description={activeFilters.size > 0
        ? 'No events match the selected filters. Try clearing your filter.'
        : 'No activity events yet. Events will appear here as agents perform actions.'}
    />
  {:else}
    <div class="timeline" aria-live="polite" aria-labelledby="activity-title">
      {#each filtered as e (e.event_id)}
        <article class="timeline-item">
          <div class="timeline-node" aria-hidden="true">
            <div class="node-dot node-{eventVariant(e.event_type)}">{eventIcon(e.event_type)}</div>
            <div class="timeline-line"></div>
          </div>
          <div class="timeline-content">
            <div class="event-header">
              <Badge value={e.event_type} variant={eventVariant(e.event_type)} />
              <span class="agent-name">{e.agent_id ?? 'system'}</span>
              <time class="timestamp" datetime={e.timestamp}>{relativeTime(e.timestamp)}</time>
            </div>
            <p class="event-desc">{e.description}</p>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
  }

  .page-hdr { flex-shrink: 0; }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    margin-bottom: var(--space-1);
  }

  .page-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .filter-bar {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-full);
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .pill:hover { border-color: var(--color-border-strong); color: var(--color-text); }
  .pill.active { background: color-mix(in srgb, var(--color-primary) 12%, transparent); border-color: var(--color-primary); color: var(--color-primary); }

  .timeline {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .timeline-item {
    display: flex;
    gap: var(--space-4);
  }

  .timeline-node {
    display: flex;
    flex-direction: column;
    align-items: center;
    flex-shrink: 0;
    width: 32px;
  }

  .node-dot {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-xs);
    flex-shrink: 0;
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .node-success { border-color: color-mix(in srgb, var(--color-success) 40%, transparent);  color: var(--color-success); background: color-mix(in srgb, var(--color-success) 10%, transparent); }
  .node-warning { border-color: color-mix(in srgb, var(--color-warning) 40%, transparent); color: var(--color-warning); background: color-mix(in srgb, var(--color-warning) 10%, transparent); }
  .node-danger  { border-color: color-mix(in srgb, var(--color-danger) 40%, transparent);  color: var(--color-danger); background: color-mix(in srgb, var(--color-danger) 10%, transparent); }
  .node-info    { border-color: color-mix(in srgb, var(--color-info) 40%, transparent);  color: var(--color-info); background: color-mix(in srgb, var(--color-info) 10%, transparent); }
  .node-purple  { border-color: color-mix(in srgb, var(--color-blocked) 40%, transparent);  color: var(--color-blocked); background: color-mix(in srgb, var(--color-blocked) 10%, transparent); }

  .skeleton-dot {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--color-surface-elevated);
    flex-shrink: 0;
  }

  .timeline-line {
    flex: 1;
    width: 1px;
    background: var(--color-border);
    margin: 2px 0;
    min-height: var(--space-4);
  }

  .timeline-item:last-child .timeline-line { display: none; }

  .timeline-content {
    flex: 1;
    padding-bottom: var(--space-4);
    padding-top: var(--space-1);
  }

  .event-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
    flex-wrap: wrap;
  }

  .skel-row {
    display: flex;
    gap: var(--space-3);
    align-items: center;
    margin-bottom: var(--space-2);
  }

  .agent-name {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-link);
    font-weight: 500;
  }

  .timestamp {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-left: auto;
    white-space: nowrap;
    font-family: var(--font-mono);
  }

  .event-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.4;
    margin: 0;
  }

  .pill:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }

  .btn-retry {
    margin-top: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  .btn-retry:hover { background: color-mix(in srgb, var(--color-border) 30%, var(--color-surface-elevated)); border-color: var(--color-border-strong); }
  .btn-retry:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  @media (prefers-reduced-motion: reduce) {
    .pill,
    .btn-retry { transition: none; }
  }
</style>
