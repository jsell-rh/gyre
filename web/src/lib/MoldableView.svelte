<script>
  import ExplorerCanvas from './ExplorerCanvas.svelte';
  import Badge from './Badge.svelte';

  let {
    nodes = [],
    edges = [],
    onSelectNode = undefined,
  } = $props();

  let activeView = $state('graph'); // 'graph' | 'list' | 'timeline'

  // List view sort
  let sortBy = $state('type'); // 'type' | 'name' | 'file'
  let filterType = $state('');

  let nodeTypes = $derived(() => {
    const types = new Set(nodes.map(n => n.node_type).filter(Boolean));
    return ['', ...Array.from(types).sort()];
  });

  let filteredNodes = $derived(() => {
    let result = nodes;
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
  </div>

  <!-- View content -->
  <div class="view-content">
    {#if activeView === 'graph'}
      <ExplorerCanvas {nodes} {edges} {onSelectNode} />

    {:else if activeView === 'list'}
      <div class="list-view">
        <div class="list-controls">
          <select
            class="filter-select"
            value={filterType}
            onchange={(e) => (filterType = e.target.value)}
            aria-label="Filter by node type"
          >
            {#each nodeTypes() as t}
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
          <span class="list-count">{filteredNodes().length} nodes</span>
        </div>

        <div class="list-table-wrap">
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
              {#each filteredNodes() as node}
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
        </div>
      </div>

    {:else if activeView === 'timeline'}
      <div class="timeline-stub">
        <div class="stub-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="48" height="48">
            <circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 3"/>
          </svg>
        </div>
        <h3>Architectural Timeline</h3>
        <p>Timeline view — scrub through the system's architectural history — is coming in Phase 4.</p>
        <p class="stub-detail">Will show knowledge graph state at any point in git history, highlighting additions, modifications, and removals.</p>
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
    background: rgba(238, 0, 0, 0.08);
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }

  .muted-dash {
    color: var(--color-text-muted);
  }

  /* Timeline stub */
  .timeline-stub {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    padding: var(--space-8);
    text-align: center;
    color: var(--color-text-secondary);
  }

  .stub-icon {
    color: var(--color-text-muted);
    opacity: 0.5;
  }

  .timeline-stub h3 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
  }

  .timeline-stub p {
    margin: 0;
    font-size: var(--text-sm);
    max-width: 400px;
    line-height: 1.6;
  }

  .stub-detail {
    color: var(--color-text-muted);
    font-size: var(--text-xs) !important;
  }
</style>
