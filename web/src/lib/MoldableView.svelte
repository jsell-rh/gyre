<script>
  import ExplorerCanvas from './ExplorerCanvas.svelte';
  import FlowRenderer from './FlowRenderer.svelte';
  import Badge from './Badge.svelte';
  import EmptyState from './EmptyState.svelte';
  import { api } from './api.js';
  import { toast as showToast } from './toast.svelte.js';
  import { entityName, shortId } from './entityNames.svelte.js';
  import { t } from 'svelte-i18n';

  let {
    nodes = [],
    edges = [],
    repoId = '',
    onSelectNode = undefined,
    conceptFilterIds = null,
    conceptQuery = '',
    categoryFilters = null,
    nodeTypeFilter = null,
  } = $props();

  // Category → node_type mapping for filter panel integration
  const CATEGORY_NODE_TYPES = {
    boundaries: new Set(['Module', 'Crate', 'Boundary', 'Container', 'System']),
    interfaces: new Set(['Trait', 'Interface', 'Function', 'Endpoint', 'Method']),
    data: new Set(['Struct', 'Enum', 'Type', 'ValueObject', 'Entity']),
    specs: new Set(['Spec']),
  };

  let activeView = $state('graph'); // 'graph' | 'list' | 'timeline' | 'flow'

  // List view sort
  let sortBy = $state('type'); // 'type' | 'name' | 'file'
  let sortDir = $state('asc'); // 'asc' | 'desc'
  let filterType = $state('');

  let nodeTypes = $derived.by(() => {
    const types = new Set(nodes.map(n => n.node_type).filter(Boolean));
    return ['', ...Array.from(types).sort()];
  });

  // Nodes/edges visible in the current concept + category filters
  function passesCategoryFilter(node) {
    if (!categoryFilters?.categories) return true;
    const cats = categoryFilters.categories;
    // If all categories selected, no filtering needed
    if (cats.length >= Object.keys(CATEGORY_NODE_TYPES).length) return true;
    const nodeType = node.node_type ?? '';
    const activeSets = cats.map(c => CATEGORY_NODE_TYPES[c]).filter(Boolean);
    // Show node if its type matches any active category, or if type is unknown
    if (activeSets.length === 0) return true;
    return activeSets.some(s => s.has(nodeType));
  }

  let displayNodes = $derived.by(() => {
    let result = nodes;
    if (conceptFilterIds) result = result.filter(n => conceptFilterIds.has(n.id));
    if (categoryFilters?.categories) result = result.filter(passesCategoryFilter);
    if (nodeTypeFilter?.length) result = result.filter(n => nodeTypeFilter.includes(n.node_type));
    return result;
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
    if (categoryFilters?.categories) result = result.filter(passesCategoryFilter);
    if (nodeTypeFilter?.length) result = result.filter(n => nodeTypeFilter.includes(n.node_type));
    if (filterType) result = result.filter(n => n.node_type === filterType);
    const dir = sortDir === 'asc' ? 1 : -1;
    return [...result].sort((a, b) => {
      if (sortBy === 'type') return dir * (a.node_type ?? '').localeCompare(b.node_type ?? '');
      if (sortBy === 'name') return dir * (a.name ?? '').localeCompare(b.name ?? '');
      if (sortBy === 'file') return dir * (a.file_path ?? '').localeCompare(b.file_path ?? '');
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

  // ── Flow (trace) state ───────────────────────────────────────────────────────
  let flowSpans = $state([]);
  let flowLoading = $state(false);
  let flowError = $state(null);

  // Load traces when Flow tab is active and repoId is set
  $effect(() => {
    if (activeView === 'flow' && repoId) {
      fetchFlowTraces();
    }
  });

  async function fetchFlowTraces() {
    flowLoading = true;
    flowError = null;
    flowSpans = [];
    try {
      // Fetch MRs for this repo, pick the most recently updated one with a trace
      const mrs = await api.mergeRequests({ repository_id: repoId });
      const mrList = Array.isArray(mrs) ? mrs : (mrs?.items ?? []);
      for (const mr of mrList) {
        try {
          const trace = await api.mrTrace(mr.id);
          if (trace?.spans?.length) {
            // Normalize start_time to be relative (API returns epoch microseconds).
            const minStart = Math.min(...trace.spans.map(s => s.start_time));
            flowSpans = trace.spans.map(s => ({
              id: s.span_id,
              parent_id: s.parent_span_id ?? null,
              node_id: s.graph_node_id ?? null,
              start_time: s.start_time - minStart,
              duration_us: s.duration_us,
              status: s.status,
              name: s.operation_name,
            }));
            return;
          }
        } catch (_e) {
          // 404 or no trace — try next MR
        }
      }
      // No traces found for any MR
      flowSpans = [];
    } catch (_e) {
      flowError = _e.message ?? $t('moldable_view.flow_load_failed');
      flowSpans = [];
    } finally {
      flowLoading = false;
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
      showToast($t('moldable_view.timeline_load_failed') + ': ' + _e.message, { type: 'error' });
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
    if (diff < 60) return $t('moldable_view.time_just_now');
    if (diff < 3600) return $t('moldable_view.time_minutes_ago', { values: { count: Math.floor(diff / 60) } });
    if (diff < 86400) return $t('moldable_view.time_hours_ago', { values: { count: Math.floor(diff / 3600) } });
    return $t('moldable_view.time_days_ago', { values: { count: Math.floor(diff / 86400) } });
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
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div class="view-tabs" role="tablist" aria-label={$t('moldable_view.view_mode')}
    onkeydown={(e) => {
      const views = ['graph', 'list', 'timeline', 'flow'];
      const idx = views.indexOf(activeView);
      if (e.key === 'ArrowRight') { e.preventDefault(); const ni = (idx + 1) % views.length; activeView = views[ni]; document.getElementById('tab-' + views[ni])?.focus(); }
      if (e.key === 'ArrowLeft')  { e.preventDefault(); const ni = (idx - 1 + views.length) % views.length; activeView = views[ni]; document.getElementById('tab-' + views[ni])?.focus(); }
      if (e.key === 'Home') { e.preventDefault(); activeView = views[0]; document.getElementById('tab-' + views[0])?.focus(); }
      if (e.key === 'End')  { e.preventDefault(); activeView = views[views.length - 1]; document.getElementById('tab-' + views[views.length - 1])?.focus(); }
    }}
  >
    <button
      class="view-tab"
      id="tab-graph"
      class:active={activeView === 'graph'}
      role="tab"
      aria-selected={activeView === 'graph'}
      aria-controls="tabpanel-graph"
      tabindex={activeView === 'graph' ? 0 : -1}
      onclick={() => (activeView = 'graph')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/>
        <path d="M7 12h10M17 7l-10 4M17 17L7 13"/>
      </svg>
      {$t('moldable_view.tab_graph')}
    </button>
    <button
      class="view-tab"
      id="tab-list"
      class:active={activeView === 'list'}
      role="tab"
      aria-selected={activeView === 'list'}
      aria-controls="tabpanel-list"
      tabindex={activeView === 'list' ? 0 : -1}
      onclick={() => (activeView = 'list')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <line x1="8" y1="6" x2="21" y2="6"/><line x1="8" y1="12" x2="21" y2="12"/><line x1="8" y1="18" x2="21" y2="18"/>
        <line x1="3" y1="6" x2="3.01" y2="6"/><line x1="3" y1="12" x2="3.01" y2="12"/><line x1="3" y1="18" x2="3.01" y2="18"/>
      </svg>
      {$t('moldable_view.tab_list')}
    </button>
    <button
      class="view-tab"
      id="tab-timeline"
      class:active={activeView === 'timeline'}
      role="tab"
      aria-selected={activeView === 'timeline'}
      aria-controls="tabpanel-timeline"
      tabindex={activeView === 'timeline' ? 0 : -1}
      onclick={() => (activeView = 'timeline')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/>
      </svg>
      {$t('moldable_view.tab_timeline')}
    </button>
    <button
      class="view-tab"
      id="tab-flow"
      class:active={activeView === 'flow'}
      role="tab"
      aria-selected={activeView === 'flow'}
      aria-controls="tabpanel-flow"
      tabindex={activeView === 'flow' ? 0 : -1}
      onclick={() => (activeView = 'flow')}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <circle cx="5" cy="12" r="2"/><circle cx="19" cy="5" r="2"/><circle cx="19" cy="19" r="2"/>
        <path d="M7 11.5l9-5M7 12.5l9 5"/>
      </svg>
      {$t('moldable_view.tab_flow')}
    </button>
  </div>

  <!-- View content -->
  <div class="view-content" role="tabpanel" id="tabpanel-{activeView}" aria-labelledby="tab-{activeView}">
    {#if activeView === 'graph'}
      <ExplorerCanvas nodes={displayNodes} edges={displayEdges} {repoId} {onSelectNode} />

    {:else if activeView === 'list'}
      <div class="list-view">
        <div class="list-controls">
          <select
            class="filter-select"
            value={filterType}
            onchange={(e) => (filterType = e.target.value)}
            aria-label={$t('moldable_view.filter_by_type')}
          >
            {#each nodeTypes as nt}
              <option value={nt}>{nt || $t('moldable_view.all_types')}</option>
            {/each}
          </select>
          <div class="sort-group" role="group" aria-label={$t('moldable_view.sort_by')}>
            <span class="sort-label">{$t('moldable_view.sort_label')}:</span>
            {#each [['type', $t('moldable_view.col_type')], ['name', $t('moldable_view.col_name')], ['file', $t('moldable_view.col_file')]] as [val, label]}
              <button
                class="sort-btn"
                class:active={sortBy === val}
                aria-pressed={sortBy === val}
                onclick={() => { if (sortBy === val) { sortDir = sortDir === 'asc' ? 'desc' : 'asc'; } else { sortBy = val; sortDir = 'asc'; } }}
              >{label}{sortBy === val ? (sortDir === 'asc' ? ' \u2191' : ' \u2193') : ''}</button>
            {/each}
          </div>
          <span class="list-count">{filteredNodes.length} {filteredNodes.length === 1 ? $t('moldable_view.node') : $t('moldable_view.nodes')}</span>
        </div>

        <div class="list-table-wrap">
          {#if filteredNodes.length === 0 && conceptFilterIds}
            <div class="list-empty-concept">
              <EmptyState
                title={$t('moldable_view.no_nodes_matching', { values: { query: conceptQuery } })}
                description={$t('moldable_view.no_nodes_matching_desc')}
              />
            </div>
          {:else if filteredNodes.length === 0 && !conceptFilterIds}
            <div class="list-empty-concept">
              <EmptyState
                title={$t('moldable_view.no_graph_data')}
                description={$t('moldable_view.no_graph_data_desc')}
              />
            </div>
          {:else}
            <table class="list-table">
              <thead>
                <tr>
                  <th scope="col">{$t('moldable_view.col_type')}</th>
                  <th scope="col">{$t('moldable_view.col_name')}</th>
                  <th scope="col">{$t('moldable_view.col_file')}</th>
                  <th scope="col">{$t('moldable_view.col_spec')}</th>
                  <th scope="col">{$t('moldable_view.col_churn')}</th>
                </tr>
              </thead>
              <tbody>
                {#each filteredNodes as node}
                  <tr
                    class="list-row"
                    tabindex="0"
                    aria-label={$t('moldable_view.select_node', { values: { name: node.name } })}
                    onclick={() => onSelectNode?.(node)}
                    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectNode?.(node); } }}
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
      <div class="flow-container">
        {#if flowLoading}
          <div class="flow-status-bar" role="status" aria-label={$t('moldable_view.loading_traces')}>
            <div class="flow-spinner-inline" aria-hidden="true"></div>
            <span>{$t('moldable_view.loading_traces')}</span>
          </div>
        {:else if flowError}
          <div class="flow-status-bar flow-status-error" role="alert">
            <span>{$t('moldable_view.flow_error', { values: { error: flowError } })}</span>
          </div>
        {:else if repoId && flowSpans.length === 0}
          <div class="flow-status-bar">
            <span>{$t('moldable_view.no_traces')}</span>
          </div>
        {/if}
        <FlowRenderer
          nodes={displayNodes}
          edges={displayEdges}
          {repoId}
          spans={flowSpans}
        />
      </div>

    {:else if activeView === 'timeline'}
      <div class="timeline-view" aria-busy={timelineLoading}>
        <span class="sr-only" aria-live="polite">{timelineLoading ? "" : $t('moldable_view.timeline_loaded')}</span>
        <div class="timeline-header">
          <h3 class="timeline-title">{$t('moldable_view.architectural_timeline')}</h3>
          {#if !repoId}
            <p class="timeline-hint">{$t('moldable_view.select_repo_hint')}</p>
          {:else if timelineLoading}
            <p class="timeline-hint">{$t('moldable_view.loading_timeline')}</p>
          {:else if timelineDeltas.length > 0}
            <span class="timeline-meta">
              {timelineDeltas.length} {timelineDeltas.length !== 1 ? $t('moldable_view.deltas') : $t('moldable_view.delta')}
              · {timelineNodes.length} / {nodes.length} {nodes.length === 1 ? $t('moldable_view.node') : $t('moldable_view.nodes')} {$t('moldable_view.visible')}
            </span>
          {/if}
          {#if repoId && !timelineLoading && timelineDeltas.length > 0}
            <button class="now-btn" onclick={resetToNow} title={$t('moldable_view.jump_to_now')}>{$t('moldable_view.now')}</button>
          {/if}
        </div>

        {#if !repoId || (!timelineLoading && timelineDeltas.length === 0)}
          <div class="timeline-empty">
            <EmptyState
              title={$t('moldable_view.no_changes_title')}
              description={repoId ? $t('moldable_view.no_changes_desc_repo') : $t('moldable_view.no_changes_desc_no_repo')}
            />
          </div>
        {:else if timelineLoading}
          <div class="timeline-empty">
            <div class="timeline-spinner" role="status" aria-label={$t('moldable_view.loading_timeline')}></div>
          </div>
        {:else}
          <!-- Node list filtered by scrubber position -->
          <div class="timeline-nodes-area">
            {#if timelineNodes.length === 0}
              <div class="timeline-empty-inner">
                <p class="no-nodes-msg">{$t('moldable_view.no_nodes_at_point')}</p>
              </div>
            {:else}
              <div class="timeline-node-list">
                {#each timelineNodes as node}
                  <div
                    class="tl-node-row"
                    role="button"
                    tabindex="0"
                    aria-label="{node.name} ({node.node_type})"
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
            <div class="delta-card" role="complementary" aria-label={$t('moldable_view.delta_details')}>
              <div class="delta-card-header">
                <code class="delta-sha">{selectedDelta.commit_sha?.slice(0, 7) ?? '???????'}</code>
                <span class="delta-time">{relativeTime(selectedDelta.timestamp)}</span>
                <button class="delta-close" onclick={() => (selectedDelta = null)} aria-label={$t('moldable_view.close_delta_card')} title={$t('common.close')}>✕</button>
              </div>
              {#if selectedDelta.spec_ref}
                <div class="delta-row">
                  <span class="delta-label">{$t('moldable_view.delta_spec')}</span>
                  <span class="delta-value mono">{selectedDelta.spec_ref}</span>
                </div>
              {/if}
              {#if selectedDelta.agent_id}
                <div class="delta-row">
                  <span class="delta-label">{$t('moldable_view.delta_agent')}</span>
                  <span class="delta-value mono">{entityName('agent', selectedDelta.agent_id)}</span>
                </div>
              {/if}
              {#if selectedDelta.delta_json}
                <div class="delta-row">
                  <span class="delta-label">{$t('moldable_view.delta_changes')}</span>
                  <span class="delta-value">{deltaJsonSummary(selectedDelta.delta_json).join(', ') || '—'}</span>
                </div>
              {/if}
            </div>
          {/if}

          <!-- Scrubber -->
          <div class="scrubber-area" aria-label={$t('moldable_view.timeline_scrubber')}>
            <!-- Delta markers -->
            <div class="marker-track">
              {#each sortedDeltas as delta}
                <button
                  class="delta-marker"
                  class:active={selectedDelta?.id === delta.id}
                  style="left: {markerLeft(delta.timestamp)}"
                  title="{delta.commit_sha?.slice(0, 7)} · {relativeTime(delta.timestamp)}"
                  onclick={() => selectDelta(delta)}
                  aria-label={$t('moldable_view.delta_at', { values: { time: relativeTime(delta.timestamp) } })}
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
              aria-label={$t('moldable_view.scrub_timeline')}
            />

            <div class="scrubber-labels">
              <span class="scrubber-label-left">{relativeTime(minTs)}</span>
              <span class="scrubber-label-right">{$t('moldable_view.now')}</span>
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
    border-bottom-color: var(--color-focus);
  }

  .view-tab:focus-visible {
    outline: 2px solid var(--color-focus);
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

  .filter-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    cursor: pointer;
    font-family: var(--font-body);
    transition: all var(--transition-fast);
  }

  .sort-btn:hover { background: var(--color-surface-elevated); color: var(--color-text); }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
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

  .list-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
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
    color: var(--color-link);
    background: color-mix(in srgb, var(--color-link) 8%, transparent);
    padding: var(--space-1);
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
    background: var(--color-focus);
    border-color: var(--color-focus);
    color: var(--color-text-inverse);
  }

  .now-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .tl-node-row:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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
    color: var(--color-link);
    background: color-mix(in srgb, var(--color-link) 8%, transparent);
    padding: var(--space-1);
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
    padding: var(--space-1) var(--space-2);
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
    transition: color var(--transition-fast);
  }

  .delta-close:hover {
    color: var(--color-text);
  }

  .delta-close:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
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

  .delta-marker:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .marker-pip {
    display: block;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-info);
    opacity: 0.6;
    transition: all var(--transition-fast);
    margin-top: var(--space-1);
  }

  .delta-marker:hover .marker-pip,
  .delta-marker.active .marker-pip {
    opacity: 1;
    transform: scale(1.4);
  }

  .scrubber-input {
    width: 100%;
    accent-color: var(--color-focus);
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

  /* Flow view */
  .flow-container {
    display: flex;
    flex-direction: column;
    flex: 1;
    overflow: hidden;
  }

  .flow-status-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-4);
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .flow-status-error {
    color: var(--color-danger);
  }

  .flow-spinner-inline {
    width: 14px;
    height: 14px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .timeline-spinner,
    .flow-spinner-inline { animation: none; border-color: var(--color-border); border-top-color: var(--color-text-muted); }
    .view-tab,
    .tl-node-row,
    .list-row,
    .marker-pip,
    .now-btn,
    .sort-btn,
    .filter-select,
    .delta-close { transition: none; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
