<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';

  let events = $state([]);
  let stats = $state(null);
  let loading = $state(true);
  let error = $state(null);
  let filterType = $state('');
  let filterAgent = $state('');
  let liveEvents = $state([]);
  let sseConnected = $state(false);
  let activeTab = $state('live');

  const EVENT_TYPES = ['FileAccess', 'NetworkConnect', 'ProcessSpawn', 'SyscallDenied', 'CapabilityRaised', 'ContainerEscape', 'container_started', 'container_stopped', 'container_crashed', 'container_oom', 'container_network_blocked'];

  const TYPE_COLORS = {
    FileAccess: 'info',
    NetworkConnect: 'warning',
    ProcessSpawn: 'neutral',
    SyscallDenied: 'danger',
    CapabilityRaised: 'danger',
    ContainerEscape: 'danger',
    container_started: 'success',
    container_stopped: 'info',
    container_crashed: 'danger',
    container_oom: 'danger',
    container_network_blocked: 'warning',
  };

  async function loadHistory() {
    loading = true;
    error = null;
    try {
      const params = {};
      if (filterType) params.event_type = filterType;
      if (filterAgent) params.agent_id = filterAgent;
      const raw = await api.auditEvents(params);
      events = Array.isArray(raw) ? raw : (raw?.events ?? []);
      stats = await api.auditStats().catch(() => null);
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }

  $effect(() => {
    loadHistory();
  });

  let abortCtrl = $state(null);

  async function connectSSE() {
    disconnectSSE();
    const token = localStorage.getItem('gyre_auth_token') || 'gyre-dev-token';
    const ctrl = new AbortController();
    abortCtrl = ctrl;
    try {
      const resp = await fetch('/api/v1/audit/stream', {
        headers: { 'Authorization': `Bearer ${token}` },
        signal: ctrl.signal,
      });
      if (!resp.ok) { sseConnected = false; return; }
      sseConnected = true;
      const reader = resp.body.getReader();
      const decoder = new TextDecoder();
      let buf = '';
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop() || '';
        for (const line of lines) {
          if (line.startsWith('data:')) {
            try {
              const evt = JSON.parse(line.slice(5).trim());
              liveEvents = [evt, ...liveEvents].slice(0, 200);
            } catch { /* ignore parse errors */ }
          }
        }
      }
    } catch (e) {
      if (e.name !== 'AbortError') sseConnected = false;
    }
  }

  function disconnectSSE() {
    if (abortCtrl) { abortCtrl.abort(); abortCtrl = null; }
    sseConnected = false;
  }

  $effect(() => {
    if (activeTab === 'live') {
      connectSSE();
    } else {
      disconnectSSE();
    }
    return () => disconnectSSE();
  });

  function fmtDate(ts) {
    if (!ts) return '—';
    return new Date(typeof ts === 'number' ? ts * 1000 : ts).toLocaleString();
  }

  function typeColor(t) {
    return TYPE_COLORS[t] ?? 'neutral';
  }
</script>

