<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Badge from '../lib/Badge.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let graph = $state(null);
  let loading = $state(true);
  let selected = $state(null);

  let selectedOutEdges = $derived(selected && graph ? (graph.edges ?? []).filter(e => e.from === nodeId(selected)) : []);
  let selectedInEdges = $derived(selected && graph ? (graph.edges ?? []).filter(e => e.to === nodeId(selected)) : []);
  let svgW = $state(900);
  let svgH = $state(560);
  let nodePositions = $state({});

  const LINK_TYPES = [
    { type: 'implements',    color: '#22c55e', label: 'Implements' },
    { type: 'supersedes',    color: '#f59e0b', label: 'Supersedes' },
    { type: 'depends_on',    color: '#4a9eff', label: 'Depends on' },
    { type: 'conflicts_with', color: '#ef4444', label: 'Conflicts' },
    { type: 'extends',       color: '#a855f7', label: 'Extends' },
    { type: 'references',    color: '#6b7280', label: 'References' },
  ];

  function linkColor(type) {
    return LINK_TYPES.find(t => t.type === type)?.color ?? '#6b7280';
  }

  $effect(() => { load(); });

  async function load() {
    loading = true;
    try {
      graph = await api.specsGraph();
      layoutGraph();
    } catch (e) {
      showToast('Failed to load spec graph: ' + e.message, { type: 'error' });
    } finally {
      loading = false;
    }
  }

  function layoutGraph() {
    if (!graph?.nodes?.length) return;
    const nodes = graph.nodes;
    const n = nodes.length;
    const positions = {};

    // Simple hierarchical layout by approval status
    const approved = nodes.filter(nd => nd.approval_status === 'approved' || nd.approval_status === 'Approved');
    const pending = nodes.filter(nd => !approved.includes(nd));
    const allGroups = [approved, pending];
    let yi = 0;
    allGroups.forEach(group => {
      group.forEach((node, i) => {
        const cols = Math.max(1, Math.ceil(Math.sqrt(group.length)));
        positions[node.id ?? node.path] = {
          x: 80 + (i % cols) * ((svgW - 160) / Math.max(1, cols - 1) || (svgW - 160)),
          y: 80 + yi * 120,
        };
        if ((i + 1) % cols === 0) yi++;
      });
      if (group.length > 0) yi++;
    });
    nodePositions = positions;
  }

  function nodeId(node) {
    return node.id ?? node.path;
  }

  function getPos(id) {
    return nodePositions[id] ?? { x: svgW / 2, y: svgH / 2 };
  }

  function statusVariant(s) {
    const v = (s ?? '').toLowerCase();
    if (v === 'approved') return 'success';
    if (v === 'pending') return 'warning';
    return 'default';
  }

  function shortPath(path) {
    if (!path) return '';
    const parts = path.split('/');
    return parts[parts.length - 1]?.replace('.md', '') ?? path;
  }
</script>

