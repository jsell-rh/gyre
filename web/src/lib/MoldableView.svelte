<script>
  import ExplorerCanvas from './ExplorerCanvas.svelte';
  import FlowRenderer from './FlowRenderer.svelte';
  import Badge from './Badge.svelte';
  import EmptyState from './EmptyState.svelte';
  import { api } from './api.js';

  let {
    nodes = [],
    edges = [],
    repoId = '',
    onSelectNode = undefined,
    conceptFilterIds = null,
    conceptQuery = '',
  } = $props();

  let activeView = $state('graph'); // 'graph' | 'list' | 'timeline' | 'flow'

  // List view sort
  let sortBy = $state('type'); // 'type' | 'name' | 'file'
  let filterType = $state('');

  let nodeTypes = $derived.by(() => {
    const types = new Set(nodes.map(n => n.node_type).filter(Boolean));
    return ['', ...Array.from(types).sort()];
  });

  // Nodes/edges visible in the current concept filter
  let displayNodes = $derived.by(() => {
    if (!conceptFilterIds) return nodes;
    return nodes.filter(n => conceptFilterIds.has(n.id));
  });

  let displayEdges = $derived.by(() => {
    if (!conceptFilterIds) return edges;
    return edges.filter(e => {
      const sid = e.source_id ?? e.from_node_id ?? e.from;
      const tid = e.target_id ?? e.to_node_id ?? e.to;
      return conceptFilterIds.has(sid) && conceptFilterIds.has(tid);
    });
  });

  let filteredNodes = $derived.by(() => {
    let result = conceptFilterIds
      ? nodes.filter(n => conceptFilterIds.has(n.id))
      : nodes;
    if (filterType) result = result.filter(n => n.node_type === filterType);
    return [...result].sort((a, b) => {
      if (sortBy === 'type') return (a.node_type ?? '').localeCompare(b.node_type ?? '');
      if (sortBy === 'name') return (a.name ?? '').localeCompare(b.name ?? '');
      if (sortBy === 'file') return (a.file_path ?? '').localeCompare(b.file_path ?? '');
      return 0;
    });
  });

  function typeVariant(type) {
    switch (type) {
      case 'package':   return 'info';
      case 'module':    return 'info';
      case 'type':      return 'success';
      case 'interface': return 'warning';
      case 'function':  return 'default';
      case 'endpoint':  return 'danger';
      case 'component': return 'info';
      case 'table':     return 'default';
      case 'constant':  return 'warning';
      default:          return 'default';
    }
  }

  // ── Timeline state ──────────────────────────────────────────────────────────
  let timelineDeltas = $state([]);
  let timelineLoading = $state(false);
  let scrubberValue = $state(Date.now());
  let selectedDelta = $state(null);

  // Timestamps (seconds epoch) for min/max from delta records
  let minTs = $derived.by(() => {
    if (timelineDeltas.length === 0) return Math.floor(Date.now() / 1000) - 86400;
    return Math.min(...timelineDeltas.map(d => d.timestamp));
  });

  let maxTs = $derived.by(() => Math.floor(Date.now() / 1000));

  // Deltas up to scrubber position
  let activeShas = $derived.by(() => {
    const cutoff = Math.floor(scrubberValue / 1000);
    const set = new Set();
    for (const d of timelineDeltas) {
      if (d.timestamp <= cutoff) set.add(d.commit_sha);
    }
    return set;
  });

  // Nodes whose last_modified_sha appears in a delta at or before scrubber
  let timelineNodes = $derived.by(() => {
    if (timelineDeltas.length === 0) return nodes;
    if (activeShas.size === 0) return [];
    return nodes.filter(n => n.last_modified_sha && activeShas.has(n.last_modified_sha));
  });

  // Deltas sorted by timestamp ascending (for markers)
  let sortedDeltas = $derived.by(() =>
    [...timelineDeltas].sort((a, b) => a.timestamp - b.timestamp)
  );

  // Load timeline when Timeline tab is active and repoId is set
  $effect(() => {
    if (activeView === 'timeline' && repoId) {
      fetchTimeline();
    }
  });

  async function fetchTimeline() {
    timelineLoading = true;
    timelineDeltas = [];
    selectedDelta = null;
    try {
      const data = await api.repoGraphTimeline(repoId);
      timelineDeltas = Array.isArray(data) ? data : [];
    } catch (_e) {
      timelineDeltas = [];
    } finally {
      timelineLoading = false;
      // Default scrubber to "Now"
      scrubberValue = Date.now();
    }
  }

  function resetToNow() {
    scrubberValue = Date.now();
    selectedDelta = null;
  }

  function onScrubberChange(e) {
    scrubberValue = Number(e.target.value);
    selectedDelta = null;
  }

  function selectDelta(delta) {
    selectedDelta = selectedDelta?.id === delta.id ? null : delta;
    // Snap scrubber to this delta's timestamp (in ms)
    scrubberValue = delta.timestamp * 1000;
  }

  function relativeTime(epochSeconds) {
    const diff = Math.floor(Date.now() / 1000) - epochSeconds;
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function deltaJsonSummary(delta_json) {
    if (!delta_json) return [];
    let obj;
    try { obj = typeof delta_json === 'string' ? JSON.parse(delta_json) : delta_json; }
    catch (_) { return [String(delta_json)]; }
    return Object.keys(obj).slice(0, 6);
  }

  function markerLeft(ts) {
    const min = minTs;
    const max = maxTs;
    if (max === min) return '50%';
    return `${((ts - min) / (max - min)) * 100}%`;
  }
</script>

<div class="moldable-view">
  <!-- View switcher tabs -->
  <div class="view-tabs" role="tablist" aria-label="View mode">
    <button
      class="view-tab"
      class:active={activeView === 'graph'}
      role="tab"
      aria-selected={activeView === 'graph'}
      onclick={() => (activeView = 'graph')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/>
        <path d="M7 12h10M17 7l-10 4M17 17L7 13"/>
      </svg>
      Graph
    </button>
    <button
      class="view-tab"
      class:active={activeView === 'list'}
      role="tab"
      aria-selected={activeView === 'list'}
      onclick={() => (activeView = 'list')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/>
        <line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>
      </svg>
      List
    </button>
    <button
      class="view-tab"
      class:active={activeView === 'timeline'}
      role="tab"
      aria-selected={activeView === 'timeline'}
      onclick={() => (activeView = 'timeline')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/>
      </svg>
      Timeline
    </button>
    <button
      class="view-tab"
      class:active={activeView === 'flow'}
      role="tab"
      aria-selected={activeView === 'flow'}
      onclick={() => (activeView = 'flow')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/>
        <path d="M7 11.5l9-5M7 12.5l9 5"/>
      </svg>
      Flow
    </button>
  </div>

  <!-- View content -->
  <div class="view-content">
    {#if activeView === 'graph'}
      <ExplorerCanvas nodes={displayNodes} edges={displayEdges} {repoId} {onSelectNode} />

    {:else if activeView === 'list'}
      <div class="list-view">
        <div class="list-controls">
          <select
            class="filter-select"
            value={filterType}
            onchange={(e) => (filterType = e.target.value)}
            aria-label="Filter by node type"
          >
            {#each nodeTypes as t}
              <option value={t}>{t || 'All types'}</option>
            {/each}
          </select>
          <div class="sort-group" role="group" aria-label="Sort by">
            <span class="sort-label">Sort:</span>
            {#each [['type', 'Type'], ['name', 'Name'], ['file', 'File']] as [val, label]}
              <button
                class="sort-btn"
                class:active={sortBy === val}
                onclick={() => (sortBy = val)}
              >{label}</button>
            {/each}
          </div>
          <span class="list-count">{filteredNodes.length} nodes</span>
        </div>

        <div class="list-table-wrap">
          {#if filteredNodes.length === 0 && conceptFilterIds}
            <div class="list-empty-concept">
              <EmptyState
                title="No nodes matching '{conceptQuery}'"
                message="Try a shorter term or check spelling."
              />
            </div>
          {:else if filteredNodes.length === 0 && !conceptFilterIds}
            <div class="list-empty-concept">
              <EmptyState
                title="No graph data yet"
                message="Push code to trigger graph extraction."
              />
            </div>
          {:else}
            <table class="list-table">
              <thead>
                <tr>
                  <th>Type</th>
                  <th>Name</th>
                  <th>File</th>
                  <th>Spec</th>
                  <th>Churn</th>
                </tr>
              </thead>
              <tbody>
                {#each filteredNodes as node}
                  <tr
                    class="list-row"
                    role="button"
                    tabindex="0"
                    aria-label="Select node {node.name}"
                    onclick={() => onSelectNode?.(node)}
                    onkeydown={(e) => e.key === 'Enter' && onSelectNode?.(node)}
                  >
                    <td><Badge variant={typeVariant(node.node_type)} value={node.node_type ?? '?'} /></td>
                    <td class="mono">{node.name}</td>
                    <td class="mono muted">{node.file_path ?? ''}{node.line_start ? `:${node.line_start}` : ''}</td>
                    <td>
                      {#if node.spec_path}
                        <span class="spec-tag">{node.spec_path.split('/').pop()}</span>
                      {:else}
                        <span class="muted-dash">—</span>
                      {/if}
                    </td>
                    <td class="mono">{node.churn_count_30d ?? 0}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      </div>

    {:else if activeView === 'flow'}
      <FlowRenderer
        nodes={displayNodes}
        edges={displayEdges}
        {repoId}
        spans={[]}
      />

    {:else if activeView === 'timeline'}
      <div class="timeline-view">
        <div class="timeline-header">
          <h3 class="timeline-title">Architectural Timeline</h3>
          {#if !repoId}
            <p class="timeline-hint">Select a repository to view architectural history.</p>
          {:else if timelineLoading}
            <p class="timeline-hint">Loading timeline…</p>
          {:else if timelineDeltas.length > 0}
            <span class="timeline-meta">
              {timelineDeltas.length} delta{timelineDeltas.length !== 1 ? 's' : ''}
              · {timelineNodes.length} / {nodes.length} nodes visible
            </span>
          {/if}
          {#if repoId && !timelineLoading && timelineDeltas.length > 0}
            <button class="now-btn" onclick={resetToNow} title="Jump to now">Now</button>
          {/if}
        </div>

        {#if !repoId || (!timelineLoading && timelineDeltas.length === 0)}
          <div class="timeline-empty">
            <EmptyState
              title="No architectural changes recorded yet."
              message={repoId ? "Push code to trigger graph extraction and populate the timeline." : "Select a repository above to view its architectural timeline."}
            />
          </div>
        {:else if timelineLoading}
          <div class="timeline-empty">
            <div class="timeline-spinner" aria-label="Loading"></div>
          </div>
        {:else}
          <!-- Node list filtered by scrubber position -->
          <div class="timeline-nodes-area">
            {#if timelineNodes.length === 0}
              <div class="timeline-empty-inner">
                <p class="no-nodes-msg">No nodes modified up to this point in history.</p>
              </div>
            {:else}
              <div class="timeline-node-list">
                {#each timelineNodes as node}
                  <div
                    class="tl-node-row"
                    role="button"
                    tabindex="0"
                    onclick={() => onSelectNode?.(node)}
                    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectNode?.(node); } }}
                  >
                    <Badge variant={typeVariant(node.node_type)} value={node.node_type ?? '?'} />
                    <span class="tl-node-name">{node.name}</span>
                    <span class="tl-node-file">{node.file_path ?? ''}</span>
                    {#if node.spec_path}
                      <span class="tl-node-spec">{node.spec_path.split('/').pop()}</span>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          <!-- Selected delta card -->
          {#if selectedDelta}
            <div class="delta-card" role="complementary" aria-label="Delta details">
              <div class="delta-card-header">
                <code class="delta-sha">{selectedDelta.commit_sha?.slice(0, 7) ?? '???????'}</code>
                <span class="delta-time">{relativeTime(selectedDelta.timestamp)}</span>
                <button class="delta-close" onclick={() => (selectedDelta = null)} aria-label="Close delta card">✕</button>
              </div>
              {#if selectedDelta.spec_ref}
                <div class="delta-row">
                  <span class="delta-label">Spec</span>
                  <span class="delta-value mono">{selectedDelta.spec_ref}</span>
                </div>
              {/if}
              {#if selectedDelta.agent_id}
                <div class="delta-row">
                  <span class="delta-label">Agent</span>
                  <span class="delta-value mono">{selectedDelta.agent_id}</span>
                </div>
              {/if}
              {#if selectedDelta.delta_json}
                <div class="delta-row">
                  <span class="delta-label">Changes</span>
                  <span class="delta-value">{deltaJsonSummary(selectedDelta.delta_json).join(', ') || '—'}</span>
                </div>
              {/if}
            </div>
          {/if}

          <!-- Scrubber -->
          <div class="scrubber-area" aria-label="Timeline scrubber">
            <!-- Delta markers -->
            <div class="marker-track" aria-hidden="true">
              {#each sortedDeltas as delta}
                <button
                  class="delta-marker"
                  class:active={selectedDelta?.id === delta.id}
                  style="left: {markerLeft(delta.timestamp)}"
                  title="{delta.commit_sha?.slice(0, 7)} · {relativeTime(delta.timestamp)}"
                  onclick={() => selectDelta(delta)}
                  aria-label="Delta at {relativeTime(delta.timestamp)}"
                >
                  <span class="marker-pip"></span>
                </button>
              {/each}
            </div>

            <input
              type="range"
              class="scrubber-input"
              min={minTs * 1000}
              max={maxTs * 1000}
              step={1000}
              value={scrubberValue}
              oninput={onScrubberChange}
              aria-label="Scrub architectural timeline"
            />

            <div class="scrubber-labels">
              <span class="scrubber-label-left">{relativeTime(minTs)}</span>
              <span class="scrubber-label-right">Now</span>
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .moldable-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  /* View tabs */
  .view-tabs {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    padding: 0 var(--space-4);
  }

  .view-tab {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .view-tab:hover {
    color: var(--color-text-secondary);
  }

  .view-tab.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
  }

  .view-tab:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .view-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* List view */
  .list-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .list-controls {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .filter-select {
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .sort-group {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .sort-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .sort-btn {
    padding: 2px var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    cursor: pointer;
    font-family: var(--font-body);
    transition: all var(--transition-fast);
  }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .sort-btn.active {
    background: var(--color-surface-elevated);
    border-color: var(--color-border-strong);
    color: var(--color-text);
  }

  .list-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    margin-left: auto;
  }

  .list-table-wrap {
    flex: 1;
    overflow: auto;
  }

  .list-empty-concept {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: var(--space-8);
  }

  .list-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .list-table th {
    position: sticky;
    top: 0;
    background: var(--color-surface-elevated);
    padding: var(--space-2) var(--space-3);
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    border-bottom: 1px solid var(--color-border);
    white-space: nowrap;
  }

  .list-row {
    cursor: pointer;
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }

  .list-row:hover {
    background: var(--color-surface-elevated);
  }

  .list-row td {
    padding: var(--space-2) var(--space-3);
    vertical-align: middle;
    color: var(--color-text);
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .muted {
    color: var(--color-text-muted);
  }

  .spec-tag {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }

  .muted-dash {
    color: var(--color-text-muted);
  }

  /* Timeline */
  .timeline-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .timeline-header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .timeline-title {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
  }

  .timeline-hint {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .timeline-meta {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .now-btn {
    margin-left: auto;
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-xs);
    font-family: var(--font-body);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .now-btn:hover {
    background: var(--color-primary);
    border-color: var(--color-primary);
    color: var(--color-text-inverse, #fff);
  }

  .timeline-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .timeline-spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--color-border);
    border-top-color: var(--color-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Node list */
  .timeline-nodes-area {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .timeline-node-list {
    display: flex;
    flex-direction: column;
    padding: var(--space-2) 0;
  }

  .tl-node-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-sm);
    transition: background var(--transition-fast);
    cursor: pointer;
  }

  .tl-node-row:hover {
    background: var(--color-surface-elevated);
  }

  .tl-node-name {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text);
    font-weight: 600;
  }

  .tl-node-file {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tl-node-spec {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }

  .timeline-empty-inner {
    padding: var(--space-6);
    text-align: center;
  }

  .no-nodes-msg {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
  }

  /* Delta card */
  .delta-card {
    margin: var(--space-2) var(--space-4);
    padding: var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  .delta-card-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .delta-sha {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 1px 5px;
    color: var(--color-text);
  }

  .delta-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .delta-close {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0 var(--space-1);
    line-height: 1;
  }

  .delta-row {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    font-size: var(--text-xs);
    margin-bottom: var(--space-1);
  }

  .delta-label {
    color: var(--color-text-muted);
    font-weight: 600;
    min-width: 50px;
    flex-shrink: 0;
  }

  .delta-value {
    color: var(--color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Scrubber */
  .scrubber-area {
    flex-shrink: 0;
    padding: var(--space-3) var(--space-4) var(--space-2);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface);
    position: relative;
  }

  .marker-track {
    position: relative;
    height: 20px;
    margin-bottom: var(--space-1);
    pointer-events: none;
  }

  .delta-marker {
    position: absolute;
    transform: translateX(-50%);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    pointer-events: all;
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .marker-pip {
    display: block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-primary);
    opacity: 0.6;
    transition: all var(--transition-fast);
    margin-top: 6px;
  }

  .delta-marker:hover .marker-pip,
  .delta-marker.active .marker-pip {
    opacity: 1;
    transform: scale(1.4);
  }

  .scrubber-input {
    width: 100%;
    accent-color: var(--color-primary);
    cursor: pointer;
    display: block;
  }

  .scrubber-labels {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-top: var(--space-1);
  }

  .scrubber-label-left,
  .scrubber-label-right {
    font-family: var(--font-mono);
  }
</style>