<div class="audit-view" aria-busy={loading}>
  <span class="sr-only" aria-live="polite">{loading ? "" : "audit view loaded"}</span>
  <div class="view-header">
    <div class="header-left">
      <h2>Audit Events</h2>
      <p class="header-desc">eBPF-level security audit log (M7.1). Live stream + historical query.</p>
    </div>
    {#if stats}
      <div class="stats-row">
        {#each Object.entries(stats) as [k, v]}
          <div class="stat-card">
            <span class="stat-val">{v}</span>
            <span class="stat-label">{k}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <div class="tab-bar" role="tablist" aria-label="Audit view tabs"
    onkeydown={(e) => {
      const tabs = ['live', 'history'];
      const ids = ['tab-live', 'tab-history'];
      const idx = tabs.indexOf(activeTab);
      if (e.key === 'ArrowRight') { e.preventDefault(); const ni = (idx + 1) % 2; activeTab = tabs[ni]; document.getElementById(ids[ni])?.focus(); }
      if (e.key === 'ArrowLeft')  { e.preventDefault(); const ni = (idx + 1) % 2; activeTab = tabs[ni]; document.getElementById(ids[ni])?.focus(); }
    }}
  >
    <button class="tab-btn" role="tab" id="tab-live" aria-selected={activeTab === 'live'} aria-controls="panel-live" tabindex={activeTab === 'live' ? 0 : -1} class:active={activeTab === 'live'} onclick={() => (activeTab = 'live')}>
      <span class="live-dot" class:connected={sseConnected} aria-hidden="true"></span>
      Live Stream
    </button>
    <button class="tab-btn" role="tab" id="tab-history" aria-selected={activeTab === 'history'} aria-controls="panel-history" tabindex={activeTab === 'history' ? 0 : -1} class:active={activeTab === 'history'} onclick={() => (activeTab = 'history')}>
      History
    </button>
  </div>

  {#if activeTab === 'live'}
    <div id="panel-live" role="tabpanel" aria-labelledby="tab-live" class="tab-panel">
      <div class="live-header">
        <span class="sse-status">
          SSE:
          {#if sseConnected}
            <Badge value="Connected" color="success" />
          {:else}
            <Badge value="Disconnected" color="danger" />
          {/if}
        </span>
        <button class="clear-btn" onclick={() => (liveEvents = [])}>Clear</button>
      </div>

      <div class="event-feed">
        {#if liveEvents.length === 0}
          <EmptyState
            title="Waiting for events"
            description="eBPF audit events will appear here in real time."
          />
        {:else}
          {#each liveEvents as evt (evt.id ?? Math.random())}
            <div class="event-row">
              <Badge value={evt.event_type ?? evt.type ?? 'Unknown'} color={typeColor(evt.event_type ?? evt.type)} />
              <span class="event-agent">{evt.agent_id ?? '—'}</span>
              <span class="event-detail">{evt.details ?? evt.message ?? JSON.stringify(evt)}</span>
              <span class="event-time">{fmtDate(evt.timestamp ?? evt.created_at)}</span>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {:else}
    <!-- History tab -->
    <div id="panel-history" role="tabpanel" aria-labelledby="tab-history" class="tab-panel">
      <div class="filter-bar">
        <select class="filter-select" bind:value={filterType} aria-label="Filter by event type">
          <option value="">All event types</option>
          {#each EVENT_TYPES as t}
            <option value={t}>{t}</option>
          {/each}
        </select>
        <input
          class="filter-input"
          type="text"
          placeholder="Filter by agent ID"
          bind:value={filterAgent}
          onkeydown={(e) => e.key === 'Enter' && loadHistory()}
          aria-label="Filter by agent ID"
        />
        <button class="search-btn" onclick={loadHistory}>Search</button>
      </div>

      <div class="event-feed">
        {#if loading}
          <div class="skeleton-rows">
            {#each Array(8) as _}
              <Skeleton width="100%" height="2.2rem" />
            {/each}
          </div>
        {:else if error}
          <EmptyState title="Failed to load audit events" description={error} />
        {:else if events.length === 0}
          <EmptyState
            title="No audit events"
            description="Audit events appear here once agents are active. Enable GYRE_AUDIT_SIMULATE=true to generate demo events."
          />
        {:else}
          {#each events as evt (evt.id)}
            <div class="event-row">
              <Badge value={evt.event_type ?? evt.type ?? 'Unknown'} color={typeColor(evt.event_type ?? evt.type)} />
              <span class="event-agent">{evt.agent_id ?? '—'}</span>
              <span class="event-detail">{evt.details ?? evt.message ?? ''}</span>
              <span class="event-time">{fmtDate(evt.timestamp ?? evt.created_at)}</span>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .audit-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
  }

  .view-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-shrink: 0;
  }

  .header-left { display: flex; flex-direction: column; gap: var(--space-1); }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .header-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .stats-row {
    display: flex;
    gap: var(--space-3);
    flex-shrink: 0;
  }

  .stat-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-4);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
  }

  .stat-val {
    font-family: var(--font-mono);
    font-size: var(--text-lg);
    font-weight: 700;
    color: var(--color-text);
  }

  .stat-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: capitalize;
  }

  .tab-bar {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
    font-weight: 500;
  }

  .tab-btn:hover:not(.active) { color: var(--color-text-secondary); }

  .live-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--color-text-muted);
    flex-shrink: 0;
  }

  .live-dot.connected {
    background: var(--color-success);
    box-shadow: 0 0 5px color-mix(in srgb, var(--color-success) 60%, transparent);
    animation: pulse 2s infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .live-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }

  .sse-status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--color-text-muted);
  }

  .clear-btn {
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .clear-btn:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text-secondary);
  }

  .tab-panel {
    display: contents;
  }

  .filter-bar {
    display: flex;
    gap: var(--space-2);
    flex-shrink: 0;
  }

  .filter-select,
  .filter-input {
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    transition: border-color var(--transition-fast);
  }

  .filter-select { width: 180px; }
  .filter-input { flex: 1; }

  .filter-select:focus:not(:focus-visible),
  .filter-input:focus:not(:focus-visible) {
    outline: none;
    border-color: var(--color-primary);
  }
  .filter-select:focus-visible,
  .filter-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .search-btn {
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .search-btn:hover {
    border-color: var(--color-text-muted);
    color: var(--color-text);
  }

  .event-feed {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .skeleton-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .event-row {
    display: grid;
    grid-template-columns: 140px 120px 1fr auto;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .event-agent {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .event-detail {
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .event-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    font-family: var(--font-mono);
  }

  .tab-btn:focus-visible,
  .clear-btn:focus-visible,
  .search-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .live-dot { animation: none; }
    .tab-btn,
    .clear-btn,
    .search-btn,
    .filter-select,
    .filter-input { transition: none; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