<div class="spec-graph-view">
  <div class="view-header">
    <div>
      <h2>Spec Link Graph</h2>
      <p class="subtitle">DAG of spec relationships — click a node for details</p>
    </div>
    <div class="legend">
      {#each LINK_TYPES as lt}
        <span class="legend-item">
          <svg width="18" height="8" viewBox="0 0 18 8" aria-hidden="true">
            <line x1="0" y1="4" x2="14" y2="4" stroke={lt.color} stroke-width="2"/>
            <path d="M14,1 L18,4 L14,7" fill="none" stroke={lt.color} stroke-width="1.5"/>
          </svg>
          {lt.label}
        </span>
      {/each}
    </div>
  </div>

  <div class="graph-area">
    {#if loading}
      <div class="loading-box"><Skeleton lines={8} /></div>
    {:else if !graph?.nodes?.length}
      <EmptyState
        title="No spec links"
        message="No spec link relationships found. Add links (implements, supersedes, depends_on, etc.) to spec frontmatter to see them here."
      />
    {:else}
      <div class="graph-container">
        <svg
          viewBox="0 0 {svgW} {svgH}"
          class="spec-svg"
          role="img"
          aria-label="Spec link graph"
        >
          <defs>
            {#each LINK_TYPES as lt}
              <marker
                id="arr-{lt.type}"
                markerWidth="8" markerHeight="8"
                refX="6" refY="3"
                orient="auto"
              >
                <path d="M0,0 L0,6 L8,3 z" fill={lt.color} />
              </marker>
            {/each}
          </defs>

          <!-- Edges -->
          {#each graph.edges as edge}
            {@const from = getPos(edge.from)}
            {@const to = getPos(edge.to)}
            {@const color = linkColor(edge.link_type)}
            <line
              x1={from.x} y1={from.y}
              x2={to.x} y2={to.y}
              stroke={color}
              stroke-width="1.5"
              stroke-opacity="0.7"
              marker-end="url(#arr-{edge.link_type ?? 'references'})"
            />
          {/each}

          <!-- Nodes -->
          {#each graph.nodes as node}
            {@const id = nodeId(node)}
            {@const pos = getPos(id)}
            {@const isSelected = selected && nodeId(selected) === id}
            <g
              class="spec-node"
              transform="translate({pos.x},{pos.y})"
              role="button"
              tabindex="0"
              aria-label="Spec: {node.title ?? node.path}"
              onclick={() => (selected = node)}
              onkeydown={(e) => e.key === 'Enter' && (selected = node)}
            >
              <rect
                class="node-rect"
                class:selected={isSelected}
                x="-48" y="-18"
                width="96" height="36"
                rx="6"
                stroke-width={isSelected ? 2 : 1}
              />
              <text
                text-anchor="middle"
                dominant-baseline="middle"
                y="-4"
                font-size="8"
                fill="var(--color-text)"
                font-family="var(--font-mono)"
              >
                {shortPath(node.path ?? node.id).substring(0, 16)}
              </text>
              <text
                text-anchor="middle"
                dominant-baseline="middle"
                y="9"
                font-size="7"
                fill={node.approval_status?.toLowerCase() === 'approved' ? '#22c55e' : '#f59e0b'}
              >
                {node.approval_status ?? 'pending'}
              </text>
            </g>
          {/each}
        </svg>

        <!-- Side panel -->
        {#if selected}
          <div class="detail-panel">
            <div class="panel-header">
              <span class="panel-title">{selected.title ?? shortPath(selected.path)}</span>
              <button class="close-btn" onclick={() => (selected = null)} aria-label="Close"><span aria-hidden="true">×</span></button>
            </div>
            <div class="panel-field">
              <span class="field-label">Path</span>
              <span class="mono-sm">{selected.path ?? '—'}</span>
            </div>
            <div class="panel-field">
              <span class="field-label">Status</span>
              <Badge variant={statusVariant(selected.approval_status)} value={selected.approval_status ?? 'pending'} />
            </div>
            {#if selected.title}
              <div class="panel-field">
                <span class="field-label">Title</span>
                <span class="text-sm">{selected.title}</span>
              </div>
            {/if}
            <!-- Edges involving this node -->
            {#if selectedOutEdges.length > 0}
              <div class="panel-section">
                <div class="panel-label">Outbound Links</div>
                {#each selectedOutEdges as edge}
                  <div class="edge-row">
                    <span class="edge-type" style="color:{linkColor(edge.link_type)}">{edge.link_type}</span>
                    <span class="mono-sm">{shortPath(edge.to)}</span>
                  </div>
                {/each}
              </div>
            {/if}
            {#if selectedInEdges.length > 0}
              <div class="panel-section">
                <div class="panel-label">Inbound Links</div>
                {#each selectedInEdges as edge}
                  <div class="edge-row">
                    <span class="mono-sm">{shortPath(edge.from)}</span>
                    <span class="edge-type" style="color:{linkColor(edge.link_type)}">→ {edge.link_type}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .spec-graph-view { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .view-header h2 { margin: 0 0 var(--space-1); font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .subtitle { margin: 0; font-size: var(--text-sm); color: var(--color-text-secondary); }

  .legend { display: flex; flex-wrap: wrap; gap: var(--space-3); align-items: center; }
  .legend-item { display: flex; align-items: center; gap: var(--space-1); font-size: var(--text-xs); color: var(--color-text-muted); white-space: nowrap; }

  .graph-area { flex: 1; overflow: hidden; }
  .loading-box { padding: var(--space-6); }

  .graph-container { display: flex; height: 100%; }

  .spec-svg {
    flex: 1;
    width: 100%;
    height: 100%;
    background: var(--color-surface);
    cursor: default;
  }

  .spec-node { cursor: pointer; }
  .spec-node:hover rect { stroke: var(--color-primary); }

  .node-rect {
    fill: var(--color-surface-elevated);
    stroke: var(--color-border-strong);
  }

  .node-rect.selected {
    fill: color-mix(in srgb, var(--color-primary) 12%, transparent);
    stroke: var(--color-primary);
  }

  .detail-panel {
    width: 280px;
    flex-shrink: 0;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    padding: var(--space-4);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .panel-header { display: flex; justify-content: space-between; align-items: center; }
  .panel-title { font-weight: 600; color: var(--color-text); font-size: var(--text-sm); flex: 1; overflow: hidden; text-overflow: ellipsis; }
  .close-btn { background: transparent; border: none; color: var(--color-text-muted); cursor: pointer; font-size: var(--text-lg); line-height: 1; flex-shrink: 0; }
  .close-btn:hover { color: var(--color-text); }
  .close-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .panel-field { display: flex; flex-direction: column; gap: 2px; }
  .field-label { font-size: var(--text-xs); color: var(--color-text-muted); text-transform: uppercase; letter-spacing: 0.05em; }
  .text-sm { font-size: var(--text-sm); color: var(--color-text); }

  .panel-section { display: flex; flex-direction: column; gap: var(--space-2); }
  .panel-label { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--color-text-muted); }

  .edge-row { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-xs); }
  .edge-type { font-weight: 500; white-space: nowrap; }
  .mono-sm { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--color-text); word-break: break-all; }
</style>
